//! Nested families (T6a): a family loaded into a family, its instances placed in it and flexed through the host's
//! parameters, built into the host's pieces and IFC, saved inside it, reloaded over itself as Family Already Exists asks, and
//! its shared families reconciled with a project's as Shared Family Already Loaded asks.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::Family;
use crate::model::Model;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(r) => panic!("{id} {p} should have been refused, and did {r}"),
        Err(e) => e,
    }
}

/// A box `W` by `D` by `H`, from its origin, as a family file holds it.
fn box_definition(w: f64) -> Value {
    json!({"category": "generic", "parameters": [{"name": "W", "value": w}, {"name": "D", "value": 100}, {"name": "H", "value": 100}],
           "types": {"B": {}}, "forms": [{"profile": [[0, 0], ["W", 0], ["W", "D"], [0, "D"]], "to": "H"}]})
}

/// A cabinet of two types, Small (600) and Large (1200), nesting a box placed at (100, 50) whose width is the cabinet's and
/// whose height is half of it.
fn cabinet() -> Session {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Cabinet", "category": "furniture", "parameters": [{"name": "Width", "value": 600}],
               "types": {"Small": {"Width": 600}, "Large": {"Width": 1200}},
               "forms": [{"profile": [[0, -400], [10, -400], [10, -300], [0, -300]], "to": 10}]}),
    );
    run(&mut s, "family", json!({"name": "Cabinet", "nest": {"definition": box_definition(50.0), "name": "Box"}}));
    run(
        &mut s,
        "family",
        json!({"name": "Cabinet", "nested_instance": {"family": "Box", "type": "B", "at": [100, 50],
               "params": {"W": "Width", "H": "Width / 2"}}}),
    );
    s
}

/// The box round family `name`'s pieces for type `t`.
fn extent(m: &Model, name: &str, t: &str) -> ([f64; 3], [f64; 3]) {
    let f = &m.families[name];
    let v = f.values(t, &BTreeMap::new()).unwrap();
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in f.pieces(&v).unwrap().iter().flat_map(|p| p.tris.iter().flatten()) {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

/// A session holding `s`'s project, to change apart from it.
fn copy(s: &Session) -> Session {
    let mut c = Session::new();
    c.model = std::sync::Arc::clone(&s.model);
    c
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn a_nested_box_flexes_with_the_hosts_types_through_its_associated_parameters() {
    let s = cabinet();
    let f = &s.model.families["Cabinet"];
    assert_eq!(f.nested.len(), 1);
    assert_eq!(f.nested_instances.len(), 1);
    assert!(!crate::ids::unset(&f.nested[0].id) && !f.nested_instances[0].guid.is_nil(), "both are parts with ids and GUIDs");
    let (_, small) = extent(&s.model, "Cabinet", "Small");
    let (_, large) = extent(&s.model, "Cabinet", "Large");
    assert!(near(small, [700.0, 150.0, 300.0]), "Small: {small:?}");
    assert!(near(large, [1300.0, 150.0, 600.0]), "Large: {large:?}");
    // A component of the cabinet takes the box with it, where it stands.
    let mut s = s;
    run(&mut s, "component", json!({"family": "Cabinet", "type": "Large", "at": [5000, 0]}));
    let id = *s.model.elements.keys().next_back().unwrap();
    let Some(crate::model::Element::Component(c)) = s.model.elements.get(id) else { panic!("a component") };
    let pieces = s.model.component_pieces(c);
    let top = pieces.iter().map(|p| p.top).fold(f64::NEG_INFINITY, f64::max);
    assert!((top - 600.0).abs() < 1e-6, "the box stands 600 high in the project: {top}");
    // No envelope from the forms alone: it is boxed by what it builds, nested instances and all.
    let v = s.model.families["Cabinet"].values("Large", &BTreeMap::new()).unwrap();
    assert!(s.model.families["Cabinet"].conservative_bounds(&v).is_none());
    // The host's parameter renamed: the association follows it.
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Width", "rename": "Breadth"}}));
    let inst = &s.model.families["Cabinet"].nested_instances[0];
    assert_eq!(inst.params["W"], crate::family::Expr::Formula("Breadth".into()));
    assert_eq!(inst.params["H"], crate::family::Expr::Formula("Breadth / 2".into()));
}

#[test]
fn a_nested_instance_turns_on_its_plane_and_swings_a_leaf_by_an_instance_parameter() {
    let mut s = cabinet();
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Swing Angle", "kind": "angle", "instance": true, "value": 45}}));
    let g = s.model.families["Cabinet"].nested_instances[0].guid;
    run(&mut s, "family", json!({"name": "Cabinet", "nested_instance": {"guid": g, "at": [0, 0], "angle": "Swing Angle"}}));
    let f = &s.model.families["Cabinet"];
    let v = f.values("Small", &BTreeMap::new()).unwrap();
    let corner = |x: f64, y: f64| {
        let (sn, c) = 45f64.to_radians().sin_cos();
        [x * c - y * sn, x * sn + y * c]
    };
    let far = corner(600.0, 100.0);
    let found = f
        .pieces(&v)
        .unwrap()
        .iter()
        .flat_map(|p| p.tris.iter().flatten())
        .any(|p| (p[0] - far[0]).abs() < 1e-6 && (p[1] - far[1]).abs() < 1e-6);
    assert!(found, "the box's far corner turned 45 degrees about its hinge");
}

#[test]
fn a_family_cant_be_loaded_into_itself_nor_nest_too_deep() {
    let mut s = cabinet();
    let e = err(&mut s, "family", json!({"name": "Cabinet", "nest": {"definition": box_definition(10.0), "name": "Cabinet"}}));
    assert!(e.contains("into itself"), "{e}");
    // A definition that nests the host's name further down.
    let mut inner = box_definition(10.0);
    inner["nested"] = json!([{"name": "Cabinet", "family": box_definition(5.0)}]);
    let e = err(&mut s, "family", json!({"name": "Cabinet", "nest": {"definition": inner, "name": "Deep"}}));
    assert!(e.contains("into itself"), "{e}");
    // Six levels down is one too many.
    let mut def = box_definition(1.0);
    for k in 0..6 {
        let mut up = box_definition(1.0);
        up["nested"] = json!([{"name": format!("L{k}"), "family": def}]);
        def = up;
    }
    let e = err(&mut s, "family", json!({"name": "Cabinet", "nest": {"definition": def, "name": "Tower"}}));
    assert!(e.contains("nested more than 5 deep"), "{e}");
    // An instance of what it doesn't nest, or of a type that isn't there.
    let e = err(&mut s, "family", json!({"name": "Cabinet", "nested_instance": {"family": "Nope", "type": "B"}}));
    assert!(e.contains("doesn't nest"), "{e}");
    let e = err(&mut s, "family", json!({"name": "Cabinet", "nested_instance": {"family": "Box", "type": "Z"}}));
    assert!(e.contains("has no type"), "{e}");
    // A nested family an instance places can't go.
    let e = err(&mut s, "family", json!({"name": "Cabinet", "delete_nested": ["Box"]}));
    assert!(e.contains("delete them first"), "{e}");
}

#[test]
fn a_nested_family_saves_inside_its_host_and_reloads_the_same() {
    let s = cabinet();
    let text = s.model.to_json().unwrap();
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.families["Cabinet"].nested, s.model.families["Cabinet"].nested);
    assert_eq!(back.families["Cabinet"].nested_instances, s.model.families["Cabinet"].nested_instances);
    assert_eq!(extent(&back, "Cabinet", "Large"), extent(&s.model, "Cabinet", "Large"));
    // Saved to a family file and loaded into a new project, it brings the box with it.
    let dir = std::env::temp_dir().join(format!("bimcraft-nested-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Cabinet.json");
    let mut s = s;
    run(&mut s, "family", json!({"name": "Cabinet", "save": path.to_string_lossy()}));
    let mut fresh = Session::new();
    run(&mut fresh, "family", json!({"path": path.to_string_lossy()}));
    assert_eq!(extent(&fresh.model, "Cabinet", "Large"), extent(&s.model, "Cabinet", "Large"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ifc_writes_the_nested_geometry_in_the_hosts_body() {
    let mut with = cabinet();
    run(&mut with, "component", json!({"family": "Cabinet", "type": "Large", "at": [0, 0]}));
    let mut without = copy(&with);
    let g = without.model.families["Cabinet"].nested_instances[0].guid;
    run(&mut without, "family", json!({"name": "Cabinet", "delete_nested_instance": [g.to_string()]}));
    // The triangles of the cabinet's body: its face set's indices, three to a triangle.
    let count = |s: &Session| {
        let file = crate::ifc::export(&s.model);
        let tris: usize = file.lines().filter(|l| l.contains("IFCTRIANGULATEDFACESET")).map(|l| l.matches('(').count()).sum();
        let points: String = file.lines().filter(|l| l.contains("IFCCARTESIANPOINTLIST3D")).collect();
        (tris, points)
    };
    let ((a, with_points), (b, without_points)) = (count(&with), count(&without));
    assert!(a > b + 10, "the box's triangles are in the cabinet's body: {a} against {b}");
    // The box reaches 1300 mm along x: a point of it is there, and not without it.
    let far = |p: &str| p.contains("1300.") || p.contains("1.3,");
    assert!(far(&with_points) && !far(&without_points), "a point of the box in the file");
}

#[test]
fn loading_a_nested_family_over_one_defined_otherwise_asks_and_overwrites_with_or_without_its_values() {
    let mut s = cabinet();
    let mut changed = box_definition(80.0);
    changed["types"] = json!({"B": {}, "C": {}});
    // Unasked, it's refused, saying what to choose.
    let e = err(&mut s, "family", json!({"name": "Cabinet", "nest": {"definition": changed.clone(), "name": "Box"}}));
    assert!(e.contains("overwrite") && e.contains("version_and_values"), "{e}");
    // The host's value of `W` was 50; `version` keeps it, `version_and_values` takes the incoming 80.
    let mut keep = copy(&s);
    run(&mut keep, "family", json!({"name": "Cabinet", "nest": {"definition": changed.clone(), "name": "Box", "overwrite": "version"}}));
    let nested = &keep.model.families["Cabinet"].nested[0].family;
    assert!(nested.types.contains_key("C"), "the new definition");
    assert_eq!(nested.parameters[0].value, json!(50.0));
    run(&mut s, "family", json!({"name": "Cabinet", "nest": {"definition": changed, "name": "Box", "overwrite": "version_and_values"}}));
    assert_eq!(s.model.families["Cabinet"].nested[0].family.parameters[0].value, json!(80.0));
    // The part keeps its id and GUID through the reload.
    assert_eq!(keep.model.families["Cabinet"].nested[0].guid, s.model.families["Cabinet"].nested[0].guid);
}

#[test]
fn a_shared_nested_family_the_project_has_otherwise_is_kept_or_overwritten_as_answered() {
    let mut s = cabinet();
    let mut shared = box_definition(50.0);
    shared["shared"] = json!(true);
    run(&mut s, "family", json!({"name": "Cabinet", "nest": {"definition": shared, "name": "Box", "overwrite": "version"}}));
    let definition = serde_json::to_value(&s.model.families["Cabinet"]).unwrap();
    // A project with its own Box, 70 wide.
    let project = |s: &mut Session| {
        let mut own = box_definition(70.0);
        own["shared"] = json!(true);
        own["name"] = json!("Box");
        run(s, "family", json!({"definition": own}));
    };
    let mut kept = Session::new();
    project(&mut kept);
    let dry = run(&mut kept, "family", json!({"definition": definition.clone(), "name": "Cabinet", "dry_run": true}));
    assert_eq!(dry["shared_conflicts"], json!(["Box"]));
    // Unanswered, the project keeps its own, and the cabinet takes it.
    run(&mut kept, "family", json!({"definition": definition.clone(), "name": "Cabinet"}));
    assert_eq!(kept.model.families["Box"].parameters[0].value, json!(70.0));
    assert_eq!(kept.model.families["Cabinet"].nested[0].family, kept.model.families["Box"]);
    // Answered "family", the project's is overwritten by the cabinet's.
    let mut taken = Session::new();
    project(&mut taken);
    run(&mut taken, "family", json!({"definition": definition.clone(), "name": "Cabinet", "shared_nested": {"Box": "family"}}));
    assert_eq!(taken.model.families["Box"].types.len(), 1);
    assert_eq!(taken.model.families["Box"], taken.model.families["Cabinet"].nested[0].family);
    // A project without it gains it.
    let mut fresh = Session::new();
    let r = run(&mut fresh, "family", json!({"definition": definition, "name": "Cabinet"}));
    assert_eq!(r["shared_loaded"], json!(["Box"]));
    assert!(fresh.model.families.contains_key("Box"));
    // One definition across the project: the project's Box changed, the cabinet's nested one follows.
    run(&mut fresh, "family", json!({"name": "Box", "types": {"Wide": {"W": 900}}}));
    assert!(fresh.model.families["Cabinet"].nested[0].family.types.contains_key("Wide"));
}

#[test]
fn a_nested_material_follows_a_rename_and_counts_as_used() {
    let mut s = cabinet();
    run(&mut s, "material", json!({"name": "Oak", "color": [180, 140, 90]}));
    let mut wood = box_definition(50.0);
    wood["forms"][0]["material"] = json!("Oak");
    run(&mut s, "family", json!({"name": "Cabinet", "nest": {"definition": wood, "name": "Box", "overwrite": "version"}}));
    let f: &Family = &s.model.families["Cabinet"];
    assert!(f.materials().contains(&"Oak"));
    run(&mut s, "material", json!({"name": "Oak", "rename": "Walnut"}));
    assert_eq!(s.model.families["Cabinet"].nested[0].family.forms[0].material, "Walnut");
}

#[test]
fn deleting_a_plane_that_hosts_a_nested_instance_is_refused() {
    let mut s = cabinet();
    run(
        &mut s,
        "family",
        json!({"name": "Cabinet", "ref_planes": [{"name": "Shelf", "start": [0, 0, 300], "end": [100, 0, 300], "up": [0, 1, 0]}]}),
    );
    let g = s.model.families["Cabinet"].nested_instances[0].guid;
    run(&mut s, "family", json!({"name": "Cabinet", "nested_instance": {"guid": g, "host": "Reference Plane : Shelf"}}));
    let (_, hi) = extent(&s.model, "Cabinet", "Small");
    assert!((hi[2] - 600.0).abs() < 1e-6, "the box, 300 high, stands on the shelf plane at 300: {hi:?}");
    let plane = s.model.families["Cabinet"].planes[3].guid;
    let e = err(&mut s, "family", json!({"name": "Cabinet", "delete_ref_plane": [plane.to_string()]}));
    assert!(e.contains("nested instance 1"), "{e}");
}

/// A shared family `Leaf` whose extrusion runs to `to`.
fn leaf(to: f64) -> Value {
    json!({"category": "generic", "shared": true, "types": {"B": {}},
           "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "to": to}]})
}

/// A shared family nesting `child` as `Leaf`.
fn branch(child: Value) -> Value {
    json!({"category": "generic", "shared": true, "types": {"A": {}},
           "forms": [{"profile": [[0, 0], [50, 0], [50, 50], [0, 50]], "to": 5}], "nested": [{"name": "Leaf", "family": child}]})
}

/// A family whose two shared branches, P and Q, each hold a copy of the shared Leaf, defined otherwise (P's to 10, Q's to
/// 20), is loaded with one definition of Leaf: the first met, P's, everywhere, in the project and in every copy, whatever
/// order the shared families are taken in.
#[test]
fn a_shared_family_nested_twice_otherwise_loads_as_one_definition_the_first_met() {
    let root = json!({"category": "furniture", "types": {"A": {}},
        "forms": [{"profile": [[0, 0], [10, 0], [10, 10], [0, 10]], "to": 10}],
        "nested": [{"name": "P", "family": branch(leaf(10.0))}, {"name": "Q", "family": branch(leaf(20.0))}]});
    let mut s = Session::new();
    run(&mut s, "family", json!({"name": "Root", "definition": root, "overwrite": "version_and_values"}));
    let m = &s.model;
    let to = |f: &Family| serde_json::to_value(f).unwrap()["forms"][0]["to"].as_f64();
    let nested = |f: &Family, name: &str| f.nested.iter().find(|n| n.name == name).map(|n| n.family.clone()).unwrap();
    // Every copy as the project's Leaf: P's, to 10 (ids and GUIDs aside, which the project's own entries are given).
    assert_eq!(to(&m.families["Leaf"]), Some(10.0), "the project's Leaf, the first copy met");
    let root = &m.families["Root"];
    for (who, f) in [
        ("Root.P.Leaf", nested(&nested(root, "P"), "Leaf")),
        ("Root.Q.Leaf", nested(&nested(root, "Q"), "Leaf")),
        ("P.Leaf", nested(&m.families["P"], "Leaf")),
        ("Q.Leaf", nested(&m.families["Q"], "Leaf")),
    ] {
        assert_eq!(to(&f), Some(10.0), "{who}");
    }
}
