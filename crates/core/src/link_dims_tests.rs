//! Dimensions to a linked model's elements (issue 537), as Revit dimensions to a link: the reference names the link and the
//! element's GUID in the linked file, which never change, and is measured in the link's model where the link stands.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id, Model, Reference};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn err(s: &mut Session, c: &str, p: Value) -> String {
    s.execute(c, &p).err().unwrap_or_else(|| panic!("{c} {p} was accepted"))
}

fn id(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_default()
}

/// A box of walls 8000 by 6000 from the origin, saved to a file: the path.
fn architect(name: &str) -> String {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    let path = std::env::temp_dir().join(format!("bimcraft-link-dims-{name}-{}.bimcraft", std::process::id()));
    let path = path.to_string_lossy().into_owned();
    run(&mut s, "save", json!({"path": path}));
    path
}

/// A project linking the box at (1000, 2000), with a family of its own and one of it at (3000, 4000): the link and the
/// component.
fn linked(s: &mut Session, path: &str) -> (Id, Id) {
    let link = id(&run(s, "link", json!({"path": path, "name": "Architecture", "at": [1000, 2000]})));
    run(
        s,
        "family",
        json!({"name": "Box", "types": {"A": {}}, "forms": [{"profile": [[-100, -100], [100, -100], [100, 100], [-100, 100]], "to": 100}]}),
    );
    let c = id(&run(s, "component", json!({"family": "Box", "type": "A", "at": [3000, 4000]})));
    (link, c)
}

fn refs(m: &Model, d: Id) -> Vec<Reference> {
    match m.elements.get(d) {
        Some(Element::Dimension(dim)) => dim.refs.clone(),
        other => panic!("{d} is {other:?}"),
    }
}

fn values(m: &Model, d: Id) -> Vec<f64> {
    match m.elements.get(d) {
        Some(Element::Dimension(dim)) => m.dimension_values(dim).into_iter().map(|v| (v * 1e6).round() / 1e6).collect(),
        other => panic!("{d} is {other:?}"),
    }
}

#[test]
fn a_point_on_a_linked_wall_names_the_link_and_the_walls_guid_there_and_follows_the_link() {
    let path = architect("point");
    let mut s = Session::new();
    let (link, c) = linked(&mut s, &path);
    // The linked box's west wall runs up x = 1000 here; the component's line across it stands at x = 3000.
    let d = id(&run(&mut s, "dimension", json!({"refs": [[1000, 4500], {"id": c, "face": "across"}], "at": [2000, 5000]})));
    assert_eq!(values(&s.model, d), [2000.0]);
    let r = refs(&s.model, d)[0];
    assert_eq!(r.link, Some(s.model.guid(link)));
    let there = &s.model.loaded[&path];
    let west = there.elements.iter().find_map(|(i, e)| matches!(e, Element::Wall(w) if w.start.0 == 0.0 && w.end.0 == 0.0).then_some(*i));
    assert_eq!(Some(r.id), west.map(|w| there.guid(w)), "the wall's GUID in the linked file");
    assert!(s.model.elements.get(r.id).is_none(), "which is no element of the project");
    // The link moved half a metre east: the dimension follows it.
    run(&mut s, "move", json!({"ids": [link], "by": [500, 0]}));
    assert_eq!(values(&s.model, d), [1500.0]);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_linked_element_is_taken_by_its_id_in_the_views_and_two_links_of_one_file_stay_apart() {
    let path = architect("twice");
    let mut s = Session::new();
    let (first, c) = linked(&mut s, &path);
    let second = id(&run(&mut s, "link", json!({"path": path, "name": "Again", "at": [20000, 2000]})));
    // The views' ids of each link's west wall, as the Dimension tool sees them.
    let seen = crate::link::seen(&s.model);
    let west = |x: f64| {
        seen.elements
            .iter()
            .find_map(|(i, e)| matches!(e, Element::Wall(w) if w.start.0 == x && w.end.0 == x).then_some(*i))
            .unwrap_or_default()
    };
    let (a, b) = (west(1000.0), west(20000.0));
    assert!(crate::link::is_linked(a) && crate::link::is_linked(b));
    let d = id(&run(&mut s, "dimension", json!({"refs": [{"id": a}, {"id": c, "face": "across"}, {"id": b}], "at": [2000, 5000]})));
    assert_eq!(values(&s.model, d), [2000.0, 17000.0]);
    let r = refs(&s.model, d);
    assert_eq!((r[0].link, r[2].link), (Some(s.model.guid(first)), Some(s.model.guid(second))));
    assert_eq!(r[0].id, r[2].id, "the same wall of the same file, in two links");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_dimension_to_a_link_is_never_locked_survives_the_link_unloaded_and_goes_with_it() {
    let path = architect("lost");
    let mut s = Session::new();
    let (link, c) = linked(&mut s, &path);
    let d = id(&run(&mut s, "dimension", json!({"refs": [[1000, 4500], {"id": c, "face": "across"}], "at": [2000, 5000]})));
    // It measures; it can't hold what it measures.
    let e = err(&mut s, "dimension_constraint", json!({"id": d, "locked": [0]}));
    assert!(e.contains("linked model"), "{e}");
    // Read back without the link loaded (a file never holds what a link read): it stands, undrawn, and the project checks.
    let (again, _) = Model::read(&s.model.to_json().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(again.loaded.is_empty());
    assert!(again.elements.get(d).is_some());
    assert_eq!(again.check(), Ok(()));
    // The link taken away: its references go, and a dimension left with one goes too.
    run(&mut s, "delete", json!({"ids": [link]}));
    assert!(s.model.elements.get(d).is_none());
    let _ = std::fs::remove_file(&path);
}
