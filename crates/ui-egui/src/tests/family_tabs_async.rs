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

/// Whether no file dialog is waiting for an answer.
fn no_dialog_begun() -> bool {
    crate::dialog::BEGUN.with(|b| b.borrow().is_empty())
}

/// The project saved to `dir/House.bimcraft`, then changed so it is unsaved; Box open as a document, renamed `name`. The
/// project's file and what it held when saved.
fn saved_project_and_a_changed_family(dir: &std::path::Path, name: &str) -> (Harness<'static, BimApp>, std::path::PathBuf, Vec<u8>) {
    let mut h = app(&format!("{BOX}\n{FAMILIES}"));
    h.set_size(vec2(1600.0, 1000.0));
    h.run();
    let project = dir.join("House.bimcraft");
    assert!(h.state_mut().save_to(&project));
    let held = std::fs::read(&project).unwrap();
    h.state_mut().run("wall", json!({"points": [[0, 8000], [4000, 8000]]}));
    assert!(h.state().dirty());
    h.state_mut().edit_family("Box");
    h.run();
    rename(&mut h, name);
    (h, project, held)
}

/// What a family file at `path` names its family.
fn name_in(path: &std::path::Path) -> String {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    v["name"].as_str().unwrap_or_default().to_string()
}

#[test]
fn save_on_a_family_tab_saves_the_family_and_never_the_project() {
    let dir = tempfile::tempdir().unwrap();
    // The Quick Access Toolbar's Save.
    let (mut h, project, held) = saved_project_and_a_changed_family(dir.path(), "Box Q");
    let doc = h.state().active_doc().unwrap();
    press(&mut h, "Save");
    let file = dir.path().join("Box Q.json");
    answer(&mut h, &file);
    assert_eq!(name_in(&file), "Box Q", "the family's file written");
    assert!(!unsaved(&h, doc), "the document is clean");
    assert_eq!(std::fs::read(&project).unwrap(), held, "the project's file as it was");
    assert!(h.state().dirty(), "the project still unsaved");
    // Saved once, it has a file: Save writes there without asking.
    rename(&mut h, "Box R");
    press(&mut h, "Save");
    assert!(no_dialog_begun(), "nothing asked");
    assert_eq!(name_in(&file), "Box R");
    assert!(!unsaved(&h, doc));
    assert_eq!(std::fs::read(&project).unwrap(), held);
    // File > Save Family: the same.
    rename(&mut h, "Box S");
    let keys = |h: &Harness<BimApp>, m| h.ctx.format_shortcut(&egui::KeyboardShortcut::new(m, Key::S));
    press(&mut h, "File");
    let item = format!("Save Family {}", keys(&h, Modifiers::COMMAND));
    press(&mut h, &item);
    assert!(no_dialog_begun());
    assert_eq!(name_in(&file), "Box S");
    assert!(!unsaved(&h, doc));
    // File > Save Family As…: asked, always.
    rename(&mut h, "Box T");
    press(&mut h, "File");
    let item = format!("Save Family As… {}", keys(&h, Modifiers::COMMAND | Modifiers::SHIFT));
    press(&mut h, &item);
    let other = dir.path().join("Box T.json");
    answer(&mut h, &other);
    assert_eq!(name_in(&other), "Box T");
    assert_eq!(name_in(&file), "Box S", "the first file left as it was");
    assert!(!unsaved(&h, doc));
    assert_eq!(std::fs::read(&project).unwrap(), held, "the project's file as it was");
    assert!(h.state().dirty(), "the project still unsaved");
}

#[test]
fn ctrl_s_on_a_family_tab_saves_the_family_and_never_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, project, held) = saved_project_and_a_changed_family(dir.path(), "Box K");
    let doc = h.state().active_doc().unwrap();
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run();
    let file = dir.path().join("Box K.json");
    answer(&mut h, &file);
    assert_eq!(name_in(&file), "Box K");
    assert!(!unsaved(&h, doc));
    assert_eq!(std::fs::read(&project).unwrap(), held);
    assert!(h.state().dirty());
    // Again, to the same file without asking.
    rename(&mut h, "Box L");
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run();
    assert!(no_dialog_begun());
    assert_eq!(name_in(&file), "Box L");
    assert!(!unsaved(&h, doc));
    // Ctrl+Shift+S asks.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    h.run();
    let other = dir.path().join("Box M.json");
    answer(&mut h, &other);
    assert_eq!(name_in(&other), "Box L");
    assert_eq!(std::fs::read(&project).unwrap(), held);
    assert!(h.state().dirty());
}

#[test]
fn a_family_opened_from_a_file_saves_there_without_asking() {
    let dir = tempfile::tempdir().unwrap();
    let file = stool(dir.path(), "Stool.json", 400);
    let mut h = app(BOX);
    h.set_size(vec2(1600.0, 1000.0));
    h.run();
    h.state_mut().open_family_file(&file);
    h.run();
    let doc = h.state().active_doc().expect("Stool open");
    rename(&mut h, "Stool B");
    assert!(unsaved(&h, doc));
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run();
    assert!(no_dialog_begun(), "nothing asked");
    assert_eq!(name_in(&file), "Stool B");
    assert!(!unsaved(&h, doc));
    assert!(h.state().session.path.is_none(), "the project never saved");
}

#[test]
fn save_on_a_project_tab_still_saves_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, project, held) = saved_project_and_a_changed_family(dir.path(), "Box P");
    let doc = h.state().active_doc().unwrap();
    press(&mut h, "Level 1");
    assert!(h.state().active_doc().is_none());
    press(&mut h, "Save");
    assert!(no_dialog_begun());
    assert_ne!(std::fs::read(&project).unwrap(), held, "the project's file written");
    assert!(!h.state().dirty());
    assert!(unsaved(&h, doc), "the family's draft left as it was");
    // And Ctrl+S.
    h.state_mut().run("wall", json!({"points": [[0, 9000], [4000, 9000]]}));
    assert!(h.state().dirty());
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run();
    assert!(no_dialog_begun());
    assert!(!h.state().dirty());
    assert!(unsaved(&h, doc));
}
