//! Deleting a type or a family, and Purge Unused: what goes, what is refused, what stays so that a tool has something to place.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Element;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn walls() -> Session {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    s
}

fn wall_types(s: &Session) -> Vec<String> {
    s.model.types.wall.keys().cloned().collect()
}

fn chair(s: &mut Session) {
    run(
        s,
        "family",
        json!({"name": "Chair", "category": "furniture", "types": {"Small": {}, "Large": {}},
            "forms": [{"plane": "xy", "profile": [[-200, -200], [200, -200], [200, 200], [-200, 200]], "to": 450}]}),
    );
}

#[test]
fn an_unused_type_is_deleted_and_a_used_one_is_refused_with_how_many_use_it() {
    let mut s = walls();
    run(&mut s, "type", json!({"category": "wall", "name": "Spare 100", "thickness": 100}));
    assert!(wall_types(&s).contains(&"Spare 100".to_string()));
    let r = run(&mut s, "delete_type", json!({"category": "wall", "type": "Spare 100"}));
    assert_eq!(r["deleted"], json!({"kind": "wall type", "name": "Spare 100"}));
    assert!(!wall_types(&s).contains(&"Spare 100".to_string()));
    // The wall's own type is used once.
    let used = s.model.elements.values().find_map(|e| if let Element::Wall(w) = e { Some(w.type_name.clone()) } else { None }).unwrap();
    let e = s.execute("delete_type", &json!({"category": "wall", "type": used})).unwrap_err();
    assert!(e.contains("is used 1 time"), "{e}");
    let e = s.execute("delete_type", &json!({"category": "wall", "type": "No such"})).unwrap_err();
    assert!(e.contains("no wall type") && e.contains("there are:"), "{e}");
    let e = s.execute("delete_type", &json!({"category": "wall"})).unwrap_err();
    assert!(e.contains("give `category` and `type`"), "{e}");
    // It undoes.
    run(&mut s, "undo", json!({}));
    assert!(wall_types(&s).contains(&"Spare 100".to_string()));
}

#[test]
fn the_last_type_of_a_category_cannot_be_deleted() {
    let mut s = Session::new();
    let names = wall_types(&s);
    for n in &names[1..] {
        run(&mut s, "delete_type", json!({"category": "wall", "type": n}));
    }
    assert_eq!(wall_types(&s), vec![names[0].clone()]);
    let e = s.execute("delete_type", &json!({"category": "wall", "type": names[0]})).unwrap_err();
    assert!(e.contains("last wall type"), "{e}");
}

#[test]
fn purge_takes_out_what_nothing_uses_but_keeps_a_type_of_each_category_and_what_a_wall_stands_on() {
    let mut s = walls();
    run(&mut s, "type", json!({"category": "wall", "name": "Spare 100", "thickness": 100}));
    run(&mut s, "type", json!({"category": "window", "name": "Round 600", "width": 600, "height": 600}));
    let before = s.model.types.wall.len();
    // A dry run says what would go, and changes nothing, not even the undo history.
    let steps = s.history().len();
    let dry = run(&mut s, "purge", json!({"dry_run": true}));
    assert!(dry["count"].as_u64().unwrap() > 0 && dry["dry_run"] == true);
    assert_eq!((s.model.types.wall.len(), s.history().len()), (before, steps));
    let r = run(&mut s, "purge", json!({}));
    assert_eq!(r["count"], dry["count"], "the dry run was right");
    let used = s.model.elements.values().find_map(|e| if let Element::Wall(w) = e { Some(w.type_name.clone()) } else { None }).unwrap();
    assert_eq!(wall_types(&s), vec![used], "the wall's type, and no more: the category keeps one");
    assert_eq!(s.model.types.window.len(), 1, "and a window type, though none is used: the default's");
    assert_eq!(s.history().len(), steps + 1, "one undo step");
    assert_eq!(run(&mut s, "purge", json!({}))["count"], 0, "nothing more");
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.types.wall.len(), before);
}

#[test]
fn a_family_keeps_its_used_type_and_goes_when_none_of_it_is_used() {
    let mut s = Session::new();
    chair(&mut s);
    let component = run(&mut s, "component", json!({"family": "Chair", "type": "Small", "at": [1000, 1000]}))["id"].as_u64().unwrap();
    let r = run(&mut s, "purge", json!({}));
    let gone: Vec<&str> =
        r["removed"].as_array().unwrap().iter().filter(|e| e["kind"] == "family type").filter_map(|e| e["name"].as_str()).collect();
    assert!(gone.contains(&"Chair: Large") && !gone.contains(&"Chair: Small"), "{gone:?}");
    assert_eq!(s.model.families["Chair"].types.keys().collect::<Vec<_>>(), ["Small"], "the type in use is the one left");
    // The family is used, so it stays; delete the component and it goes with its types.
    let e = s.execute("delete_type", &json!({"family": "Chair"})).unwrap_err();
    assert!(e.contains("is used"), "{e}");
    run(&mut s, "delete", json!({"ids": [component]}));
    let r = run(&mut s, "purge", json!({}));
    assert!(r["removed"].as_array().unwrap().iter().any(|e| e["kind"] == "family" && e["name"] == "Chair"));
    assert!(!s.model.families.contains_key("Chair"));
}

#[test]
fn one_family_type_can_be_deleted_when_unused_and_not_when_it_is_the_last() {
    let mut s = Session::new();
    chair(&mut s);
    let others: Vec<String> = s.model.families["Chair"].types.keys().filter(|t| *t != "Small").cloned().collect();
    for t in &others {
        run(&mut s, "delete_type", json!({"family": "Chair", "type": t}));
    }
    let e = s.execute("delete_type", &json!({"family": "Chair", "type": "Small"})).unwrap_err();
    assert!(e.contains("last type: delete the family"), "{e}");
    run(&mut s, "delete_type", json!({"family": "Chair"}));
    assert!(!s.model.families.contains_key("Chair"));
}

#[test]
fn purging_materials_follows_the_types_that_use_them_and_repeats_for_what_that_frees() {
    let mut s = walls();
    let only = |s: &Session| s.model.materials.keys().cloned().collect::<Vec<_>>();
    let first = only(&s).len();
    // Types first (their layers' materials come free), then the materials nothing names.
    run(&mut s, "purge", json!({"kinds": ["type", "material"]}));
    let left = only(&s);
    assert!(left.len() < first, "{} of {first} are left", left.len());
    let layers: Vec<String> = s.model.types.wall.values().flat_map(|t| t.layers.iter().map(|l| l.material.clone())).collect();
    assert!(layers.iter().filter(|m| !m.is_empty()).all(|m| left.contains(m)), "the wall type's materials stay");
    let e = s.execute("purge", &json!({"kinds": ["wall"]})).unwrap_err();
    assert!(e.contains("`kinds` holds"), "{e}");
    let e = s.execute("purge", &json!({"kinds": "type"})).unwrap_err();
    assert!(e.contains("list of what to purge"), "{e}");
}

#[test]
fn a_purged_project_is_saved_and_read_back_whole() {
    let mut s = walls();
    chair(&mut s);
    run(&mut s, "component", json!({"family": "Chair", "type": "Small", "at": [1000, 1000]}));
    run(&mut s, "purge", json!({"kinds": ["type", "family", "material", "picture", "filter", "template"]}));
    let back = crate::model::Model::from_json(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(back.elements, s.model.elements);
    assert_eq!((back.types.wall.len(), back.families.len()), (s.model.types.wall.len(), s.model.families.len()));
    assert!(back.check().is_ok());
}
