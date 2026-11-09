//! Family documents as tabs beside the project's (#311): any number open at once, each with its own draft, history and
//! selection; the project's ribbon, Properties and Browser on a project tab, the family's on its tabs; and the close rules.

use super::*;

/// A box family the project has, and a second one.
const FAMILIES: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[0, 0], [500, 0], [500, 300], [0, 300]], "to": 400}]}
family {"name": "Crate", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[0, 0], [800, 0], [800, 600], [0, 600]], "to": 600}]}"#;

fn project() -> Harness<'static, BimApp> {
    let mut h = app(&format!("{BOX}\n{FAMILIES}"));
    h.set_size(vec2(1600.0, 1000.0));
    h.run();
    h
}

fn chord(h: &mut Harness<BimApp>, modifiers: Modifiers, k: Key) {
    h.key_press_modifiers(modifiers, k);
    h.run();
}

fn active_title(h: &Harness<BimApp>) -> Option<String> {
    h.state().active_family().map(|ed| ed.title().to_string())
}

/// The family document's name typed in Properties.
fn rename(h: &mut Harness<BimApp>, name: &str) {
    type_into(h, "Family name", name);
    key(h, Key::Enter);
}

#[test]
fn edit_family_opens_its_3d_as_a_tab_next_to_the_projects_and_a_second_edit_brings_it_forward() {
    let mut h = project();
    let project_tabs = h.state().views.clone();
    h.state_mut().edit_family("Box");
    h.run();
    assert!(h.query_by_label("Box - {3D}").is_some(), "the family's tab");
    assert!(h.query_by_label("Level 1").is_some(), "beside the project's");
    assert_eq!(h.state().views[..project_tabs.len()], project_tabs[..], "after the project's tabs");
    // A second family, and the first again: brought forward, not opened twice.
    h.state_mut().edit_family("Crate");
    h.run();
    assert_eq!(active_title(&h).as_deref(), Some("Crate"));
    assert!(h.query_by_label("Box - {3D}").is_some() && h.query_by_label("Crate - {3D}").is_some());
    h.state_mut().edit_family("Box");
    h.run();
    assert_eq!(active_title(&h).as_deref(), Some("Box"));
    assert_eq!(h.state().families.len(), 2, "a family is never open twice");
}

#[test]
fn a_project_tab_shows_the_projects_ribbon_properties_and_browser_and_the_family_tab_comes_back_as_it_was() {
    let mut h = project();
    h.state_mut().edit_family("Box");
    h.run();
    rename(&mut h, "Box 2");
    assert!(h.query_by_label("Load into Project").is_some(), "the family's ribbon");
    press(&mut h, "Level 1");
    assert!(h.state().active_family().is_none(), "the project's tab");
    assert!(h.query_by_label("Architecture").is_some(), "the project's ribbon");
    assert!(h.query_by_label("Load into Project").is_none());
    assert!(h.query_by_label("Floor Plans").is_some(), "the project's browser");
    press(&mut h, "Box 2 - {3D}");
    assert_eq!(active_title(&h).as_deref(), Some("Box 2"), "the draft kept");
    assert!(h.state().active_family().unwrap().unsaved());
    assert!(h.query_by_label("Load into Project").is_some());
}

#[test]
fn two_documents_undo_their_own_drafts_and_never_the_project() {
    let mut h = project();
    let project = h.state().session.model.to_json().unwrap();
    h.state_mut().edit_family("Box");
    h.run();
    rename(&mut h, "Box A");
    h.state_mut().edit_family("Crate");
    h.run();
    rename(&mut h, "Crate A");
    press(&mut h, "Undo");
    assert_eq!(active_title(&h).as_deref(), Some("Crate"), "Crate's own step undone");
    press(&mut h, "Box A - {3D}");
    assert_eq!(active_title(&h).as_deref(), Some("Box A"), "Box's untouched");
    press(&mut h, "Undo");
    assert_eq!(active_title(&h).as_deref(), Some("Box"));
    assert_eq!(h.state().session.model.to_json().unwrap(), project, "the project is as it was");
}

#[test]
fn closing_a_documents_last_tab_with_changes_asks_and_cancel_keeps_it() {
    let mut h = project();
    h.state_mut().edit_family("Box");
    h.run();
    // No changes: it closes at once, back on the project's tab.
    press(&mut h, "Close Family");
    assert!(h.state().families.is_empty() && h.state().active_family().is_none());
    assert!(h.query_by_label("Architecture").is_some());
    // Changed: asked.
    h.state_mut().edit_family("Box");
    h.run();
    rename(&mut h, "Box B");
    press(&mut h, "Close Family");
    assert!(h.query_by_label("Save changes to Box B?").is_some());
    press(&mut h, "Cancel");
    assert_eq!(active_title(&h).as_deref(), Some("Box B"), "Cancel keeps it open");
    // Close Without Loading: closed, the project's family as it was.
    press(&mut h, "Close Family");
    press(&mut h, "Close Without Loading");
    assert!(h.state().families.is_empty());
    assert!(!h.state().session.model.families.contains_key("Box B"));
    // Load into Project and Close: loaded (renamed), then closed.
    h.state_mut().edit_family("Box");
    h.run();
    rename(&mut h, "Box C");
    press(&mut h, "Close Family");
    press_near(&mut h, "Load into Project and Close", "Close Without Loading");
    assert!(h.state().families.is_empty(), "closed once it went in");
    assert!(h.state().session.model.families.contains_key("Box C"), "loaded");
}

#[test]
fn load_into_project_keeps_the_tab_open_and_the_document_clean() {
    let mut h = project();
    h.state_mut().edit_family("Box");
    h.run();
    rename(&mut h, "Box D");
    press(&mut h, "Load into Project");
    assert_eq!(active_title(&h).as_deref(), Some("Box D"), "still open");
    assert!(!h.state().active_family().unwrap().unsaved(), "nothing left to load");
    assert!(h.state().session.model.families.contains_key("Box D"));
}

#[test]
fn a_family_view_opened_from_the_browser_is_a_tab_of_its_own() {
    let mut h = project();
    h.state_mut().edit_family("Box");
    h.run();
    super::family_views::view(&mut h, Some(bimcraft_core::family::views::Which::Plan));
    assert!(h.query_by_label("Box - Floor Plan: Ref. Level").is_some());
    assert!(h.query_by_label("Box - {3D}").is_some(), "{{3D}}'s tab stays");
    // Closing a tab that is not the document's last only closes it.
    press(&mut h, "Close Box - Floor Plan: Ref. Level");
    assert_eq!(h.state().families.len(), 1);
    assert_eq!(h.state().active_family().unwrap().views.shown, None, "back on {{3D}}");
}

#[test]
fn a_new_project_asks_about_the_open_documents_first() {
    let mut h = project();
    // The project changed too.
    h.state_mut().run("wall", json!({"points": [[0, 8000], [4000, 8000]]}));
    assert!(h.state().dirty());
    h.state_mut().edit_family("Box");
    h.run();
    rename(&mut h, "Box E");
    // From the project's tab, where its keys work.
    press(&mut h, "Level 1");
    chord(&mut h, Modifiers::COMMAND, Key::N);
    assert!(h.query_by_label("Save changes to Box E?").is_some(), "the document first");
    press(&mut h, "Close Without Loading");
    assert!(h.state().families.is_empty());
    // Then the project's own question.
    assert!(h.query_by_label(&format!("Save changes to {}?", h.state().session.model.name)).is_some(), "then the project");
}

#[test]
fn new_families_are_named_family1_family2_and_open_side_by_side() {
    let mut h = project();
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    h.state_mut().new_family(bimcraft_core::family::Category::Furniture);
    h.run();
    assert!(h.query_by_label("Family1 - {3D}").is_some() && h.query_by_label("Family2 - {3D}").is_some());
    assert_eq!(active_title(&h).as_deref(), Some("Family2"));
}
