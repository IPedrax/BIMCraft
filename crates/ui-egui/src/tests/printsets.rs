//! Print sets: views picked in the Project Browser with Ctrl and Shift and made a set from its right-click menu, the Print
//! Sets window (Revit's View/Sheet Set), and a set exported from File > Export….

use super::*;
use crate::browser::Pick;
use egui_kittest::kittest::NodeT;
use std::time::Duration;

const PROJECT: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
sheet {"name": "Plans"}
place {"sheet": "A101", "view": "Level 1"}"#;

fn in_browser<'a>(h: &'a Harness<BimApp>, label: &'a str) -> egui_kittest::Node<'a> {
    let entries = h.get_all_by_label(label).filter(|n| n.rect().center().x < 290.0 && n.accesskit_node().role() == Role::Button);
    entries.min_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y)).unwrap()
}

/// Clicks the browser's entry `label` with `mods`, scrolled to first: the browser shows only part of its tree.
fn click_in(h: &mut Harness<BimApp>, label: &str, mods: Modifiers) {
    in_browser(h, label).scroll_to_me();
    h.run();
    in_browser(h, label).click_modifiers(mods);
    h.run();
}

fn levels(h: &Harness<BimApp>) -> Vec<Id> {
    h.state().session.model.levels().iter().map(|(id, _)| *id).collect()
}

#[test]
fn ctrl_picks_views_in_the_browser_and_its_right_click_makes_them_a_print_set() {
    let mut h = app(PROJECT);
    let l = levels(&h);
    click_in(&mut h, "Level 1", Modifiers::NONE);
    assert_eq!(h.state().browser_picked, vec![Pick::View(Sheet::Plan(l[0]))], "a click picks one, and opens it");
    click_in(&mut h, "Level 2", Modifiers::COMMAND);
    assert_eq!(h.state().browser_picked, vec![Pick::View(Sheet::Plan(l[0])), Pick::View(Sheet::Plan(l[1]))]);
    assert_eq!(h.state().sheet, Sheet::Plan(l[0]), "Ctrl picks, it doesn't open");
    // Properties shows what the two share.
    assert!(h.query_by_label("Floor Plans (2)").is_some(), "the palette names the two");
    assert!(h.query_by_label("Cut Plane").is_some(), "and their view range");
    // Ctrl again takes one out.
    click_in(&mut h, "Level 2", Modifiers::COMMAND);
    assert_eq!(h.state().browser_picked.len(), 1);
    click_in(&mut h, "Level 2", Modifiers::COMMAND);
    in_browser(&h, "Level 2").click_secondary();
    h.run();
    press(&mut h, "Create Print Set of 2…");
    assert!(h.query_by_label("Create Print Set").is_some(), "the name is asked");
    type_into(&mut h, "Name", "Plans only");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    let set = m.print_sets.get("Plans only").expect("saved under its name");
    assert_eq!(set.views, vec![m.guid(l[0]), m.guid(l[1])]);
    assert!(m.library.id(&bimcraft_core::library::Kind::PrintSet, "Plans only").is_some(), "with an id of its own");
    assert!(h.state().name_set.is_none());
}

#[test]
fn shift_picks_a_run_and_mixed_kinds_are_multiple_categories() {
    let mut h = app(PROJECT);
    let l = levels(&h);
    let sheet = h.state().session.model.sheets()[0];
    click_in(&mut h, "Level 1", Modifiers::NONE);
    click_in(&mut h, "A101 - Plans", Modifiers::SHIFT);
    let picked = h.state().browser_picked.clone();
    assert_eq!(picked.first(), Some(&Pick::View(Sheet::Plan(l[0]))));
    assert_eq!(picked.last(), Some(&Pick::View(Sheet::Paper(sheet))), "{picked:?}");
    assert!(picked.contains(&Pick::View(Sheet::Ceiling(l[1]))), "everything between: {picked:?}");
    assert!(h.query_by_label_contains("Multiple Categories").is_some());
    // A group's right-click: its views as a set, and folding.
    in_browser(&h, "Floor Plans").scroll_to_me();
    h.run();
    in_browser(&h, "Floor Plans").click_secondary();
    h.run();
    assert!(h.query_by_label("Create Print Set of 2…").is_some(), "Floor Plans' two");
    press(&mut h, "Collapse All");
    assert!(h.get_all_by_label("Level 1").all(|n| n.rect().center().x >= 290.0), "folded away");
    in_browser(&h, "Views (all)").scroll_to_me();
    h.run();
    in_browser(&h, "Views (all)").click_secondary();
    h.run();
    press(&mut h, "Expand All");
    assert!(h.get_all_by_label("Level 1").any(|n| n.rect().center().x < 290.0), "open again");
}

#[test]
fn the_print_sets_window_saves_as_renames_and_deletes_a_set() {
    let mut h = app(PROJECT);
    let sheet = h.state().session.model.sheets()[0];
    h.state_mut().open_print_sets(None);
    h.run();
    assert!(h.get_all_by_role(Role::ComboBox).any(|n| n.value().as_deref() == Some("<in-session>")), "no set yet");
    let title = h.state().printables().into_iter().find(|(id, ..)| *id == sheet).map(|(_, t, _)| t).unwrap();
    pick(&mut h, Role::CheckBox, &title);
    press(&mut h, "Save As…");
    type_into(&mut h, "New name", "Issue");
    press(&mut h, "Create");
    let m = &h.state().session.model;
    assert_eq!(m.print_sets["Issue"].views, vec![m.guid(sheet)]);
    assert_eq!(h.state().print_sets.as_ref().unwrap().current.as_deref(), Some("Issue"));
    // Show: Views only hides the sheet.
    pick(&mut h, Role::CheckBox, "Sheets");
    assert!(h.query_by_label(&title).is_none());
    pick(&mut h, Role::CheckBox, "Sheets");
    // Check All, then Save.
    press(&mut h, "Check All");
    // The window's Save, not the title bar's.
    h.get_all_by_label("Save").max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().click();
    h.run();
    let n = h.state().printables().len();
    assert_eq!(h.state().session.model.print_sets["Issue"].views.len(), n);
    press(&mut h, "Rename…");
    type_into(&mut h, "Rename to", "Issue A");
    press(&mut h, "Rename");
    assert!(h.state().session.model.print_sets.contains_key("Issue A"));
    press(&mut h, "Delete");
    assert!(h.state().session.model.print_sets.is_empty());
}

#[test]
fn a_print_set_exports_as_one_pdf_from_the_export_dialog() {
    let mut h = app(PROJECT);
    let l = levels(&h);
    let sheet = h.state().session.model.sheets()[0];
    h.state_mut().session.execute("print_set", &json!({"name": "Issue", "views": [sheet, l[1]]})).unwrap();
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run();
    assert_eq!(h.state().export_dialog.as_ref().unwrap().options.page.margin, 5.0, "a 5 mm margin to start with");
    pick(&mut h, Role::RadioButton, "Print set");
    assert_eq!(h.state().export_dialog.as_ref().unwrap().of, crate::export::Of::Set("Issue".into()));
    assert!(h.query_by_label("2 views and sheets, in one file").is_some());
    // The document's settings are in their own window.
    assert!(h.query_by_label("Archive").is_none());
    press(&mut h, "PDF Settings…");
    assert!(h.query_by_label("Archive").is_some() && h.query_by_label("Author").is_some());
    h.get_all_by_label("OK").last().unwrap().click();
    h.run();
    assert!(!h.state().export_dialog.as_ref().unwrap().pdf_open);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("issue.pdf");
    press(&mut h, "Export…");
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a file dialog was begun");
    tx.send(Some(vec![file.clone()])).unwrap();
    h.run();
    for _ in 0..2000 {
        if !h.state().exporting() {
            break;
        }
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    let pdf = std::fs::read(&file).unwrap_or_else(|e| panic!("{e}: {}", h.state().status.text));
    assert_eq!(String::from_utf8_lossy(&pdf).matches("/Type/Page/").count(), 2);
}

#[test]
fn pages_take_a_typed_list() {
    let mut h = app(PROJECT);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run();
    pick(&mut h, Role::RadioButton, "Custom");
    type_into(&mut h, "Pages to print", "1, 3-4");
    assert_eq!(
        h.state().export_dialog.as_ref().unwrap().options.page.pages,
        Some(bimcraft_core::export::page::Pages::List("1, 3-4".into()))
    );
    pick(&mut h, Role::RadioButton, "All");
    assert_eq!(h.state().export_dialog.as_ref().unwrap().options.page.pages, None);
}
