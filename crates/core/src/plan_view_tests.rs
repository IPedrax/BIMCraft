//! Plan views besides a level's own (Revit's Duplicate View): what they share with the level, what is theirs, and how they come
//! and go. Driven through the commands every front end drives.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::graphics::Look;
use crate::model::{Element, Id};
use crate::render::{self, Drawing};

const BOX: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
door {"at": [4000, 0]}
floor {"boundary": [[0,0],[8000,0],[8000,6000],[0,6000]]}
level {"elevation": 3000}"#;

fn house() -> Session {
    let mut s = Session::new();
    s.script(BOX).unwrap();
    s
}

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_u64().unwrap()
}

/// What plan `view` draws at the framing of the first level's own plan.
fn drawn(s: &Session, view: Id) -> Drawing {
    let level = s.model.levels()[0].0;
    render::draw(&s.model, view, render::plan(&s.model, level, 800, 600).view)
}

/// Whether the drawing holds something of element `id`.
fn has(d: &Drawing, id: Id) -> bool {
    d.owners.contains(&Some(id))
}

#[test]
fn a_duplicate_is_a_plan_view_of_the_same_level_named_after_it_and_numbered() {
    let mut s = house();
    let level = s.model.levels()[0].0;
    let a = run(&mut s, "duplicate_view", json!({"view": "Level 1"}));
    let b = run(&mut s, "duplicate_view", json!({"view": "Level 1"}));
    assert_eq!((a["name"].as_str(), b["name"].as_str()), (Some("Level 1 Copy 1"), Some("Level 1 Copy 2")));
    assert_eq!(s.model.plan_level(id(&a)), Some(level));
    assert_eq!(s.model.plan_views(level).len(), 2);
    // A name of its own, but never a level's or another view's.
    let named = run(&mut s, "duplicate_view", json!({"view": "Level 1", "name": "Plumbing"}));
    assert_eq!(s.model.view_name(id(&named)), Some("Plumbing"));
    for taken in ["Level 2", "Plumbing"] {
        let e = s.execute("duplicate_view", &json!({"view": "Level 1", "name": taken})).unwrap_err();
        assert!(e.contains("already"), "{taken}: {e}");
    }
    // A view of a plan view is a view of the same level.
    let again = run(&mut s, "duplicate_view", json!({"view": "Plumbing"}));
    assert_eq!(again["name"], "Plumbing Copy 1");
    assert_eq!(s.model.plan_level(id(&again)), Some(level));
}

#[test]
fn only_plans_are_duplicated_and_the_mode_is_one_of_three() {
    let mut s = house();
    run(&mut s, "elevation", json!({"facing": "north"}));
    let e = s.execute("duplicate_view", &json!({"view": "North"})).unwrap_err();
    assert!(e.contains("isn't a plan"), "{e}");
    let e = s.execute("duplicate_view", &json!({"view": "Level 1", "mode": "twin"})).unwrap_err();
    assert!(e.contains("duplicate, detailing or dependent"), "{e}");
    let e = s.execute("duplicate_view", &json!({"view": "No such view"})).unwrap_err();
    assert!(e.contains("no view named"), "{e}");
}

#[test]
fn a_duplicate_has_the_range_crop_and_graphics_of_its_source_and_then_its_own() {
    let mut s = house();
    let walls: Vec<Id> = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(i, _)| *i).collect();
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["door"]}));
    run(&mut s, "crop", json!({"view": "Level 1", "region": [[-500, -500], [8500, 6500]]}));
    let level = s.model.levels()[0].0;
    run(&mut s, "modify", json!({"id": level, "view_range": {"cut": 1500}}));
    let copy = id(&run(&mut s, "duplicate_view", json!({"view": "Level 1"})));
    let m = &s.model;
    assert_eq!(m.view_range_of(copy).cut, 1500.0);
    assert_eq!(m.crop_of(copy), m.crop_of(m.levels()[0].0));
    assert!(!Look::of(m, copy).shows(find(m, |e| matches!(e, Element::Door(_)))), "the door stays hidden in the copy");
    // Then it goes its own way: walls hidden in the copy only.
    run(&mut s, "graphics", json!({"view": "Level 1 Copy 1", "hide": ["wall"]}));
    run(&mut s, "modify", json!({"id": copy, "view_range": {"cut": 900}}));
    let m = &s.model;
    assert!(walls.iter().all(|w| !Look::of(m, copy).shows(*w) && Look::of(m, m.levels()[0].0).shows(*w)));
    assert_eq!((m.view_range_of(copy).cut, m.view_range_of(m.levels()[0].0).cut), (900.0, 1500.0));
    // Cropping the copy leaves the level's plan alone.
    run(&mut s, "crop", json!({"view": "Level 1 Copy 1", "region": null}));
    assert_eq!((s.model.crop_of(copy), s.model.crop_of(s.model.levels()[0].0).is_some()), (None, true));
}

fn find(m: &crate::model::Model, f: impl Fn(&Element) -> bool) -> Id {
    m.elements.iter().find(|(_, e)| f(e)).map(|(i, _)| *i).unwrap()
}

#[test]
fn an_annotation_is_in_the_view_it_was_made_in_and_only_there() {
    let mut s = house();
    run(&mut s, "duplicate_view", json!({"view": "Level 1"}));
    let door = find(&s.model, |e| matches!(e, Element::Door(_)));
    let refs = json!([{"id": find(&s.model, |e| matches!(e, Element::Wall(w) if w.start.0 == 0.0 && w.start.1 == 0.0)), "face": "centre"},
        {"id": find(&s.model, |e| matches!(e, Element::Wall(w) if w.start.0 == 8000.0 && w.start.1 == 6000.0)), "face": "centre"}]);
    // The same four kinds in each of the two plans of the level.
    let mut made = Vec::new();
    for (view, y) in [("Level 1", -2000), ("Level 1 Copy 1", -3000)] {
        let dim = id(&run(&mut s, "dimension", json!({"level": view, "refs": refs, "at": [4000, y]})));
        let tag = run(&mut s, "tag", json!({"ids": [door], "level": view}))["ids"][0].as_u64().unwrap();
        let note = id(&run(&mut s, "text", json!({"text": view, "at": [1000, y], "view": view})));
        let spot = id(&run(&mut s, "spot", json!({"at": [2000, 2000], "view": view})));
        made.push((view, [dim, tag, note, spot]));
    }
    let own = s.model.levels()[0].0;
    let copy = find(&s.model, |e| matches!(e, Element::PlanView(_)));
    let (in_level, in_copy) = (drawn(&s, own), drawn(&s, copy));
    for (view, ids) in &made {
        for i in ids {
            assert_eq!((has(&in_level, *i), has(&in_copy, *i)), (*view == "Level 1", *view == "Level 1 Copy 1"), "{view}: element {i}");
        }
    }
    // What says which view it is in: the copy's own GUID, or nothing for the level's.
    let tag_in = |i: Id| match &s.model.elements[&i] {
        Element::Tag(t) => t.view,
        _ => panic!(),
    };
    assert_eq!((tag_in(made[0].1[1]), tag_in(made[1].1[1])), (None, Some(s.model.guid(copy))));
}

#[test]
fn tag_all_tags_what_the_plan_has_no_tag_of_in_that_plan() {
    let mut s = house();
    run(&mut s, "duplicate_view", json!({"view": "Level 1"}));
    run(&mut s, "tag", json!({"level": "Level 1"}));
    // The copy has none of its own yet: its tag is not the level's.
    let r = run(&mut s, "tag", json!({"level": "Level 1 Copy 1"}));
    assert_eq!(r["ids"].as_array().map(Vec::len), Some(1));
    let e = s.execute("tag", &json!({"level": "Level 1 Copy 1"})).unwrap_err();
    assert!(e.contains("nothing to tag"), "{e}");
}

#[test]
fn detailing_copies_the_annotation_and_the_plain_duplicate_does_not() {
    let mut s = house();
    run(&mut s, "text", json!({"text": "note", "at": [1000, 1000], "view": "Level 1"}));
    run(&mut s, "spot", json!({"at": [2000, 2000], "view": "Level 1"}));
    run(&mut s, "tag", json!({"level": "Level 1"}));
    let count =
        |s: &Session| s.model.elements.values().filter(|e| matches!(e, Element::Text(_) | Element::Spot(_) | Element::Tag(_))).count();
    let before = count(&s);
    let plain = run(&mut s, "duplicate_view", json!({"view": "Level 1"}));
    assert_eq!((plain["annotations"].as_u64(), count(&s)), (Some(0), before));
    let detailed = run(&mut s, "duplicate_view", json!({"view": "Level 1", "mode": "detailing"}));
    assert_eq!((detailed["annotations"].as_u64(), count(&s)), (Some(3), before + 3));
    // The copies are the new view's, the originals are still the level's.
    let in_new = drawn(&s, id(&detailed));
    let own = drawn(&s, s.model.levels()[0].0);
    let notes: Vec<Id> = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Text(_))).map(|(i, _)| *i).collect();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes.iter().filter(|n| has(&in_new, **n)).count(), 1);
    assert_eq!(notes.iter().filter(|n| has(&own, **n)).count(), 1);
}

#[test]
fn a_dependent_follows_its_parents_range_and_graphics_has_its_own_crop_and_shows_its_parents_annotation() {
    let mut s = house();
    run(&mut s, "text", json!({"text": "parent's", "at": [1000, 1000], "view": "Level 1"}));
    let dep = id(&run(&mut s, "duplicate_view", json!({"view": "Level 1", "mode": "dependent"})));
    assert_eq!(s.model.view_name(dep), Some("Level 1 Dependent 1"));
    let level = s.model.levels()[0].0;
    // What the parent's graphics and range say, the dependent says.
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["door"]}));
    run(&mut s, "modify", json!({"id": level, "view_range": {"cut": 1700}}));
    let door = find(&s.model, |e| matches!(e, Element::Door(_)));
    assert!(!Look::of(&s.model, dep).shows(door));
    assert_eq!(s.model.view_range_of(dep).cut, 1700.0);
    // It cannot change them itself.
    for (command, params) in [
        ("graphics", json!({"view": "Level 1 Dependent 1", "hide": ["wall"]})),
        ("scale", json!({"view": "Level 1 Dependent 1", "scale": 50})),
        ("detail", json!({"view": "Level 1 Dependent 1", "detail": "coarse"})),
    ] {
        let e = s.execute(command, &params).unwrap_err();
        assert!(e.contains("dependent view") && e.contains("Level 1"), "{command}: {e}");
    }
    // Its crop is its own.
    run(&mut s, "crop", json!({"view": "Level 1 Dependent 1", "region": [[0, 0], [4000, 3000]]}));
    assert_eq!((s.model.crop_of(dep).is_some(), s.model.crop_of(level)), (true, None));
    // The parent's annotation shows in it, its own does not show in the parent.
    let parents = find(&s.model, |e| matches!(e, Element::Text(_)));
    run(&mut s, "text", json!({"text": "dependent's", "at": [1500, 1500], "view": "Level 1 Dependent 1"}));
    let mine = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Text(_))).map(|(i, _)| *i).find(|i| *i != parents).unwrap();
    let (in_dep, in_parent) = (drawn(&s, dep), drawn(&s, level));
    assert_eq!((has(&in_dep, parents), has(&in_dep, mine)), (true, true));
    assert_eq!((has(&in_parent, parents), has(&in_parent, mine)), (true, false));
    // No dependent of a dependent.
    let e = s.execute("duplicate_view", &json!({"view": "Level 1 Dependent 1", "mode": "dependent"})).unwrap_err();
    assert!(e.contains("dependent view"), "{e}");
}

#[test]
fn deleting_takes_a_views_annotation_a_parents_dependents_and_a_levels_plan_views() {
    let mut s = house();
    let detailed = id(&run(&mut s, "duplicate_view", json!({"view": "Level 1"})));
    run(&mut s, "text", json!({"text": "in the copy", "at": [1000, 1000], "view": "Level 1 Copy 1"}));
    run(&mut s, "tag", json!({"level": "Level 1 Copy 1"}));
    let dep = id(&run(&mut s, "duplicate_view", json!({"view": "Level 1 Copy 1", "mode": "dependent"})));
    run(&mut s, "level", json!({"elevation": 9000}));
    let upper = id(&run(&mut s, "duplicate_view", json!({"view": "Level 3"})));
    let count = |s: &Session, f: fn(&Element) -> bool| s.model.elements.values().filter(|e| f(e)).count();
    run(&mut s, "delete", json!({"ids": [detailed]}));
    assert!(s.model.elements.get(dep).is_none(), "its dependent went with it");
    assert_eq!(count(&s, |e| matches!(e, Element::Text(_) | Element::Tag(_))), 0, "and what was annotated in it");
    assert!(s.model.elements.get(upper).is_some());
    // A level takes its plan views with it.
    let third = s.model.levels()[2].0;
    run(&mut s, "delete", json!({"ids": [third]}));
    assert!(s.model.elements.get(upper).is_none());
    assert_eq!(count(&s, |e| matches!(e, Element::PlanView(_))), 0, "none is left");
}

#[test]
fn a_plan_view_goes_on_a_sheet_is_fitted_to_its_crop_and_is_found_by_name() {
    let mut s = house();
    let copy = id(&run(&mut s, "duplicate_view", json!({"view": "Level 1", "name": "Cropped"})));
    run(&mut s, "crop", json!({"view": "Cropped", "region": [[1000, 1000], [3000, 2000]]}));
    // Fitted to its crop, not to the whole plan.
    let fitted = render::plan(&s.model, copy, 800, 600).view;
    let whole = render::plan(&s.model, s.model.levels()[0].0, 800, 600).view;
    assert!(fitted.scale > whole.scale * 2.0, "{} against {}", fitted.scale, whole.scale);
    run(&mut s, "sheet", json!({}));
    run(&mut s, "place", json!({"sheet": "A101", "view": "Cropped"}));
    let dir = std::env::temp_dir();
    let path = |name: &str| dir.join(format!("plan-view-{}-{name}.png", std::process::id()));
    let sheet = run(&mut s, "render", json!({"view": "A101", "path": path("sheet").to_str().unwrap()}));
    let plan = run(&mut s, "render", json!({"view": "Cropped", "path": path("plan").to_str().unwrap()}));
    assert_eq!((sheet["kind"].as_str(), plan["kind"].as_str()), (Some("sheet"), Some("plan")));
    for name in ["sheet", "plan"] {
        let _ = std::fs::remove_file(path(name));
    }
}

#[test]
fn plan_views_are_saved_with_the_project_and_a_project_without_them_is_written_as_before() {
    let mut s = house();
    let plain = s.model.to_json().unwrap();
    assert!(!plain.contains("planview") && !plain.contains("\"view\""), "nothing of it in a project that has none");
    run(&mut s, "duplicate_view", json!({"view": "Level 1", "mode": "detailing"}));
    run(&mut s, "tag", json!({"level": "Level 1 Copy 1"}));
    let written = s.model.to_json().unwrap();
    let back = crate::model::Model::from_json(&written).unwrap();
    assert_eq!(back.elements, s.model.elements);
    assert!(written.contains("planview"));
}

#[test]
fn a_plan_views_range_follows_a_view_template_that_controls_it() {
    let mut s = house();
    let level = s.model.levels()[0].0;
    run(&mut s, "modify", json!({"id": level, "view_range": {"cut": 1500}}));
    run(&mut s, "template", json!({"name": "Low", "from": "Level 1", "controls": ["range"]}));
    let copy = id(&run(&mut s, "duplicate_view", json!({"view": "Level 1"})));
    run(&mut s, "modify", json!({"id": copy, "view_range": {"cut": 800}}));
    assert_eq!(s.model.view_range_of(copy).cut, 800.0);
    run(&mut s, "graphics", json!({"view": "Level 1 Copy 1", "template": "Low"}));
    assert_eq!(s.model.view_range_of(copy).cut, 1500.0, "the template's range, as a level's plan takes it");
}
