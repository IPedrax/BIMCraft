//! New Family and Open Family, as Revit's File > New > Family and File > Open > Family, and the start page's Families: a
//! family's document opened from each, with the project behind it left as it was; and Edit Family on the contextual tab's Mode
//! panel, as Revit's Modify | <Category> and Modify | Place Component have it. Manage has neither.
use super::*;

/// The system's file dialog the last press began, answered with `path`.
fn answer(h: &mut Harness<BimApp>, path: &std::path::Path) {
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a dialog was begun");
    tx.send(Some(vec![path.to_path_buf()])).unwrap();
    h.run();
}

/// A family file, as Save Family writes one: Stool, a furniture family of one box, made in a session of its own.
fn stool(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("Stool.json");
    let mut s = Session::new();
    s.execute(
        "family",
        &json!({"name": "Stool", "category": "furniture", "types": {"Low": {}},
                "forms": [{"profile": [[0, 0], [400, 0], [400, 400], [0, 400]], "to": 450}]}),
    )
    .unwrap();
    s.execute("family", &json!({"name": "Stool", "save": path.to_string_lossy()})).unwrap();
    path
}

/// The family document's name, as its title has it.
fn open_family(h: &Harness<BimApp>) -> Option<String> {
    h.state().active_family().map(|ed| ed.title().to_string())
}

/// The app on its start page, as the window opens with no project.
fn home() -> Harness<'static, BimApp> {
    let mut h = app("");
    h.state_mut().show_start_page();
    h.run();
    h
}

#[test]
fn the_start_pages_new_family_picks_a_category_and_closing_it_goes_back_to_the_page() {
    let mut h = home();
    // Headings, in capitals as the page sets them.
    h.get_by_label("MODELS");
    h.get_by_label("FAMILIES");
    let before = h.state().session.model.to_json().unwrap();
    press(&mut h, "New Family");
    press(&mut h, "Casework");
    assert!(!h.state().home.shown, "the family document has the window");
    assert_eq!(h.state().active_family().unwrap().draft_family().unwrap().category, bimcraft_core::family::Category::Casework);
    press(&mut h, "Close Family");
    assert!(h.state().active_family().is_none());
    assert!(h.state().home.shown, "with no project behind it, the start page comes back");
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
}

#[test]
fn the_start_pages_open_family_opens_a_family_file_and_a_project_behind_the_page_stays() {
    let dir = tempfile::tempdir().unwrap();
    let file = stool(dir.path());
    let mut h = app(BOX);
    let before = h.state().session.model.to_json().unwrap();
    // The page brought up over a project.
    press(&mut h, "Home");
    assert!(h.state().home.shown && h.state().home.over);
    press(&mut h, "Open Family…");
    answer(&mut h, &file);
    assert_eq!(open_family(&h).as_deref(), Some("Stool"));
    assert!(!h.state().home.shown);
    assert_eq!(h.state().active_family().unwrap().draft_family().unwrap().forms.len(), 1);
    press(&mut h, "Close Family");
    assert!(!h.state().home.shown, "the project behind the page is on screen again");
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
}

#[test]
fn file_new_family_asks_for_a_category_over_the_project() {
    let mut h = app(BOX);
    file_menu(&mut h, "New", "Family…");
    assert!(h.state().new_family_dialog.is_some(), "New Family asks for its category");
    // Cancel opens nothing.
    press_near(&mut h, "Cancel", "Category:");
    assert!(h.state().new_family_dialog.is_none() && h.state().active_family().is_none());
    new_family_of(&mut h, "Lighting");
    let ed = h.state().active_family().expect("the family document is open");
    assert_eq!(ed.draft_family().unwrap().category, bimcraft_core::family::Category::Lighting);
    press(&mut h, "Close Family");
    assert!(!h.state().home.shown, "the project is still open");
    assert_eq!(walls(&h).len(), 4);
}

#[test]
fn file_open_family_opens_a_family_file_that_load_into_project_loads() {
    let dir = tempfile::tempdir().unwrap();
    let file = stool(dir.path());
    let mut h = app(BOX);
    // Wide enough for the family document's Family Editor panel to show its tools rather than fold them.
    h.set_size(vec2(1600.0, 800.0));
    assert!(!h.state().session.model.families.contains_key("Stool"));
    file_menu(&mut h, "Open", "Family…");
    answer(&mut h, &file);
    assert_eq!(open_family(&h).as_deref(), Some("Stool"));
    assert!(!h.state().session.model.families.contains_key("Stool"), "opened, not loaded");
    press(&mut h, "Load into Project and Close");
    assert!(h.state().active_family().is_none(), "{}", h.state().status.text);
    assert_eq!(h.state().session.model.families["Stool"].types.keys().collect::<Vec<_>>(), ["Low"]);
}

#[test]
fn a_file_that_isnt_a_family_opens_nothing_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.json");
    std::fs::write(&file, "[1, 2, 3]").unwrap();
    let mut h = app(BOX);
    file_menu(&mut h, "Open", "Family…");
    answer(&mut h, &file);
    assert!(h.state().active_family().is_none());
    assert!(h.state().status.error && h.state().status.text.contains("isn't a family"), "{}", h.state().status.text);
}

#[test]
fn manage_has_no_new_family() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    assert!(h.query_by_label("New Family").is_none(), "it is on the start page and in File > New");
}

#[test]
fn edit_family_is_on_the_contextual_tab_of_a_family_instance_and_of_place_component_not_on_manage() {
    let mut h = app(&format!("{BOX}\ncomponent {{\"family\": \"Table\", \"type\": \"1600 x 900\", \"at\": [3000, 3000]}}"));
    h.set_size(vec2(1800.0, 900.0));
    press(&mut h, "Manage");
    assert!(h.query_by_label("Edit Family").is_none(), "not on Manage");
    // A wall selected: its tab has no Edit Family.
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().tab, Tab::Context);
    assert!(h.query_by_label("Edit Family").is_none(), "a wall is of no family");
    key(&mut h, Key::Escape);
    // The table selected: Modify | Furniture, its Mode panel's Edit Family.
    click(&mut h, P2(3800.0, 3000.0));
    assert_eq!(h.state().tab, Tab::Context);
    assert_eq!(h.state().context().as_deref(), Some("Modify | Furniture"));
    // Its Mode panel at the tab's right end, as Revit's: past Geometry's Paint and Remove Paint.
    let right = |h: &Harness<BimApp>, l: &str| h.get_by_label(l).rect().left();
    assert!(right(&h, "Edit Family") > right(&h, "Remove Paint"), "Mode is last");
    press(&mut h, "Edit Family");
    assert_eq!(open_family(&h).as_deref(), Some("Table"));
    press(&mut h, "Close Family");
    key(&mut h, Key::Escape);
    // Placing a component: Modify | Place Component has Load Family and Edit Family, of the family being placed.
    h.state_mut().set_tool(Tool::Component);
    h.run();
    let placing = h.state().placing().map(|(f, _)| f).expect("a family to place");
    assert!(h.query_by_label("Load Family").is_some());
    press(&mut h, "Edit Family");
    assert_eq!(open_family(&h), Some(placing));
}
