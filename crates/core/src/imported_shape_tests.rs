//! An element's imported shape (`Element::imported`): another program's own geometry of a wall, a floor, a roof, a ceiling, a
//! column, a beam, a pipe, a duct or a cable tray, drawn in 3D while it is the element it was drawn for, and its own geometry again once it is edited.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, ImportedPart, ImportedShape};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// The element a command made: its `id`, or the first of its `ids`.
fn made(out: &Value) -> u64 {
    out["id"].as_u64().or_else(|| out["ids"][0].as_u64()).unwrap_or_else(|| panic!("{out}"))
}

/// A box from `lo` to `hi` as twelve triangles.
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Vec<[[f64; 3]; 3]> {
    let c =
        |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    quads.iter().flat_map(|q| [[c(q[0]), c(q[1]), c(q[2])], [c(q[0]), c(q[2]), c(q[3])]]).collect()
}

/// Each kind that may carry an imported shape, as a command that makes one.
fn kinds() -> Vec<(&'static str, Value)> {
    vec![
        ("wall", json!({"points": [[0, 0], [5000, 0]], "height": 3000})),
        ("floor", json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]]})),
        ("roof", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]})),
        ("ceiling", json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]]})),
        ("column", json!({"at": [0, 0]})),
        ("structural_column", json!({"at": [500, 500]})),
        ("beam", json!({"points": [[0, 0], [5000, 2000]], "offset": 3000})),
        ("pipe", json!({"points": [[0, 0], [4000, 0]], "offset": 2500})),
        ("duct", json!({"points": [[0, 0], [0, 4000]], "offset": 2800})),
        ("cable_tray", json!({"points": [[0, 0], [3000, 3000]], "offset": 3000})),
    ]
}

/// Gives element `id` the shape of a block from `lo` to `hi` in material `material`, drawn for the element as it stands.
fn give(s: &mut Session, id: u64, (lo, hi): ([f64; 3], [f64; 3]), material: &str) {
    let m = s.model_mut();
    let key = crate::imported::key(m, id).unwrap_or_else(|| panic!("element {id} has no key"));
    let shape = ImportedShape { key, parts: vec![ImportedPart { material: material.to_string(), tris: Arc::new(cuboid(lo, hi)) }] };
    let slot = m.elements.get_mut(id).and_then(Element::imported_mut).unwrap_or_else(|| panic!("element {id} takes no shape"));
    *slot = Some(shape);
}

fn box_of(s: &Session, id: u64) -> ([f64; 3], [f64; 3]) {
    let ids: HashSet<u64> = [id].into();
    let boxes = crate::view3d::Scene::new(&s.model).boxes(&ids);
    boxes.values().copied().next().unwrap_or_else(|| panic!("{boxes:?}"))
}

fn near(a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3])) -> bool {
    a.0.iter().chain(&a.1).zip(b.0.iter().chain(&b.1)).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn each_kind_with_an_imported_shape_is_drawn_in_it_and_in_its_own_geometry_once_it_is_edited() {
    for (command, params) in kinds() {
        let mut s = Session::new();
        let id = made(&run(&mut s, command, params));
        let own = box_of(&s, id);
        // Somewhere its own geometry is not: 20 m east of the origin.
        let shape = ([20_000.0, 0.0, 0.0], [21_000.0, 500.0, 2500.0]);
        give(&mut s, id, shape, "");
        assert!(s.model.check().is_ok(), "{command}: {:?}", s.model.check());
        assert!(crate::imported::live(&s.model, id).is_some(), "{command}");
        assert!(near(box_of(&s, id), shape), "{command}: {:?}", box_of(&s, id));
        // Moved, it is another element: its own geometry again, where it was moved to.
        run(&mut s, "move", json!({"ids": [id], "by": [0, 1000]}));
        let moved = ([own.0[0], own.0[1] + 1000.0, own.0[2]], [own.1[0], own.1[1] + 1000.0, own.1[2]]);
        assert!(near(box_of(&s, id), moved), "{command}: {:?} against {moved:?}", box_of(&s, id));
        assert!(crate::imported::live(&s.model, id).is_none(), "{command}");
    }
}

#[test]
fn a_door_set_in_a_wall_with_an_imported_shape_changes_the_wall_it_is_and_a_tag_on_it_does_not() {
    let mut s = Session::new();
    let id = made(&run(&mut s, "wall", json!({"points": [[0, 0], [5000, 0]], "height": 3000})));
    give(&mut s, id, ([0.0, -100.0, 0.0], [5000.0, 100.0, 3000.0]), "");
    let key = crate::imported::key(&s.model, id);
    run(&mut s, "tag", json!({"ids": [id]}));
    assert_eq!(crate::imported::key(&s.model, id), key, "a tag says something of the wall and changes nothing of it");
    run(&mut s, "door", json!({"at": [2000, 0]}));
    assert_ne!(crate::imported::key(&s.model, id), key, "a door cuts it");
    assert!(crate::imported::live(&s.model, id).is_none());
}

#[test]
fn a_parameter_set_on_an_element_with_an_imported_shape_keeps_it() {
    let mut s = Session::new();
    let id = made(&run(&mut s, "floor", json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]]})));
    give(&mut s, id, ([0.0, 0.0, -300.0], [6000.0, 4000.0, 0.0]), "");
    run(&mut s, "parameter", json!({"name": "Fire rating", "kind": "text", "categories": ["floor"]}));
    run(&mut s, "modify", json!({"ids": [id], "params": {"Fire rating": "REI 60"}}));
    assert!(crate::imported::live(&s.model, id).is_some(), "a value of its own is no edit of its shape");
}

#[test]
fn a_shape_whose_corner_is_no_number_or_whose_material_is_unknown_is_refused() {
    for (command, params) in kinds() {
        let mut s = Session::new();
        let id = made(&run(&mut s, command, params));
        give(&mut s, id, ([0.0, 0.0, 0.0], [100.0, 100.0, 100.0]), "Nothing");
        let why = s.model.check().unwrap_err();
        assert!(why.contains("Nothing"), "{command}: {why}");
        give(&mut s, id, ([0.0, 0.0, 0.0], [100.0, 100.0, 100.0]), "");
        if let Some(Some(shape)) = s.model_mut().elements.get_mut(id).and_then(Element::imported_mut) {
            Arc::make_mut(&mut shape.parts[0].tris)[0][0][0] = f64::NAN;
        }
        let why = s.model.check().unwrap_err();
        assert!(why.contains("not a number"), "{command}: {why}");
    }
}

#[test]
fn a_shapes_material_is_named_anew_when_the_material_is_renamed_and_is_in_use() {
    for (command, params) in kinds() {
        let mut s = Session::new();
        let id = made(&run(&mut s, command, params));
        run(&mut s, "material", json!({"name": "Render", "color": [200, 50, 50]}));
        give(&mut s, id, ([0.0, 0.0, 0.0], [100.0, 100.0, 100.0]), "Render");
        assert!(s.model.material_user("Render").is_some(), "{command}");
        run(&mut s, "material", json!({"name": "Render", "rename": "Stucco"}));
        let shape = s.model.elements.get(id).and_then(Element::imported).unwrap_or_else(|| panic!("{command}: no shape"));
        assert_eq!(shape.parts[0].material, "Stucco", "{command}");
    }
}

#[test]
fn a_run_other_than_a_beam_a_pipe_a_duct_or_a_cable_tray_carrying_an_imported_shape_is_refused() {
    let mut s = Session::new();
    let id = made(&run(&mut s, "conduit", json!({"points": [[0, 0], [5000, 0]], "offset": 2500})));
    let shape = ImportedShape { key: 0, parts: vec![ImportedPart { material: String::new(), tris: Arc::new(cuboid([0.0; 3], [1.0; 3])) }] };
    if let Some(Element::Conduit(r)) = s.model_mut().elements.get_mut(id) {
        r.imported = Some(shape);
    }
    let why = s.model.check().unwrap_err();
    assert!(why.contains("only a beam, a pipe, a duct or a cable tray"), "{why}");
}
