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

// ---------- plans ----------

/// What a plan shows at a point of the model, as the PNG has it.
fn plan_pixel(s: &Session, at: crate::geom::P2) -> (u8, u8, u8) {
    let level = s.model.levels()[0].0;
    let d = crate::render::plan(&s.model, level, 1600, 400);
    let png = crate::render::png(&d).unwrap();
    let (x, y) = d.view.px(at);
    let c = tiny_skia::Pixmap::decode_png(&png).unwrap().pixel(x as u32, y as u32).unwrap();
    (c.red(), c.green(), c.blue())
}

#[test]
fn a_plan_fills_the_mullions_it_cuts_and_not_the_glass_between() {
    use crate::geom::P2;
    let mut s = session();
    grid(&mut s, "CW", &[2000.0], &[3000.0], json!({}));
    let id = wall(&mut s, "CW", 6000.0, 3000.0);
    let plane = curtain::grid_plane(&s.model, wall_of(&s, id));
    let dark = |c: (u8, u8, u8)| c.0 < 200 && c.1 < 200 && c.2 < 200;
    assert!(dark(plan_pixel(&s, P2(2000.0, plane))), "the mullion on the grid line is cut");
    // A little off the panel's plane, between its two faces and the wall's face: nothing is filled there.
    let off = plan_pixel(&s, P2(1000.0, plane + 60.0));
    assert!(!dark(off), "no poche across the glass: {off:?}");
}

#[test]
fn a_click_on_a_curtain_walls_plan_picks_the_wall() {
    use crate::geom::P2;
    let mut s = session();
    grid(&mut s, "CW", &[2000.0], &[3000.0], json!({}));
    let id = wall(&mut s, "CW", 6000.0, 3000.0);
    let plane = curtain::grid_plane(&s.model, wall_of(&s, id));
    let level = s.model.levels()[0].0;
    let d = crate::render::plan(&s.model, level, 1600, 400);
    assert_eq!(d.pick(d.view.px(P2(2000.0, plane)), 2.0), Some(id));
}

#[test]
fn a_plan_below_a_curtain_wall_shows_nothing_of_it_and_above_sees_it() {
    let mut s = session();
    grid(&mut s, "CW", &[2000.0], &[1000.0], json!({}));
    let id = wall(&mut s, "CW", 6000.0, 800.0);
    let level = s.model.levels()[0].0;
    let d = crate::render::plan(&s.model, level, 1600, 400);
    assert!(d.owners.contains(&Some(id)), "seen below the cut plane");
}

// ---------- IFC ----------

fn lines_of(f: &str, entity: &str) -> Vec<String> {
    f.lines().filter(|l| l.split_once('=').is_some_and(|(_, e)| e.starts_with(&format!("{entity}(")))).map(String::from).collect()
}

#[test]
fn ifc_writes_a_curtain_wall_as_the_sum_of_its_members_and_plates() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[1500.0], json!({"panels": [["Spandrel"]]}));
    let id = wall(&mut s, "CW", 3000.0, 3000.0);
    let l = layout_of(&s, id);
    let f = crate::ifc::export(&s.model);
    assert_eq!(lines_of(&f, "IFCWALL").len(), 0, "not a solid wall");
    let wall = lines_of(&f, "IFCCURTAINWALL");
    assert_eq!(wall.len(), 1);
    assert!(wall[0].contains(",$,'"), "no body of its own: its parts are its shape: {}", wall[0]);
    let (members, plates) = (lines_of(&f, "IFCMEMBER"), lines_of(&f, "IFCPLATE"));
    assert_eq!(members.len(), count(&l, PartKind::Mullion));
    assert_eq!(plates.len(), count(&l, PartKind::Panel));
    assert!(members.iter().all(|m| m.ends_with(".MULLION.);")), "{}", members[0]);
    assert!(plates.iter().all(|p| p.ends_with(".CURTAIN_PANEL.);")), "{}", plates[0]);
    // The parts are aggregated into the wall.
    let wall_ref = wall[0].split_once('=').unwrap().0;
    assert!(f.lines().any(|x| x.contains("IFCRELAGGREGATES(") && x.contains(&format!(",{wall_ref},("))), "{wall_ref} aggregates its parts");
    assert_eq!(lines_of(&f, "IFCCURTAINWALLTYPE").len(), 1);
    assert_eq!(lines_of(&f, "IFCPLATETYPE").len(), 1);
    assert_eq!(lines_of(&f, "IFCMEMBERTYPE").len(), 1);
}

#[test]
fn ifc_writes_identical_parts_with_one_shape_and_each_its_own_lasting_global_id() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[1000.0], json!({}));
    let id = wall(&mut s, "CW", 20_000.0, 5000.0);
    let l = layout_of(&s, id);
    let f = crate::ifc::export(&s.model);
    let plates = lines_of(&f, "IFCPLATE");
    assert_eq!(plates.len(), 100);
    let shape = |line: &str| line.split(',').nth(6).unwrap().to_string();
    let shapes: std::collections::BTreeSet<String> = plates.iter().map(|p| shape(p)).collect();
    assert!(shapes.len() < 10, "{} shapes for 100 plates", shapes.len());
    assert!(lines_of(&f, "IFCPRODUCTDEFINITIONSHAPE").len() < l.shapes.len() + 10);
    // Each has its own GlobalId, the same at every export.
    let ids = |f: &str| lines_of(f, "IFCPLATE").iter().map(|p| p.split('\'').nth(1).unwrap().to_string()).collect::<Vec<_>>();
    let once = ids(&f);
    assert_eq!(once.iter().collect::<std::collections::BTreeSet<_>>().len(), 100);
    assert_eq!(once, ids(&crate::ifc::export(&s.model)));
}

// ---------- fit ----------

#[test]
fn a_fixed_grid_draws_a_wall_in_the_lengths_of_its_pattern() {
    let mut s = session();
    grid(&mut s, "CW", &[900.0, 600.0], &[3000.0], json!({}));
    let fixed = |s: &mut Session| {
        let mut l = s.model.types.wall["CW"].curtain.clone().unwrap();
        l.columns.fit = Fit::Fixed;
        l.rows.fit = Fit::Fixed;
        run(s, "type", json!({"category": "wall", "name": "CW", "curtain": l}));
    };
    fixed(&mut s);
    let d = s.model.types.wall["CW"].curtain.clone().unwrap().columns;
    assert_eq!(curtain::snap_length(&d, 1000.0), 900.0);
    assert_eq!(curtain::snap_length(&d, 1300.0), 1500.0);
    assert_eq!(curtain::snap_length(&d, 100.0), 900.0, "never shorter than its first segment");
    assert_eq!(curtain::snap_length(&d, 3100.0), 3000.0);
    let out = run(&mut s, "wall", json!({"points": [[0, 0], [3100, 0]], "type": "CW", "height": 3000}));
    let id = out["ids"][0].as_u64().unwrap();
    assert_eq!(wall_of(&s, id).end.0, 3000.0, "{out}");
    assert_eq!(out["adjusted"][0]["length"], 3000.0, "the reply says the length it used: {out}");
    // A chain goes on from where the last wall ended.
    let out = run(&mut s, "wall", json!({"points": [[0, 5000], [1000, 5000], [900, 7000]], "type": "CW", "height": 3000}));
    let (a, b) = (out["ids"][0].as_u64().unwrap(), out["ids"][1].as_u64().unwrap());
    assert_eq!(wall_of(&s, a).end.0, 900.0);
    assert_eq!(wall_of(&s, b).start.0, 900.0);
    assert_eq!(wall_of(&s, b).end.1, 5000.0 + 2400.0);
    // A height set directly snaps too; one a top level sets can't, and the top row takes up the difference.
    let id = run(&mut s, "wall", json!({"points": [[0, 9000], [900, 9000]], "type": "CW", "height": 4000}))["ids"][0].as_u64().unwrap();
    assert_eq!(wall_of(&s, id).height, 3000.0);
    let id =
        run(&mut s, "wall", json!({"points": [[0, 11000], [900, 11000]], "type": "CW", "top_level": "Level 2", "top_offset": 500}))["ids"]
            [0]
        .as_u64()
        .unwrap();
    assert_eq!(layout_of(&s, id).rows.len(), 2, "3500 tall: a whole row and what's left");
}

#[test]
fn a_flexible_grid_keeps_the_length_given() {
    let mut s = session();
    grid(&mut s, "CW", &[900.0], &[3000.0], json!({}));
    let out = run(&mut s, "wall", json!({"points": [[0, 0], [2000, 0]], "type": "CW", "height": 3000}));
    assert_eq!(wall_of(&s, out["ids"][0].as_u64().unwrap()).end.0, 2000.0);
    assert!(out.get("adjusted").is_none(), "{out}");
}

#[test]
fn a_malformed_wall_is_said_in_the_reply_and_the_warnings() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[3000.0], json!({}));
    let out = run(&mut s, "wall", json!({"points": [[0, 0], [3100, 0]], "type": "CW", "height": 3000}));
    let id = out["ids"][0].as_u64().unwrap();
    assert!(out["warning"].as_str().unwrap().contains(curtain::MALFORMED), "{out}");
    assert_eq!(curtain::malformed_walls(&s.model), [id]);
    assert!(s.model.warnings().iter().any(|w| w.contains(curtain::MALFORMED) && w.contains(&id.to_string())));
    // Fixed by Remove Last Interior Grid.
    run(&mut s, "modify", json!({"id": id, "curtain": {"absorb_columns": true}}));
    assert!(curtain::malformed_walls(&s.model).is_empty());
    assert!(!s.model.warnings().iter().any(|w| w.contains(curtain::MALFORMED)));
    // A wall that's fine says nothing.
    let out = run(&mut s, "wall", json!({"points": [[0, 5000], [3000, 5000]], "type": "CW", "height": 3000}));
    assert!(out.get("warning").is_none(), "{out}");
}

// Joins.

/// A pattern of `columns` by `rows` with its four borders the given mullion joined as `join`.
fn framed(s: &mut Session, name: &str, columns: &[f64], rows: &[f64], mullion: &str, join: &str) {
    let edge = json!({"type": mullion, "join": join});
    let verticals = vec![vec![Value::Null; columns.len()]; rows.len()];
    let horizontals = verticals.clone();
    let borders = json!({"left": edge, "right": edge, "bottom": edge, "top": edge});
    grid(s, name, columns, rows, json!({"vertical": verticals, "horizontal": horizontals, "borders": borders}));
}

fn mullions(l: &Layout) -> Vec<&curtain::Part> {
    l.parts.iter().filter(|p| l.shapes[p.shape as usize].kind == PartKind::Mullion).collect()
}

/// A part's face area on the wall.
fn face_area(l: &Layout, p: &curtain::Part) -> f64 {
    let s = &l.shapes[p.shape as usize];
    match &s.face {
        Some(q) => (0..4).map(|i| q[i][0] * q[(i + 1) % 4][1] - q[(i + 1) % 4][0] * q[i][1]).sum::<f64>() / 2.0,
        None => s.size[0] * s.size[1],
    }
}

#[test]
fn a_butt_joint_is_a_box_and_who_runs_through_is_the_layouts_say() {
    let mut s = session();
    framed(&mut s, "Frame", &[4000.0], &[3000.0], "50 x 150", "butt");
    let l = built(&mut s, "Frame", 4000.0, 3000.0);
    let m = mullions(&l);
    assert_eq!(m.len(), 4);
    assert!(m.iter().all(|p| l.shapes[p.shape as usize].face.is_none()));
    // Verticals run through by default: the horizontals stop between them.
    let area: f64 = m.iter().map(|p| face_area(&l, p)).sum();
    assert!((area - (2.0 * 50.0 * (4000.0 + 3000.0) - 4.0 * 2500.0)).abs() < 1e-6, "{area}");
}

#[test]
fn a_mitre_cuts_both_mullions_on_the_corners_diagonal() {
    let mut s = session();
    framed(&mut s, "Frame", &[4000.0], &[3000.0], "50 x 150", "mitre");
    let l = built(&mut s, "Frame", 4000.0, 3000.0);
    let m = mullions(&l);
    assert_eq!(m.len(), 4);
    assert!(m.iter().all(|p| l.shapes[p.shape as usize].face.is_some()), "every end is at a corner");
    // Nothing overlaps and nothing's missing: the frame's area exactly.
    let area: f64 = m.iter().map(|p| face_area(&l, p)).sum();
    assert!((area - (2.0 * 50.0 * (4000.0 + 3000.0) - 4.0 * 2500.0)).abs() < 1e-6, "{area}");
    // Each runs out to the wall's corners.
    for p in &m {
        let s = &l.shapes[p.shape as usize];
        if matches!(p.address, curtain::Address::Vertical { .. }) {
            assert_eq!(s.size[1], 3000.0);
        } else {
            assert_eq!(s.size[0], 4000.0);
        }
    }
    // The bottom left corner's diagonal is from the wall's corner to (50, 50), on both bars.
    let left = m.iter().find(|p| matches!(p.address, curtain::Address::Vertical { .. }) && p.centre[0] < 100.0).unwrap();
    let bottom = m.iter().find(|p| matches!(p.address, curtain::Address::Horizontal { .. }) && p.centre[1] < 100.0).unwrap();
    for (part, way) in [(left, "left"), (bottom, "bottom")] {
        let q = l.shapes[part.shape as usize].face.unwrap();
        let at: Vec<[f64; 2]> = q.iter().map(|c| [c[0] + part.centre[0], c[1] + part.centre[1]]).collect();
        for corner in [[0.0, 0.0], [50.0, 50.0]] {
            assert!(at.iter().any(|c| (c[0] - corner[0]).abs() < 1e-6 && (c[1] - corner[1]).abs() < 1e-6), "{way}: {at:?}");
        }
    }
    // The panel is what's inside the frame, as with a butt joint.
    let panel = l.parts.iter().find(|p| l.shapes[p.shape as usize].kind == PartKind::Panel).unwrap();
    assert_eq!(l.shapes[panel.shape as usize].size[..2], [3900.0, 2900.0]);
}

#[test]
fn a_mitre_falls_back_to_butt_where_sizes_differ_or_only_one_asks() {
    let mut s = session();
    run(&mut s, "type", json!({"category": "mullion", "name": "Deep", "width": 50, "depth": 250}));
    let pick = |t: &str, j: &str| json!({"type": t, "join": j});
    let none = json!([[null]]);
    let cases = [
        // Another depth meets it, then the same mullion asking for butt.
        (json!({"left": pick("50 x 150", "mitre"), "right": null, "bottom": pick("Deep", "mitre"), "top": null}), "Sizes"),
        (json!({"left": pick("50 x 150", "mitre"), "right": null, "bottom": pick("50 x 150", "butt"), "top": null}), "One"),
    ];
    for (borders, name) in cases {
        grid(&mut s, name, &[4000.0], &[3000.0], json!({"vertical": none, "horizontal": none, "borders": borders}));
        let l = built(&mut s, name, 4000.0, 3000.0);
        assert!(mullions(&l).iter().all(|p| l.shapes[p.shape as usize].face.is_none()), "{name}");
    }
}

#[test]
fn a_mitre_is_only_at_a_corner_a_t_or_a_crossing_stays_butt() {
    let mut s = session();
    let edge = json!({"type": "50 x 150", "join": "mitre"});
    let borders = json!({"left": edge, "right": edge, "bottom": edge, "top": edge});
    // Two columns: a vertical in the middle makes a T on the top and bottom borders.
    grid(&mut s, "T", &[2000.0], &[3000.0], json!({"vertical": [[edge]], "horizontal": [[null]], "borders": borders}));
    let l = built(&mut s, "T", 4000.0, 3000.0);
    let middle = mullions(&l)
        .into_iter()
        .find(|p| matches!(p.address, curtain::Address::Vertical { .. }) && (p.centre[0] - 2000.0).abs() < 1.0)
        .cloned()
        .unwrap();
    assert!(l.shapes[middle.shape as usize].face.is_none(), "the T's stem is square");
    // The border runs on through the T, mitred only at the wall's corners, and nothing overlaps.
    let area: f64 = mullions(&l).iter().map(|p| face_area(&l, p)).sum();
    let frame = 2.0 * 50.0 * (4000.0 + 3000.0) - 4.0 * 2500.0;
    assert!((area - (frame + 50.0 * (3000.0 - 100.0))).abs() < 1e-6, "{area}");
}

#[test]
fn an_inner_corner_of_mullions_mitres_too() {
    let mut s = session();
    // Only the first cell's right and top edges: one L where they meet, inside the wall.
    let edge = json!({"type": "50 x 150", "join": "mitre"});
    grid(
        &mut s,
        "L",
        &[1000.0, 3000.0],
        &[1000.0, 2000.0],
        json!({
            "vertical": [[null, edge], [null, null]],
            "horizontal": [[null, null], [edge, null]],
            "borders": {"left": null, "right": null, "bottom": null, "top": null},
        }),
    );
    let l = built(&mut s, "L", 4000.0, 3000.0);
    let m = mullions(&l);
    assert_eq!(m.len(), 2, "{:?}", l.parts);
    assert!(m.iter().all(|p| l.shapes[p.shape as usize].face.is_some()));
    // They reach the corner's outer face: 25 past the lines.
    let v = m.iter().find(|p| matches!(p.address, curtain::Address::Vertical { .. })).unwrap();
    let vs = &l.shapes[v.shape as usize];
    assert!((v.centre[1] + vs.size[1] / 2.0 - 1025.0).abs() < 1e-6, "{v:?} {vs:?}");
    let h = m.iter().find(|p| matches!(p.address, curtain::Address::Horizontal { .. })).unwrap();
    let hs = &l.shapes[h.shape as usize];
    assert!((h.centre[0] + hs.size[0] / 2.0 - 1025.0).abs() < 1e-6, "{h:?} {hs:?}");
}

#[test]
fn mitred_parts_share_shapes_too_and_never_make_a_broken_shape() {
    let mut s = session();
    // Left and right mirror each other, so two shapes; every row of a tall wall's verticals share them.
    framed(&mut s, "Frame", &[4000.0], &[3000.0], "50 x 150", "mitre");
    let l = built(&mut s, "Frame", 4000.0, 3000.0);
    let mitred: Vec<_> = l.shapes.iter().filter(|x| x.face.is_some()).collect();
    assert_eq!(mitred.len(), 4);
    // Barely taller than its bars: each bar's two diagonals would cross, so it's left a box rather than a bow tie.
    run(&mut s, "type", json!({"category": "mullion", "name": "Fat", "width": 400, "depth": 100}));
    framed(&mut s, "Tight", &[1000.0], &[1000.0], "Fat", "mitre");
    let l = built(&mut s, "Tight", 1000.0, 700.0);
    for shape in &l.shapes {
        if let Some(q) = shape.face {
            let area: f64 = (0..4).map(|i| q[i][0] * q[(i + 1) % 4][1] - q[(i + 1) % 4][0] * q[i][1]).sum();
            assert!(area > 0.0, "{q:?}");
            assert!(q.iter().all(|c| c[0].abs() <= shape.size[0] / 2.0 + 1e-6 && c[1].abs() <= shape.size[1] / 2.0 + 1e-6));
        }
    }
}

#[test]
fn a_mitred_mullion_is_a_closed_solid_in_the_3d_scene() {
    use crate::view3d::{self, Solid};
    let mut s = session();
    framed(&mut s, "Frame", &[4000.0], &[3000.0], "50 x 150", "mitre");
    let id = wall(&mut s, "Frame", 4000.0, 3000.0);
    let l = layout_of(&s, id);
    let meshes: Vec<_> = view3d::solids(&s.model)
        .into_iter()
        .filter_map(|x| match x {
            Solid::Mesh { tris, .. } => Some(tris),
            _ => None,
        })
        .collect();
    assert_eq!(meshes.len(), 4);
    // Outward and closed: the divergence theorem gives its bar's volume, the bars' faces times their depth.
    let volume: f64 = meshes
        .iter()
        .flatten()
        .map(|[a, b, c]| {
            let cross = [b[1] * c[2] - b[2] * c[1], b[2] * c[0] - b[0] * c[2], b[0] * c[1] - b[1] * c[0]];
            (a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]) / 6.0
        })
        .sum();
    let faces: f64 = mullions(&l).iter().map(|p| face_area(&l, p)).sum();
    assert!((volume - faces * 150.0).abs() < 1.0, "{volume} {}", faces * 150.0);
}

#[test]
fn a_mitred_mullion_goes_to_ifc_as_a_closed_face_set_each_shape_once() {
    let mut s = session();
    framed(&mut s, "Frame", &[4000.0], &[3000.0], "50 x 150", "mitre");
    wall(&mut s, "Frame", 4000.0, 3000.0);
    let text = crate::ifc::export(&s.model);
    assert_eq!(text.matches("IFCMEMBER(").count(), 4);
    assert_eq!(text.matches("IFCTRIANGULATEDFACESET(").count(), 4);
}

// Walls that meet one.

#[test]
fn a_plain_wall_ends_square_against_a_curtain_wall() {
    use crate::geom;
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[1000.0], json!({}));
    let plain =
        run(&mut s, "wall", json!({"points": [[-4000, 0], [0, 0]], "height": 3000, "location": "centre"}))["ids"][0].as_u64().unwrap();
    let c = run(&mut s, "wall", json!({"points": [[0, 0], [0, 4000]], "type": "CW", "height": 3000}))["ids"][0].as_u64().unwrap();
    let o = crate::walls::outline(&s.model, plain);
    let w = wall_of(&s, plain);
    let t = s.model.thickness(w);
    // A rectangle, its end reaching the curtain wall's far face: no spike, no gap.
    let far = crate::walls::outline(&s.model, c).iter().map(|p| p.0).fold(f64::MIN, f64::max);
    let (xs, ys): (Vec<f64>, Vec<f64>) = o.iter().map(|p| (p.0, p.1)).unzip();
    let max_x = xs.iter().copied().fold(f64::MIN, f64::max);
    assert!((max_x - far).abs() < 1e-6, "{o:?} to {far}");
    let (lo, hi) = (ys.iter().copied().fold(f64::MAX, f64::min), ys.iter().copied().fold(f64::MIN, f64::max));
    assert!((geom::area(&o).abs() - (max_x + 4000.0) * (hi - lo)).abs() < 1e-3, "{o:?}");
    assert!((hi - lo - t).abs() < 1e-6);
    // Rooms still close on it: its centre line runs on to the curtain wall's.
    let line = crate::walls::centre_line(&s.model, plain);
    assert!(line.last().unwrap().0 != -4000.0);
}

#[test]
fn a_plain_wall_ending_on_a_curtain_walls_side_stops_at_its_face() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[1000.0], json!({}));
    let c = run(&mut s, "wall", json!({"points": [[0, -2000], [0, 2000]], "type": "CW", "height": 3000}))["ids"][0].as_u64().unwrap();
    let outline = crate::walls::outline(&s.model, c);
    let (lo, hi) = outline.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let mid = (lo + hi) / 2.0;
    let plain =
        run(&mut s, "wall", json!({"points": [[-4000, 0], [mid, 0]], "height": 3000, "location": "centre"}))["ids"][0].as_u64().unwrap();
    let reach = crate::walls::outline(&s.model, plain).iter().map(|p| p.0).fold(f64::MIN, f64::max);
    assert!((reach - lo).abs() < 1e-6, "stops at the near face: {reach}, not {lo}");
}

#[test]
fn walls_by_face_cant_be_curtain_walls_yet() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[1000.0], json!({}));
    let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
    let mass = run(&mut s, "mass", json!({"boundary": boundary, "height": 3000}))["id"].as_u64().unwrap();
    let why = fails(&mut s, "wall_by_face", json!({"mass": mass, "type": "CW", "faces": [0]}));
    assert!(why.contains("by face"), "{why}");
    // Nor can a wall made by face become one afterwards.
    let plain = run(&mut s, "wall_by_face", json!({"mass": mass, "faces": [0]}))["ids"][0].as_u64().unwrap();
    let why = fails(&mut s, "modify", json!({"id": plain, "type": "CW"}));
    assert!(why.contains("by face"), "{why}");
}
