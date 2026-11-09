//! Family documents as tabs (#311), and what finishes later than it started: a file dialog answered, a "Family Already
//! Exists" question answered. Each finishes in the document that started it, whichever tab is active by then, and does
//! nothing to any other document when that one was closed meanwhile.
use super::*;
use crate::editor::tabs::{DocId, Then};

const EXISTS: &str = "Family Already Exists";
const VERSION: &str = "Overwrite the existing version";
const VALUES: &str = "Overwrite the existing version and its parameter values";

/// Two families the project has, Box and Crate.
const FAMILIES: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[0, 0], [500, 0], [500, 300], [0, 300]], "to": 400}]}
family {"name": "Crate", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[0, 0], [800, 0], [800, 600], [0, 600]], "to": 600}]}"#;

/// Box and Crate open as documents, Box's tab the active one; their ids.
fn two_documents() -> (Harness<'static, BimApp>, DocId, DocId) {
    let mut h = app(&format!("{BOX}\n{FAMILIES}"));
    h.set_size(vec2(1600.0, 1000.0));
    h.run();
    h.state_mut().edit_family("Box");
    h.run();
    h.state_mut().edit_family("Crate");
    h.run();
    let box_id = h.state().doc_of_family("Box").expect("Box open");
    let crate_id = h.state().doc_of_family("Crate").expect("Crate open");
    press(&mut h, "Box - {3D}");
    assert_eq!(h.state().active_doc(), Some(box_id));
    (h, box_id, crate_id)
}

/// A family file of a stool `width` wide, named Stool, at `dir/file`.
fn stool(dir: &std::path::Path, file: &str, width: u32) -> std::path::PathBuf {
    let path = dir.join(file);
    let family = json!({"name": "Stool", "category": "generic", "parameters": [{"name": "W", "value": width}], "types": {"S": {}},
        "forms": [{"profile": [[0, 0], ["W", 0], ["W", 300], [0, 300]], "to": 450}]});
    std::fs::write(&path, family.to_string()).unwrap();
    path
}

/// The file dialog begun last, answered with `path`.
fn answer(h: &mut Harness<BimApp>, path: &std::path::Path) {
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a dialog was begun");
    tx.send(Some(vec![path.to_path_buf()])).unwrap();
    h.run();
}

fn nested_names(h: &Harness<BimApp>, id: DocId) -> Vec<String> {
    h.state().family_of(id).expect("open").nested.iter().map(|n| n.name.clone()).collect()
}

fn unsaved(h: &Harness<BimApp>, id: DocId) -> bool {
    h.state().family_of(id).expect("open").unsaved()
}

/// The family document's name typed in Properties.
fn rename(h: &mut Harness<BimApp>, name: &str) {
    type_into(h, "Family name", name);
    key(h, Key::Enter);
}

#[test]
fn load_family_answered_after_a_tab_switch_nests_into_the_document_that_asked() {
    let dir = tempfile::tempdir().unwrap();
    let file = stool(dir.path(), "Stool.json", 400);
    let (mut h, box_id, crate_id) = two_documents();
    press(&mut h, "Insert");
    press(&mut h, "Load Family");
    press(&mut h, "Crate - {3D}");
    assert_eq!(h.state().active_doc(), Some(crate_id));
    answer(&mut h, &file);
    assert_eq!(nested_names(&h, box_id), ["Stool"], "Box asked for it");
    assert!(nested_names(&h, crate_id).is_empty(), "Crate is as it was");
    assert!(!unsaved(&h, crate_id));
    assert_eq!(h.state().active_doc(), Some(crate_id), "no tab brought forward to find it");
}

#[test]
fn save_family_answered_after_a_tab_switch_marks_the_document_that_saved_clean() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, box_id, crate_id) = two_documents();
    rename(&mut h, "Box S");
    assert!(unsaved(&h, box_id));
    press(&mut h, "Insert");
    press(&mut h, "Save Family");
    press(&mut h, "Crate - {3D}");
    let file = dir.path().join("Box S.json");
    answer(&mut h, &file);
    assert!(file.exists(), "Box's file written");
    assert!(!unsaved(&h, box_id), "Box is saved");
    assert!(!unsaved(&h, crate_id), "Crate's clean baseline is its own");
    assert_eq!(h.state().family_of(crate_id).unwrap().title(), "Crate");
    // Crate changed afterwards is unsaved, as against its own baseline.
    rename(&mut h, "Crate S");
    assert!(unsaved(&h, crate_id));
}

#[test]
fn save_family_with_no_tab_switch_marks_the_document_clean() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, box_id, crate_id) = two_documents();
    rename(&mut h, "Box T");
    press(&mut h, "Insert");
    press(&mut h, "Save Family");
    let file = dir.path().join("Box T.json");
    answer(&mut h, &file);
    assert!(file.exists());
    assert!(!unsaved(&h, box_id));
    assert!(!unsaved(&h, crate_id));
    assert_eq!(h.state().active_doc(), Some(box_id));
}

#[test]
fn family_already_exists_answered_after_a_tab_switch_nests_into_the_document_that_asked() {
    let dir = tempfile::tempdir().unwrap();
    let (first, second) = (stool(dir.path(), "Stool.json", 400), stool(dir.path(), "Stool 2.json", 900));
    let (mut h, box_id, crate_id) = two_documents();
    h.state_mut().nest_family_file(box_id, &first);
    h.run();
    assert_eq!(nested_names(&h, box_id), ["Stool"]);
    // Loaded again, defined otherwise: Box asks; Crate's tab comes forward before the answer.
    press(&mut h, "Insert");
    press(&mut h, "Load Family");
    answer(&mut h, &second);
    assert!(h.query_by_label(EXISTS).is_some(), "Box asks");
    h.state_mut().activate_doc(crate_id);
    h.run();
    press(&mut h, VALUES);
    assert!(h.query_by_label(EXISTS).is_none());
    let width = |h: &Harness<BimApp>| {
        let ed = h.state().family_of(box_id).unwrap();
        ed.nested.iter().find(|n| n.name == "Stool").map(|n| n.family.parameters[0].value.clone())
    };
    assert_eq!(width(&h), Some(json!(900)), "the new Stool, with its values, is Box's");
    assert!(nested_names(&h, crate_id).is_empty(), "Crate is as it was");
    assert_eq!(h.state().active_doc(), Some(crate_id));
}

#[test]
fn family_already_exists_answered_after_a_tab_switch_loads_the_document_that_asked() {
    let (mut h, box_id, crate_id) = two_documents();
    // A parameter added: the project's Box is defined otherwise.
    press(&mut h, "Family Types");
    press(&mut h, "New Parameter");
    type_into(&mut h, "Name:", "Depth");
    press_near(&mut h, "OK", "Name:");
    press(&mut h, "OK");
    assert!(unsaved(&h, box_id));
    press(&mut h, "Load into Project");
    assert!(h.query_by_label(EXISTS).is_some(), "Box asks");
    h.state_mut().activate_doc(crate_id);
    h.run();
    press(&mut h, VERSION);
    let m = &h.state().session.model;
    assert!(m.families["Box"].parameters.iter().any(|p| p.name == "Depth"), "Box's draft loaded");
    assert!(!unsaved(&h, box_id), "Box has nothing left to load");
    assert_eq!(h.state().doc_of_family("Crate"), Some(crate_id), "Crate is still the project's Crate's");
    assert_eq!(h.state().doc_of_family("Box"), Some(box_id));
    assert!(!unsaved(&h, crate_id));
    assert_eq!(h.state().active_doc(), Some(crate_id));
}

#[test]
fn a_document_closed_before_the_answer_leaves_the_other_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let (first, second) = (stool(dir.path(), "Stool.json", 400), stool(dir.path(), "Stool 2.json", 900));
    // Load Family's dialog answered once Box is closed.
    let (mut h, box_id, crate_id) = two_documents();
    press(&mut h, "Insert");
    press(&mut h, "Load Family");
    h.state_mut().ask_close_family(box_id, Then::Nothing);
    h.run();
    assert!(h.state().doc(box_id).is_none(), "Box closed");
    press(&mut h, "Crate - {3D}");
    assert_eq!(h.state().active_doc(), Some(crate_id));
    answer(&mut h, &first);
    assert!(nested_names(&h, crate_id).is_empty(), "nothing loaded into Crate");
    assert!(!unsaved(&h, crate_id));
    assert!(h.state().status.error, "the status bar says why");
    // Save Family's dialog answered once Box is closed: the file is written, Crate untouched.
    let (mut h, box_id, crate_id) = two_documents();
    rename(&mut h, "Box U");
    press(&mut h, "Insert");
    press(&mut h, "Save Family");
    h.state_mut().close_family_document(box_id);
    h.run();
    press(&mut h, "Crate - {3D}");
    let file = dir.path().join("Box U.json");
    answer(&mut h, &file);
    assert!(file.exists());
    assert!(!unsaved(&h, crate_id));
    // A question pending when Box closes: answered, it loads nothing anywhere.
    let (mut h, box_id, crate_id) = two_documents();
    h.state_mut().nest_family_file(box_id, &first);
    h.run();
    press(&mut h, "Insert");
    press(&mut h, "Load Family");
    answer(&mut h, &second);
    assert!(h.query_by_label(EXISTS).is_some());
    h.state_mut().close_family_document(box_id);
    h.state_mut().activate_doc(crate_id);
    h.run();
    assert!(h.query_by_label(EXISTS).is_none(), "the question goes with the document that asked");
    assert!(h.state().status.error, "the status bar says why");
    assert!(nested_names(&h, crate_id).is_empty(), "nothing loaded into Crate");
    assert!(!unsaved(&h, crate_id));
}
