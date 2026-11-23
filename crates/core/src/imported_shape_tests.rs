//! A wall's imported shape (`Wall::shape`): another program's own geometry of it, drawn in 3D while it is the wall it was drawn
//! for, its layers again once it is edited.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, ShapePart, WallShape};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A box from `lo` to `hi` as twelve triangles.
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Vec<[[f64; 3]; 3]> {
    let c =
        |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    quads.iter().flat_map(|q| [[c(q[0]), c(q[1]), c(q[2])], [c(q[0]), c(q[2]), c(q[3])]]).collect()
}

/// A session with one wall 5 m long along x, 3 m tall, and its id.
fn one_wall() -> (Session, u64) {
    let mut s = Session::new();
    let out = run(&mut s, "wall", json!({"points": [[0, 0], [5000, 0]], "height": 3000}));
    let id = out["ids"][0].as_u64().unwrap_or_else(|| panic!("{out}"));
    (s, id)
}

/// Gives wall `id` a shape: a block leaning out 500 mm at its top, in material `material`, drawn for the wall as it stands.
fn give_shape(s: &mut Session, id: u64, material: &str) {
    let m = s.model_mut();
    let key = crate::walls::shape_key(m, id).unwrap_or_default();
    let mut tris = cuboid([0.0, -100.0, 0.0], [5000.0, 100.0, 3000.0]);
    for t in &mut tris {
        for p in t.iter_mut() {
            p[1] += 500.0 * p[2] / 3000.0;
        }
    }
    let shape = WallShape { key, parts: vec![ShapePart { material: material.to_string(), tris: Arc::new(tris) }] };
    if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
        w.shape = Some(shape);
    }
}

fn box_of(s: &Session, id: u64) -> ([f64; 3], [f64; 3]) {
    let ids: HashSet<u64> = [id].into();
    let boxes = crate::view3d::Scene::new(&s.model).boxes(&ids);
    boxes.values().copied().next().unwrap_or_else(|| panic!("{boxes:?}"))
}

#[test]
fn a_wall_with_an_imported_shape_is_drawn_in_it_and_in_its_layers_once_it_is_edited() {
    let (mut s, id) = one_wall();
    let layered = box_of(&s, id);
    give_shape(&mut s, id, "");
    assert!(s.model.check().is_ok(), "{:?}", s.model.check());
    // Its shape leans: it reaches 600 mm out at its top, where its layers stand no more than its thickness out.
    let (lo, hi) = box_of(&s, id);
    assert!((hi[1] - 600.0).abs() < 1e-6 && (lo[1] + 100.0).abs() < 1e-6 && (hi[2] - 3000.0).abs() < 1e-6, "{lo:?} {hi:?}");
    // Moved, it is another wall: its layers again.
    run(&mut s, "move", json!({"ids": [id], "by": [0, 1000]}));
    let (lo, hi) = box_of(&s, id);
    assert!((lo[1] - (layered.0[1] + 1000.0)).abs() < 1e-6 && (hi[1] - (layered.1[1] + 1000.0)).abs() < 1e-6, "{lo:?} {hi:?}");
}

#[test]
fn a_door_set_in_a_wall_with_an_imported_shape_changes_the_wall_it_is() {
    let (mut s, id) = one_wall();
    give_shape(&mut s, id, "");
    let key = crate::walls::shape_key(&s.model, id);
    run(&mut s, "door", json!({"at": [2000, 0]}));
    assert_ne!(crate::walls::shape_key(&s.model, id), key, "a door changes what the wall is");
    assert!(crate::walls::live_shape(&s.model, id, None).is_none());
}

#[test]
fn a_shape_whose_corner_is_no_number_or_whose_material_is_unknown_is_refused() {
    let (mut s, id) = one_wall();
    give_shape(&mut s, id, "Nothing");
    let why = s.model.check().unwrap_err();
    assert!(why.contains("Nothing"), "{why}");
    give_shape(&mut s, id, "");
    if let Some(Element::Wall(w)) = s.model_mut().elements.get_mut(id)
        && let Some(shape) = &mut w.shape
    {
        Arc::make_mut(&mut shape.parts[0].tris)[0][0][0] = f64::NAN;
    }
    let why = s.model.check().unwrap_err();
    assert!(why.contains("not a number"), "{why}");
}

#[test]
fn a_shapes_material_is_named_anew_when_the_material_is_renamed() {
    let (mut s, id) = one_wall();
    run(&mut s, "material", json!({"name": "Render", "color": [200, 50, 50]}));
    give_shape(&mut s, id, "Render");
    run(&mut s, "material", json!({"name": "Render", "rename": "Stucco"}));
    let Some(Element::Wall(w)) = s.model.elements.get(id) else { panic!("no wall") };
    assert_eq!(w.shape.as_ref().map(|x| x.parts[0].material.as_str()), Some("Stucco"));
}
