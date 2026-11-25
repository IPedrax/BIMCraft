//! Dimensions to components (issue 537), as Revit dimensions a family instance: its family's reference planes where it
//! stands, its centre lines along it and across it, and any other plane of its family that is a reference.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id, Model};

/// A plane of the family's own 300 mm east of Center (Left/Right), a strong reference.
const RIGHT: &str = "5a1e0f3c-2b4d-4e6f-8a9b-0c1d2e3f4a5b";
/// One 200 mm west of it that is not a reference.
const LOOSE: &str = "6b2f1a4d-3c5e-4f70-9bac-1d2e3f4a5b6c";
/// A weak reference where the strong one stands.
const WEAK: &str = "7c3a2b5e-4d6f-4a81-9cbd-2e3f4a5b6c7d";

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

#[test]
fn where_a_weak_plane_lies_over_a_strong_one_a_point_takes_the_strong_one() {
    let mut s = Session::new();
    // The weak one comes first in the family.
    run(
        &mut s,
        "family",
        json!({"name": "Box", "types": {"A": {}},
               "forms": [{"profile": [[-100, -100], [100, -100], [100, 100], [-100, 100]], "to": 100}],
               "ref_planes": [
                   {"guid": WEAK, "start": [300, -500, 0], "end": [300, 500, 0], "up": [0, 0, 1], "reference": "weak"},
                   {"guid": RIGHT, "name": "Right", "start": [300, -500, 0], "end": [300, 500, 0], "up": [0, 0, 1], "reference": "strong"}]}),
    );
    let a = id(&run(&mut s, "component", json!({"family": "Box", "type": "A", "at": [1000, 1000]})));
    let w = run(&mut s, "wall", json!({"points": [[0, -1000], [0, 4000]]}))["ids"][0].as_u64().unwrap_or_default();
    let d = id(&run(&mut s, "dimension", json!({"refs": [[0, 0], [1300, 1200]], "at": [600, -500]})));
    let plane = |g: &str| crate::model::Face::Plane(serde_json::from_value(json!(g)).unwrap_or_default());
    match s.model.elements.get(d) {
        Some(Element::Dimension(dim)) => assert_eq!((dim.refs[1].id, dim.refs[1].face), (s.model.guid(a), plane(RIGHT))),
        other => panic!("{other:?}"),
    }
    // The weak one is still there to name.
    let e = id(&run(&mut s, "dimension", json!({"refs": [{"id": w}, {"id": a, "face": {"plane": WEAK}}], "at": [600, -800]})));
    assert_eq!(values(&s, e), [1300.0]);
}

/// Left and right planes of their own, strong, 300 mm either side of Center (Left/Right).
const SIDE_L: &str = "8d4b3c6f-5e70-4b92-8dce-3f4a5b6c7d8e";
const SIDE_R: &str = "9e5c4d70-6f81-4ca3-9edf-4a5b6c7d8e9f";

/// A cabinet with Left and Right; `parametric`, an instance Width labels them, kept equal about Center (Left/Right).
fn cabinet(parametric: bool) -> Value {
    let plane = |g: &str, name: &str, x: f64| json!({"guid": g, "name": name, "start": [x, -1000.0, 0.0], "end": [x, 1000.0, 0.0], "up": [0.0, 0.0, 1.0], "reference": "strong"});
    let mut f = json!({"name": "Cabinet", "types": {"A": {}},
        "forms": [{"profile": [[-100, -100], [100, -100], [100, 100], [-100, 100]], "to": 100}],
        "ref_planes": [plane(SIDE_L, "Left", -300.0), plane(SIDE_R, "Right", 300.0)]});
    if parametric {
        f["parameters"] = json!([{"name": "Width", "kind": "length", "instance": true, "value": 600}]);
        f["dimensions"] = json!([
            {"view": "plan", "refs": [{"plane": SIDE_L}, {"plane": SIDE_R}], "at": [0, -800, 0], "label": "Width"},
            {"view": "plan", "refs": [{"plane": SIDE_L}, {"plane": crate::family::CENTER_LEFT_RIGHT}, {"plane": SIDE_R}],
             "at": [0, -900, 0], "equal": true}
        ]);
    }
    f
}

/// Two cabinets at (1000, 1000) and (3000, 1000), a wall at x 0, and two dimensions: the wall to the first one's Right
/// (1300), and its Right to the second one's Left (1400).
fn two_cabinets(parametric: bool) -> (Session, Id, Id, Id, Id) {
    let mut s = Session::new();
    run(&mut s, "family", cabinet(parametric));
    let a = id(&run(&mut s, "component", json!({"family": "Cabinet", "type": "A", "at": [1000, 1000]})));
    let b = id(&run(&mut s, "component", json!({"family": "Cabinet", "type": "A", "at": [3000, 1000]})));
    let w = run(&mut s, "wall", json!({"points": [[0, -1000], [0, 4000]]}))["ids"][0].as_u64().unwrap_or_default();
    let to_wall = id(&run(&mut s, "dimension", json!({"refs": [{"id": w}, {"id": a, "face": {"plane": SIDE_R}}], "at": [600, -500]})));
    let between = id(&run(
        &mut s,
        "dimension",
        json!({"refs": [{"id": a, "face": {"plane": SIDE_R}}, {"id": b, "face": {"plane": SIDE_L}}], "at": [2000, -700]}),
    ));
    (s, a, b, to_wall, between)
}

/// The family as a file holds it, to load again.
fn cabinet_file(s: &Session) -> Value {
    let mut v = serde_json::to_value(&s.model.families["Cabinet"]).unwrap_or_default();
    v["name"] = json!("Cabinet");
    v
}

/// The file's own planes (not the datums), each given by `f`.
fn own_planes(file: &mut Value, f: impl Fn(&mut Vec<Value>)) {
    let Some(planes) = file["planes"].as_array_mut() else { panic!("the family file has no planes") };
    let is_own = |p: &Value| [SIDE_L, SIDE_R].contains(&p["guid"].as_str().unwrap_or_default());
    let mut own: Vec<Value> = planes.iter().filter(|p| is_own(p)).cloned().collect();
    planes.retain(|p| !is_own(p));
    f(&mut own);
    planes.extend(own);
}

fn refs_of(s: &Session, d: Id) -> Vec<crate::model::Reference> {
    match s.model.elements.get(d) {
        Some(Element::Dimension(dim)) => dim.refs.clone(),
        other => panic!("{d} is {other:?}"),
    }
}

#[test]
fn a_family_loaded_again_keeps_the_dimensions_to_its_planes_by_their_guids_each_placement_flexing() {
    let (mut s, a, b, to_wall, between) = two_cabinets(true);
    // The second one widened to 900: its Left 450 west of its point.
    run(&mut s, "modify", json!({"id": b, "params": {"Width": 900}}));
    assert_eq!((values(&s, to_wall), values(&s, between)), (vec![1300.0], vec![1250.0]));
    let refs = (refs_of(&s, to_wall), refs_of(&s, between));
    // Published again, its form changed each time and its planes given back with their GUIDs, either way of loading it: a
    // step to undo each.
    for (overwrite, height) in [("version", 250), ("version_and_values", 400)] {
        let mut file = cabinet_file(&s);
        file["forms"][0]["to"] = json!(height);
        let r = run(&mut s, "family", json!({"definition": file, "overwrite": overwrite}));
        assert_eq!(r["deleted_dimensions"].as_u64().unwrap_or_default(), 0, "{overwrite}");
        assert_eq!((refs_of(&s, to_wall), refs_of(&s, between)), refs, "{overwrite}");
        assert_eq!((values(&s, to_wall), values(&s, between)), (vec![1300.0], vec![1250.0]), "{overwrite}");
    }
    // Only the one resized moves its planes.
    run(&mut s, "modify", json!({"id": a, "params": {"Width": 1000}}));
    assert_eq!((values(&s, to_wall), values(&s, between)), (vec![1500.0], vec![1050.0]));
    // Undone past both loads, and done again.
    for _ in 0..3 {
        run(&mut s, "undo", json!({}));
    }
    assert_eq!((values(&s, to_wall), values(&s, between)), (vec![1300.0], vec![1250.0]));
    for _ in 0..3 {
        run(&mut s, "redo", json!({}));
    }
    assert_eq!((values(&s, to_wall), values(&s, between)), (vec![1500.0], vec![1050.0]));
    assert_eq!((refs_of(&s, to_wall), refs_of(&s, between)), refs);
    // Saved and opened again.
    let (again, _) = Model::read(&s.model.to_json().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((values_in(&again, to_wall), values_in(&again, between)), (vec![1500.0], vec![1050.0]));
}

#[test]
fn planes_loaded_again_without_guids_take_the_ones_at_their_place_so_reordered_they_move_a_dimension() {
    let (s, _, _, to_wall, between) = two_cabinets(false);
    let load = |f: &dyn Fn(&mut Vec<Value>)| {
        let mut s = s.clone();
        let mut file = cabinet_file(&s);
        own_planes(&mut file, f);
        run(&mut s, "family", json!({"definition": file, "overwrite": "version"}));
        (values(&s, to_wall), values(&s, between))
    };
    let unnamed = |own: &mut Vec<Value>| {
        for p in own.iter_mut().filter_map(Value::as_object_mut) {
            p.remove("guid");
            p.remove("id");
        }
    };
    // With their GUIDs, their order is nothing to the dimensions.
    assert_eq!(load(&|own| own.reverse()), (vec![1300.0], vec![1400.0]));
    // Without, in their order: each takes back its own.
    assert_eq!(load(&unnamed), (vec![1300.0], vec![1400.0]));
    // Without, reordered: Right takes Left's GUID and Left Right's, and the dimensions follow the GUIDs, unwarned.
    assert_eq!(
        load(&|own| {
            unnamed(own);
            own.reverse();
        }),
        (vec![700.0], vec![2600.0])
    );
}

/// What a load gave: the two dimensions' values, or why it was refused; and whether the project is as it was.
type Loaded = (Result<(Vec<f64>, Vec<f64>), String>, bool);

#[test]
fn a_load_that_takes_away_a_measured_plane_or_its_reference_is_refused_whole_and_one_moving_it_is_followed() {
    let (s, _, _, to_wall, between) = two_cabinets(false);
    let load = |f: &dyn Fn(&mut Vec<Value>)| -> Loaded {
        let mut s = s.clone();
        let before = s.model.to_json().unwrap_or_default();
        let mut file = cabinet_file(&s);
        own_planes(&mut file, f);
        let r = s.execute("family", &json!({"definition": file, "overwrite": "version"}));
        let unchanged = s.model.to_json().unwrap_or_default() == before;
        (r.map(|_| (values(&s, to_wall), values(&s, between))), unchanged)
    };
    let right = |own: &mut Vec<Value>, f: &dyn Fn(&mut Value)| own.iter_mut().filter(|p| p["guid"] == json!(SIDE_R)).for_each(f);
    let refused = |(r, unchanged): Loaded, what: &str| {
        let e = r.err().unwrap_or_else(|| panic!("{what} was loaded"));
        assert!(e.contains("no such reference plane"), "{what}: {e}");
        assert!(unchanged, "{what} changed the project");
    };
    refused(load(&|own| own.retain(|p| p["guid"] != json!(SIDE_R))), "Right taken away");
    refused(load(&|own| right(own, &|p| p["reference"] = json!("not_a_reference"))), "Right made Not a Reference");
    refused(load(&|own| right(own, &|p| p["up"] = json!([1.0, 0.0, 0.0]))), "Right laid flat");
    // Moved 100 east, it is measured where it now stands.
    let (moved, _) = load(&|own| {
        right(own, &|p| {
            p["start"][0] = json!(400.0);
            p["end"][0] = json!(400.0);
        })
    });
    assert_eq!(moved.unwrap_or_else(|e| panic!("{e}")), (vec![1400.0], vec![1300.0]));
}
