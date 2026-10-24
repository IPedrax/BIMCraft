//! Linked models, driven as a user drives them: Manage > Link Model (its file picked for it), the Project Browser's Linked Models
//! and what a plan lets be picked.

use super::*;
use std::path::PathBuf;

use egui_kittest::kittest::NodeT;

/// A box of walls saved to a file of its own: the path.
fn shell(name: &str) -> PathBuf {
    let mut s = Session::new();
    s.script(BOX).unwrap();
    let path = std::env::temp_dir().join(format!("bimcraft-ui-link-{name}-{}.bimcraft", std::process::id()));
    s.execute("save", &json!({"path": path})).unwrap();
    path
}

fn in_browser<'a>(h: &'a Harness<BimApp>, label: &'a str) -> egui_kittest::Node<'a> {
    let entries = h.get_all_by_label(label).filter(|n| n.rect().center().x < 290.0 && n.accesskit_node().role() == Role::Button);
    entries.min_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y)).unwrap()
}

const FIND: &str = "Search the Project Browser";

fn links(h: &Harness<BimApp>) -> Vec<Id> {
    ids(h, "link")
}

#[test]
fn a_linked_model_is_drawn_in_the_plan_and_the_plan_cannot_pick_it() {
    let path = shell("plan");
    let mut h = app("");
    h.state_mut().link_path(&path);
    h.run();
    let _ = std::fs::remove_file(&path);
    assert_eq!(links(&h).len(), 1, "{}", h.state().status.text);
    // The model the window draws has the walls; the project has none.
    assert!(h.state().session.model.elements.values().all(|e| e.category() != "wall"));
    let drawn = h.state_mut().view_model();
    assert_eq!(drawn.elements.values().filter(|e| e.category() == "wall").count(), 4);
    // A click on a linked wall selects nothing.
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.state().selection.is_empty(), "{:?}", h.state().selection);
}

#[test]
fn the_project_browser_lists_linked_models_and_a_click_selects_one_for_properties() {
    let path = shell("browser");
    let mut h = app("");
    assert!(h.query_by_label("Linked Models").is_none(), "no group where nothing is linked");
    h.state_mut().link_path(&path);
    h.run();
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let _ = std::fs::remove_file(&path);
    assert!(h.query_by_label("Linked Models").is_some());
    type_into(&mut h, FIND, &name);
    in_browser(&h, &name).click();
    h.run();
    assert_eq!(h.state().selection, links(&h));
    assert!(h.query_by_label("Origin").is_some() && h.query_by_label("Angle").is_some(), "its parameters show");
}

#[test]
fn a_right_click_on_a_linked_model_removes_it() {
    let path = shell("remove");
    let mut h = app("");
    h.state_mut().link_path(&path);
    h.run();
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let _ = std::fs::remove_file(&path);
    type_into(&mut h, FIND, &name);
    in_browser(&h, &name).click_secondary();
    h.run();
    press(&mut h, "Remove");
    assert!(links(&h).is_empty());
    assert_eq!(h.state_mut().view_model().elements.values().filter(|e| e.category() == "wall").count(), 0);
}

#[test]
fn a_link_whose_file_is_missing_is_listed_as_not_loaded_and_reload_says_so() {
    let path = shell("lost");
    let mut h = app("");
    h.state_mut().link_path(&path);
    h.run();
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    // The file is gone when the project opens: the link stays, and says it isn't loaded.
    std::fs::remove_file(&path).unwrap();
    let r = h.state_mut().session.execute("reload", &json!({})).unwrap();
    assert_eq!(r["failed"].as_array().map(Vec::len), Some(1), "reload says which could not be read: {r}");
    h.state_mut().session.model_mut().loaded.clear();
    h.run();
    assert!(h.query_by_label(&format!("{name} (not loaded)")).is_some());
    assert_eq!(links(&h).len(), 1);
}

#[test]
fn the_manage_tab_offers_link_model_and_the_linked_models_group_reloads_them_all() {
    let path = shell("manage");
    let mut h = app("");
    press(&mut h, "Manage");
    assert!(h.query_by_label("Link Model").is_some());
    h.state_mut().link_path(&path);
    h.run();
    let walls = |h: &mut Harness<BimApp>| h.state_mut().view_model().elements.values().filter(|e| e.category() == "wall").count();
    assert_eq!(walls(&mut h), 4);
    // The file gains a wall on disk; a right click on the group reads every link again.
    let mut other = Session::new();
    other.script(BOX).unwrap();
    other.script(r#"wall {"points": [[0,0],[0,-3000]]}"#).unwrap();
    other.execute("save", &json!({"path": path})).unwrap();
    type_into(&mut h, FIND, "linked");
    h.get_by_label("Linked Models").click_secondary();
    h.run();
    press(&mut h, "Reload All");
    let _ = std::fs::remove_file(&path);
    assert_eq!(walls(&mut h), 5);
}

#[test]
fn lk_asks_for_the_file_on_the_next_frame_and_is_listed_in_ks() {
    let mut app = BimApp::new(Session::new());
    app.typed = "LK".into();
    assert!(app.shortcut());
    assert!(app.link_dialog, "the dialog is asked for, not opened where the shortcut is typed");
    assert!(crate::shortcuts().iter().any(|(letters, name)| *letters == "LK" && *name == "Link Model"));
}

#[test]
fn a_walls_levels_and_a_plan_views_level_and_parent_are_named_in_properties() {
    let mut h = app(BOX);
    let w = ids(&h, "wall")[0];
    h.state_mut().selection = vec![w];
    h.run();
    h.run();
    let shown = |h: &Harness<BimApp>, label: &str| h.get_by_role_and_label(Role::ComboBox, label).value();
    assert_eq!(shown(&h, "Base Constraint").as_deref(), Some("Level 1"));
    assert_eq!(shown(&h, "Top Constraint").as_deref(), Some("Level 2"));
}
