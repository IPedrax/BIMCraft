//! Revit's "Family Already Exists": loading a family the project has otherwise asks whether to overwrite it, and how; the
//! answer is only the `family` command's `overwrite`.
use super::*;
use crate::editor::exists::{Overwrite, Shared};

const EXISTS: &str = "Family Already Exists";
const VERSION: &str = "Overwrite the existing version";
const VALUES: &str = "Overwrite the existing version and its parameter values";
const SHARED: &str = "Shared Family Already Loaded";

/// A project with the family Reload: Width 100 by default, its type A 150.
fn project() -> Harness<'static, BimApp> {
    let mut h = app("");
    h.set_size(vec2(1600., 1000.));
    let made = h.state_mut().run(
        "family",
        json!({"name":"Reload","parameters":[{"name":"Width","kind":"length","value":100}],"types":{"A":{"Width":150}},
               "forms":[{"profile":[[0,0],["Width",0],["Width",20],[0,20]],"to":50}]}),
    );
    assert!(made.is_some());
    h
}

/// Reload opened as a document and changed both ways: a parameter added (its definition) and type A's Width 200 (a value).
fn changed() -> Harness<'static, BimApp> {
    let mut h = project();
    h.state_mut().edit_family("Reload");
    h.run();
    press(&mut h, "Family Types");
    press(&mut h, "New Parameter");
    type_into(&mut h, "Name:", "Parameter 1");
    press_near(&mut h, "OK", "Name:");
    type_into(&mut h, "Parameter Value Width", "200");
    press(&mut h, "OK");
    h
}

fn reload<'a>(h: &'a Harness<BimApp>) -> &'a bimcraft_core::family::Family {
    &h.state().session.model.families["Reload"]
}

fn steps(h: &Harness<BimApp>) -> usize {
    h.state().session.history().len()
}

fn asked(h: &Harness<BimApp>, title: &str) -> bool {
    h.query_by_label(title).is_some()
}

#[test]
fn a_changed_family_asks_and_overwrite_the_existing_version_keeps_the_projects_values() {
    let mut h = changed();
    let (before, was) = (steps(&h), h.state().session.model.to_json().unwrap());
    press(&mut h, "Load into Project");
    assert!(asked(&h, EXISTS), "the project has Reload otherwise: the dialog asks");
    assert!(h.query_all_by_label_contains("Reload").count() > 0, "it names the family");
    assert_eq!(steps(&h), before, "nothing is loaded while it asks");
    press(&mut h, VERSION);
    assert!(!asked(&h, EXISTS));
    let f = reload(&h);
    assert!(f.parameters.iter().any(|p| p.name == "Parameter 1"), "the new definition");
    assert_eq!(f.parameters[0].value, json!(100), "the project's Width default kept");
    assert_eq!(f.types["A"]["Width"], json!(150), "and its type's value");
    assert_eq!(steps(&h), before + 1, "one undo step");
    assert_eq!(h.state().family_decided.as_ref().and_then(|d| d.overwrite), Some(Overwrite::Version));
    assert!(h.state().active_family().is_some(), "Load into Project leaves the document open");
    // Undone, the project's family is as it was.
    press(&mut h, "Close Family");
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    assert_eq!(h.state().session.model.to_json().unwrap(), was);
}

#[test]
fn overwrite_the_existing_version_and_its_parameter_values_takes_the_familys_values() {
    let mut h = changed();
    let before = steps(&h);
    press(&mut h, "Load into Project");
    press(&mut h, VALUES);
    let f = reload(&h);
    assert!(f.parameters.iter().any(|p| p.name == "Parameter 1"));
    assert_eq!(f.types["A"]["Width"], json!(200.0), "the family's type A Width");
    assert!(f.types.contains_key("A"));
    assert_eq!(steps(&h), before + 1);
    assert_eq!(h.state().family_decided.as_ref().and_then(|d| d.overwrite), Some(Overwrite::VersionAndValues));
    assert!(!h.state().active_family().unwrap().unsaved(), "loaded: nothing unsaved");
}

#[test]
fn cancel_keeps_the_projects_family_as_it_is() {
    let mut h = changed();
    let (before, was) = (steps(&h), h.state().session.model.to_json().unwrap());
    press(&mut h, "Load into Project");
    press(&mut h, "Cancel");
    assert!(!asked(&h, EXISTS));
    assert_eq!(h.state().session.model.to_json().unwrap(), was, "the project keeps its version");
    assert_eq!(steps(&h), before, "no undo step");
    assert!(h.state().active_family().unwrap().unsaved(), "the document still has what it didn't load");
    // Escape is Cancel too.
    press(&mut h, "Load into Project");
    assert!(asked(&h, EXISTS));
    key(&mut h, Key::Escape);
    assert!(!asked(&h, EXISTS));
    assert_eq!(h.state().session.model.to_json().unwrap(), was);
}

#[test]
fn an_identical_family_loads_as_a_no_op_without_asking() {
    let mut h = project();
    let (before, was) = (steps(&h), h.state().session.model.to_json().unwrap());
    h.state_mut().edit_family("Reload");
    h.run();
    press(&mut h, "Load into Project");
    assert!(!asked(&h, EXISTS), "nothing would change: nothing to ask");
    assert_eq!(steps(&h), before, "no undo step");
    assert_eq!(h.state().session.model.to_json().unwrap(), was);
    press(&mut h, "Load into Project and Close");
    assert!(h.state().active_family().is_none(), "the load went ahead, a no-op: the document closes");
    assert_eq!(steps(&h), before);
}

#[test]
fn a_new_family_loads_without_asking() {
    let mut h = project();
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    type_into(&mut h, "Family name", "Brand New");
    h.get_by_label("Close Family").focus();
    h.run();
    press(&mut h, "Load into Project");
    assert!(!asked(&h, EXISTS));
    assert!(h.state().session.model.families.contains_key("Brand New"));
    // A new document named as a family the project has asks, as a file of that name would.
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    type_into(&mut h, "Family name", "Reload");
    // Two documents open now, each its tab: the new one's, last.
    h.get_all_by_label("Close Family").last().unwrap().focus();
    h.run();
    press(&mut h, "Load into Project");
    assert!(asked(&h, EXISTS));
}

#[test]
fn load_into_project_and_close_closes_on_overwrite_and_stays_open_on_cancel() {
    let mut h = changed();
    press(&mut h, "Load into Project and Close");
    assert!(asked(&h, EXISTS));
    assert!(h.state().active_family().is_some(), "open while it asks");
    press(&mut h, "Cancel");
    assert!(h.state().active_family().is_some(), "Cancel: the document stays open");
    assert!(!reload(&h).parameters.iter().any(|p| p.name == "Parameter 1"));
    press(&mut h, "Load into Project and Close");
    press(&mut h, VERSION);
    assert!(h.state().active_family().is_none(), "it went in: the document closes");
    assert!(reload(&h).parameters.iter().any(|p| p.name == "Parameter 1"));
    assert!(h.query_by_label("Architecture").is_some(), "the project's ribbon is back");
}

#[test]
fn a_refused_load_stays_said_and_the_document_stays_open() {
    let mut h = project();
    h.state_mut().edit_family("Reload");
    h.run();
    // A formula naming a parameter the family hasn't (T9's refusal), in the draft as a file from before T9 may hold one: Family
    // Types refuses to take one typed.
    h.state_mut().active_family_mut().unwrap().set_formula("Width", "Nope + 1");
    h.run();
    let was = h.state().session.model.to_json().unwrap();
    // Asked first, as Revit asks before it reads the family; the overwrite is then refused, as a whole.
    press(&mut h, "Load into Project and Close");
    press(&mut h, VALUES);
    assert!(h.state().active_family().is_some(), "refused: the document stays open");
    assert!(h.state().active_family().unwrap().problem().is_some_and(|p| p.contains("Nope")), "the refusal is kept");
    assert!(h.query_all_by_label_contains("Nope").count() > 0, "and shown");
    assert_eq!(h.state().session.model.to_json().unwrap(), was);
}

#[test]
fn a_family_file_the_project_has_otherwise_asks_before_load_family_loads_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = project();
    let file = dir.path().join("Reload.json");
    let saved = h.state_mut().run("family", json!({"name":"Reload","save":file.to_string_lossy()}));
    assert!(saved.is_some());
    // The same file loads as a no-op, without asking.
    let before = steps(&h);
    h.state_mut().load_family_file(&file);
    h.run();
    assert!(!asked(&h, EXISTS));
    assert_eq!(steps(&h), before);
    // Changed: it asks, and the answer is the command's.
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    v["parameters"][0]["value"] = json!(300);
    v["types"]["B"] = json!({});
    std::fs::write(&file, v.to_string()).unwrap();
    h.state_mut().load_family_file(&file);
    h.run();
    assert!(asked(&h, EXISTS));
    press(&mut h, VALUES);
    assert_eq!(reload(&h).parameters[0].value, json!(300));
    assert!(reload(&h).types.contains_key("B"));
    assert_eq!(h.state().component.as_ref().map(|c| c.0.as_str()), Some("Reload"), "the Component tool takes it");
}

#[test]
fn a_shared_nested_family_asks_after_the_family_and_either_choice_loads() {
    // The project has a shared Hinge, 70 wide; Reload's document loads a door nesting a shared Hinge 50 wide (T6a's real
    // trigger: the core's dry run finds it).
    let dir = tempfile::tempdir().unwrap();
    let door = dir.path().join("Door.json");
    let hinge = |w: u32| {
        json!({"category":"generic","shared":true,"parameters":[{"name":"W","value":w}],"types":{"H":{}},
                                "forms":[{"profile":[[0,0],["W",0],["W",10],[0,10]],"to":10}]})
    };
    let door_family = json!({"name":"Door","category":"generic","types":{"D":{}},"nested":[{"name":"Hinge","family":hinge(50)}],
                             "nested_instances":[{"family":"Hinge","type":"H"}]});
    std::fs::write(&door, door_family.to_string()).unwrap();
    for (choice, want, width) in
        [("Use the project's version", Shared::Project, 70.0), ("Overwrite it with the version in this family", Shared::Family, 50.0)]
    {
        let mut h = changed();
        let mut own = hinge(70);
        own["name"] = json!("Hinge");
        assert!(h.state_mut().run("family", json!({"definition": own})).is_some());
        // Loaded into the document: it has no Hinge, so nothing asks.
        h.state_mut().nest_family_file(&door);
        h.run();
        assert!(!asked(&h, EXISTS) && !asked(&h, SHARED));
        press(&mut h, "Load into Project and Close");
        press(&mut h, VERSION);
        assert!(asked(&h, SHARED), "then the nested family's question");
        assert!(h.query_all_by_label_contains("Hinge").count() > 0);
        assert!(!reload(&h).parameters.iter().any(|p| p.name == "Parameter 1"), "nothing loaded while it asks");
        press(&mut h, choice);
        assert!(!asked(&h, SHARED));
        let decided = h.state().family_decided.clone().unwrap();
        assert_eq!(decided.overwrite, Some(Overwrite::Version));
        assert_eq!(decided.shared, [("Hinge".to_string(), want)]);
        assert!(reload(&h).parameters.iter().any(|p| p.name == "Parameter 1"));
        assert!(h.state().active_family().is_none());
        // One Hinge across the project: the project's, and the one nested in Reload's door, as answered.
        let m = &h.state().session.model;
        assert_eq!(m.families["Hinge"].parameters[0].value.as_f64(), Some(width));
        let nested = reload(&h).nested.iter().find(|n| n.name == "Hinge").map(|n| &n.family);
        assert_eq!(nested, Some(&m.families["Hinge"]), "the shared Hinge Reload nests is the project's");
    }
    // Load Family from the file asks too, and Cancel there loads nothing.
    let mut h = project();
    let mut own = hinge(70);
    own["name"] = json!("Hinge");
    assert!(h.state_mut().run("family", json!({"definition": own})).is_some());
    let was = h.state().session.model.to_json().unwrap();
    h.state_mut().load_family_file(&door);
    h.run();
    assert!(!asked(&h, EXISTS) && asked(&h, SHARED));
    press(&mut h, "Cancel");
    assert!(h.state().family_decided.is_none());
    assert_eq!(h.state().session.model.to_json().unwrap(), was);
}
