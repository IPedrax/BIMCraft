//! Dimensions to components (issue 537), as Revit dimensions a family instance: its family's reference planes where it
//! stands, its centre lines along it and across it, and any other plane of its family that is a reference.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id, Model};

/// A plane of the family's own 300 mm east of Center (Left/Right), a strong reference.
const RIGHT: &str = "5a1e0f3c-2b4d-4e6f-8a9b-0c1d2e3f4a5b";
/// One 200 mm west of it that is not a reference.
const LOOSE: &str = "6b2f1a4d-3c5e-4f70-9bac-1d2e3f4a5b6c";

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn err(s: &mut Session, c: &str, p: Value) -> String {
    s.execute(c, &p).err().unwrap_or_else(|| panic!("{c} {p} was accepted"))
}

fn id(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_default()
}

/// The family, and two of it on the lowest level: at (1000, 1000) and (3000, 1500).
fn two_boxes(s: &mut Session) -> (Id, Id) {
    run(
        s,
        "family",
        json!({"name": "Box", "types": {"A": {}},
               "forms": [{"profile": [[-100, -100], [100, -100], [100, 100], [-100, 100]], "to": 100}],
               "ref_planes": [
                   {"guid": RIGHT, "name": "Right", "start": [300, -500, 0], "end": [300, 500, 0], "up": [0, 0, 1], "reference": "strong"},
                   {"guid": LOOSE, "start": [-200, -500, 0], "end": [-200, 500, 0], "up": [0, 0, 1], "reference": "not_a_reference"}]}),
    );
    let a = id(&run(s, "component", json!({"family": "Box", "type": "A", "at": [1000, 1000]})));
    let b = id(&run(s, "component", json!({"family": "Box", "type": "A", "at": [3000, 1500]})));
    (a, b)
}

fn values_in(m: &Model, d: Id) -> Vec<f64> {
    match m.elements.get(d) {
        Some(Element::Dimension(dim)) => m.dimension_values(dim).into_iter().map(|v| (v * 1e6).round() / 1e6).collect(),
        other => panic!("{d} is {other:?}"),
    }
}

fn values(s: &Session, d: Id) -> Vec<f64> {
    values_in(&s.model, d)
}

#[test]
fn a_dimension_measures_between_two_components_centre_lines_and_follows_them() {
    let mut s = Session::new();
    let (a, b) = two_boxes(&mut s);
    // Across: Center (Left/Right), running south to north at angle 0.
    let across =
        id(&run(&mut s, "dimension", json!({"refs": [{"id": a, "face": "across"}, {"id": b, "face": "across"}], "at": [2000, 3000]})));
    assert_eq!(values(&s, across), [2000.0]);
    // The default, as a column's: its centre line along it, Center (Front/Back), west to east.
    let along = id(&run(&mut s, "dimension", json!({"refs": [{"id": a}, {"id": b}], "at": [5000, 1200]})));
    assert_eq!(values(&s, along), [500.0]);
    // Moved, both follow it.
    run(&mut s, "move", json!({"ids": [b], "by": [500, 250]}));
    assert_eq!((values(&s, across), values(&s, along)), (vec![2500.0], vec![750.0]));
}

#[test]
fn a_dimension_measures_to_a_reference_plane_of_the_family_and_turns_with_the_component() {
    let mut s = Session::new();
    let (a, _) = two_boxes(&mut s);
    let w = run(&mut s, "wall", json!({"points": [[0, -1000], [0, 4000]]}))["ids"][0].as_u64().unwrap_or_default();
    // Right stands 300 mm east of the component's insertion point at x 1000.
    let d = id(&run(&mut s, "dimension", json!({"refs": [{"id": w}, {"id": a, "face": {"plane": RIGHT}}], "at": [600, -500]})));
    assert_eq!(values(&s, d), [1300.0]);
    // Half a turn about its own point takes the plane to its west.
    run(&mut s, "rotate", json!({"ids": [a], "about": [1000, 1000], "angle": 180}));
    assert_eq!(values(&s, d), [700.0]);
    // A quarter turn would take it out of parallel with the wall: refused, as for any reference.
    err(&mut s, "rotate", json!({"ids": [a], "about": [1000, 1000], "angle": 90}));
    assert_eq!(values(&s, d), [700.0]);
    // Saved and opened again, it measures the same.
    let (again, _) = Model::read(&s.model.to_json().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(values_in(&again, d), [700.0]);
}

#[test]
fn a_plane_that_is_no_reference_and_a_plane_of_another_element_are_refused() {
    let mut s = Session::new();
    let (a, b) = two_boxes(&mut s);
    let e = err(&mut s, "dimension", json!({"refs": [{"id": a, "face": {"plane": LOOSE}}, {"id": b, "face": "across"}], "at": [0, 0]}));
    assert!(e.contains("no such reference plane"), "{e}");
    let w = run(&mut s, "wall", json!({"points": [[0, -1000], [0, 4000]]}))["ids"][0].as_u64().unwrap_or_default();
    let e = err(&mut s, "dimension", json!({"refs": [{"id": w, "face": {"plane": RIGHT}}, {"id": b, "face": "across"}], "at": [0, 0]}));
    assert!(e.contains("only a component does"), "{e}");
}

#[test]
fn a_point_finds_a_components_plane_and_a_deleted_component_leaves_the_dimension() {
    let mut s = Session::new();
    let (a, b) = two_boxes(&mut s);
    let w = run(&mut s, "wall", json!({"points": [[0, -1000], [0, 4000]]}))["ids"][0].as_u64().unwrap_or_default();
    // A point on the wall, then one on the box's Right plane: of the lines there parallel to the wall, that plane.
    let d = id(&run(&mut s, "dimension", json!({"refs": [[0, 0], [1300, 1200]], "at": [600, -500]})));
    let refs = match s.model.elements.get(d) {
        Some(Element::Dimension(dim)) => dim.refs.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(refs[1].id, s.model.guid(a));
    assert_eq!(refs[1].face, crate::model::Face::Plane(serde_json::from_value(json!(RIGHT)).unwrap_or_default()));
    assert_eq!(values(&s, d), [1300.0]);
    // A chain to both boxes: the one deleted leaves it, the other stays measured.
    let chain = id(&run(
        &mut s,
        "dimension",
        json!({"refs": [{"id": w}, {"id": a, "face": "across"}, {"id": b, "face": "across"}], "at": [0, 3000]}),
    ));
    assert_eq!(values(&s, chain), [1000.0, 2000.0]);
    run(&mut s, "delete", json!({"ids": [a]}));
    assert!(s.model.elements.get(d).is_none(), "a dimension left with one reference goes");
    assert_eq!(values(&s, chain), [3000.0]);
}
