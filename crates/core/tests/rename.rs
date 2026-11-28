//! `rename`: one command for a view, a level, a schedule, a sheet, a family and a type, with the built-in schedules and the
//! default `{3D}` made elements the first time they are renamed.

#![allow(clippy::unwrap_used)]

use bimcraft_core::engine::Session;
use bimcraft_core::model::Element;
use bimcraft_core::schedule;
use serde_json::json;

fn level(s: &Session, name: &str) -> u64 {
    s.model.levels().into_iter().find(|(_, l)| l.name == name).map(|(id, _)| id).unwrap()
}

#[test]
fn a_level_is_renamed_with_one_undo_step_and_its_plans_follow() {
    let mut s = Session::new();
    let one = level(&s, "Level 1");
    let before = s.history().len();
    let reply = s.execute("rename", &json!({"id": one, "rename": "Ground"})).unwrap();
    assert_eq!(reply["kind"], "level");
    assert_eq!(reply["from"], "Level 1");
    assert_eq!(reply["to"], "Ground");
    assert_eq!(s.model.view_name(one), Some("Ground"));
    assert_eq!(s.history().len(), before + 1, "one Undo step");
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.view_name(one), Some("Level 1"));
}

#[test]
fn a_sheet_keeps_its_number_and_a_name_that_is_another_number_is_refused() {
    let mut s = Session::new();
    let first = s.execute("sheet", &json!({"number": "A101", "name": "Plans"})).unwrap()["id"].as_u64().unwrap();
    s.execute("sheet", &json!({"number": "A102", "name": "Sections"})).unwrap();
    let reply = s.execute("rename", &json!({"sheet": "A101", "rename": "Floor Plans"})).unwrap();
    assert_eq!(reply["kind"], "sheet");
    assert_eq!(reply["id"], first);
    let sheet = s.model.sheet(first).unwrap();
    assert_eq!((sheet.number.as_str(), sheet.name.as_str()), ("A101", "Floor Plans"));
    let after = s.model.clone();
    assert!(s.execute("rename", &json!({"sheet": "A101", "rename": "A102"})).is_err(), "another sheet's number");
    assert!(s.execute("rename", &json!({"sheet": "A101", "rename": "  "})).is_err(), "empty");
    assert_eq!(s.model, after, "a refused name changes nothing");
    assert_eq!(s.model.sheet(first).unwrap().number, "A101");
}

#[test]
fn a_builtin_schedule_is_made_an_element_once_and_kept_one_row() {
    let mut s = Session::new();
    let before = s.history().len();
    let reply = s.execute("rename", &json!({"schedule": "door", "rename": "Doors"})).unwrap();
    assert_eq!(reply["kind"], "schedule");
    assert_eq!(reply["created"], json!(true));
    let id = reply["id"].as_u64().unwrap();
    let Some(Element::Schedule(sc)) = s.model.elements.get(id) else { panic!("a schedule was made") };
    assert_eq!((sc.name.as_str(), sc.category().as_str(), sc.standard), ("Doors", "door", true));
    assert_eq!(s.history().len(), before + 1);
    // Its built-in name still finds it, so the open tab is the same table and the browser lists it once.
    assert_eq!(schedule::standard(&s.model, "door").0, Some(id));
    assert_eq!(schedule::named(&s.model, "Door Schedule"), Some(id));
    // The same name again is no change and no Undo step.
    let again = s.execute("rename", &json!({"schedule": "door", "rename": "Doors"})).unwrap();
    assert_eq!(again["id"], id);
    assert_eq!(again.get("created"), None);
    assert_eq!(s.history().len(), before + 1);
    // Undo takes the element away and the built-in row is virtual again.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(schedule::standard(&s.model, "door").0, None);
}

#[test]
fn a_builtin_schedule_cannot_take_another_category_name() {
    let mut s = Session::new();
    let after = s.model.clone();
    assert!(s.execute("rename", &json!({"schedule": "door", "rename": "Room Schedule"})).is_err());
    assert_eq!(s.model, after);
}

#[test]
fn the_default_3d_is_made_an_element_and_undo_makes_it_virtual_again() {
    let mut s = Session::new();
    let before = s.history().len();
    assert_eq!(s.model.default_3d(), None, "virtual until changed");
    let reply = s.execute("rename", &json!({"view": "{3D}", "rename": "Model"})).unwrap();
    assert_eq!(reply["created"], json!(true));
    let id = reply["id"].as_u64().unwrap();
    assert_eq!(s.model.view_name(id), Some("Model"));
    assert_eq!(s.model.default_3d(), None, "the name is no longer {3D}");
    assert_eq!(s.history().len(), before + 1);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.default_3d(), None);
    assert_eq!(s.history().len(), before);
    // Another view can't take the reserved names.
    let plan = s.model.levels().first().map(|(id, _)| *id).unwrap();
    assert!(s.execute("rename", &json!({"id": plan, "rename": "3D"})).is_err());
}

#[test]
fn a_family_and_its_type_are_renamed_through_the_commands_that_own_them() {
    let mut s = Session::new();
    s.execute("family", &json!({"name":"Table","types":{"A":{}},"forms":[{"profile":[[0,0],[100,0],[100,100],[0,100]],"to":100}]}))
        .unwrap();
    let reply = s.execute("rename", &json!({"family": "Table", "rename": "Desk"})).unwrap();
    assert_eq!((reply["kind"].as_str(), reply["from"].as_str(), reply["to"].as_str()), (Some("family"), Some("Table"), Some("Desk")));
    assert!(s.model.families.contains_key("Desk") && !s.model.families.contains_key("Table"));
    let reply = s.execute("rename", &json!({"family": "Desk", "type": "A", "rename": "B"})).unwrap();
    assert_eq!(reply["kind"], "type");
    assert!(s.model.families["Desk"].types.contains_key("B"));
    assert!(!s.model.families["Desk"].types.contains_key("A"));
    // A category's type goes through `rename_type` too.
    s.execute("type", &json!({"category":"wall", "name":"Thick", "thickness":300})).unwrap();
    s.execute("rename", &json!({"category":"wall", "type":"Thick", "rename":"Very Thick"})).unwrap();
    assert!(s.model.type_names("wall").contains(&"Very Thick"));
}

#[test]
fn renaming_a_view_onto_another_name_is_refused_and_the_project_is_left_as_it_was() {
    let mut s = Session::new();
    let plan = s.execute("duplicate_view", &json!({"view": "Level 1", "name": "Second Plan"})).unwrap();
    let view = plan["id"].as_u64().unwrap();
    let after = s.model.clone();
    assert!(s.execute("rename", &json!({"id": view, "rename": "Level 2"})).is_err(), "a level's name");
    assert_eq!(s.model, after);
}

#[test]
fn a_levels_own_plan_is_a_view_of_its_own_and_the_level_keeps_its_name() {
    let mut s = Session::new();
    let one = level(&s, "Level 1");
    let before = s.history().len();
    let reply = s.execute("rename", &json!({"level": one, "plan": "floor", "rename": "Ground"})).unwrap();
    assert_eq!(reply["kind"], "view");
    assert_eq!(reply["created"], json!(true));
    let id = reply["id"].as_u64().unwrap();
    let Some(Element::PlanView(v)) = s.model.elements.get(id) else { panic!("a plan view was made") };
    assert!(v.primary && !v.ceiling);
    assert_eq!(v.level, s.model.guid(one));
    assert_eq!(v.parent, Some(s.model.guid(one)), "a dependent of its level, so the level's annotation shows");
    assert_eq!(s.model.level(one).unwrap().name, "Level 1", "the level's name is not the view's");
    assert_eq!(s.model.view_name(one), Some("Ground"));
    assert_eq!(s.model.view_name(id), Some("Ground"));
    assert_eq!(s.model.primary_plan(one, false), Some(id));
    assert!(s.model.plan_views(one).is_empty(), "the level's own plan is not listed as one more view");
    assert_eq!(s.history().len(), before + 1);
    // The ceiling plan has a name of its own too, and the floor plan keeps its own.
    let rail = s.execute("rename", &json!({"level": one, "plan": "ceiling", "rename": "RCP"})).unwrap();
    let rail = rail["id"].as_u64().unwrap();
    assert_eq!(s.model.ceiling_view_name(one), Some("RCP"));
    assert_eq!(s.model.view_name(one), Some("Ground"));
    assert_eq!(s.model.view_name(rail), Some("RCP"));
    // A primary plan may take its own level's name; another level's name is still refused.
    s.execute("rename", &json!({"id": id, "rename": "Level 1"})).unwrap();
    assert_eq!(s.model.view_name(one), Some("Level 1"));
    assert!(s.execute("rename", &json!({"id": id, "rename": "Level 2"})).is_err());
    // Undo goes back name by name, then takes each primary view away as it was made.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.view_name(one), Some("Ground"));
    assert_eq!(s.model.ceiling_view_name(one), Some("RCP"));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.ceiling_view_name(one), Some("Level 1"));
    assert_eq!(s.model.view_name(one), Some("Ground"));
    s.execute("undo", &json!({})).unwrap();
    assert!(s.model.primary_plan(one, false).is_none());
    assert_eq!(s.model.view_name(one), Some("Level 1"));
}
