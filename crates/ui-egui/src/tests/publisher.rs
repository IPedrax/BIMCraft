//! The Publisher: a publishing set made by clicks, its folders and print sets with their formats, a print set made from a
//! row, file names put together of tokens, and Publish writing the set on the export's thread.

use super::*;
use egui_kittest::kittest::NodeT;
use web_time::Duration;

const PROJECT: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
sheet {"name": "Plans"}
sheet {"name": "Details"}
place {"sheet": "A101", "view": "Level 1"}
print_set {"name": "Sheets", "views": ["A101", "A102"]}
print_set {"name": "Plans", "views": ["Level 1"]}"#;

/// Frames until `done` says so, a few seconds at most: a set is published on a thread of its own.
fn until(h: &mut Harness<BimApp>, done: impl Fn(&BimApp) -> bool) {
    for _ in 0..2000 {
        if done(h.state()) {
            return;
        }
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("it never came: {}", h.state().status.text);
}

/// The window's own button `label`, not the title bar's or the ribbon's of that name: the lowest one.
fn lowest(h: &mut Harness<BimApp>, label: &str) {
    h.get_all_by_label(label).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().click();
    h.run();
}

fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-publisher-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The Publisher open on a new set called `name`.
fn publisher(name: &str) -> Harness<'static, BimApp> {
    let mut h = app(PROJECT);
    h.state_mut().open_publisher();
    h.run();
    press(&mut h, "New…");
    type_into(&mut h, "New name", name);
    press(&mut h, "Create");
    h
}

#[test]
fn a_publishing_set_is_made_by_clicks_saved_and_published() {
    let mut h = publisher("Issue");
    assert!(h.state().session.model.publish_sets.contains_key("Issue"), "{}", h.state().status.text);
    assert_eq!(h.state().publisher.as_ref().unwrap().current.as_deref(), Some("Issue"));
    // A folder, and a print set in it: the first, as PDF.
    press(&mut h, "Add Folder");
    press(&mut h, "Add Print Set");
    let d = h.state().publisher.as_ref().unwrap();
    assert_eq!(d.draft.folders[0].name, "PDF");
    let e = &d.draft.folders[0].entries[0];
    assert_eq!((e.print_set.as_str(), e.format.as_str()), ("Plans", "pdf"));
    assert!(d.changed(), "not saved yet");
    assert!(h.state().session.model.publish_sets["Issue"].is_empty(), "the project is changed by Save, not before");
    // The same print set again is another format: a print set is in a folder once for each.
    press(&mut h, "Add Print Set");
    let d = h.state().publisher.as_ref().unwrap();
    let again = &d.draft.folders[0].entries[1];
    assert_eq!(again.print_set, "Plans");
    assert_ne!(again.format, "pdf");
    let dir = folder("clicks");
    type_into(&mut h, "Destination", &dir.to_string_lossy());
    lowest(&mut h, "Save");
    let saved = &h.state().session.model.publish_sets["Issue"];
    assert_eq!((saved.len(), saved.folder.as_str()), (2, &*dir.to_string_lossy()), "{}", h.state().status.text);
    assert!(!h.state().publisher.as_ref().unwrap().changed());
    // Publish writes it on the export's thread; the window says what it wrote.
    press(&mut h, "Publish");
    until(&mut h, |a| !a.exporting());
    assert!(!h.state().status.error, "{}", h.state().status.text);
    h.run();
    let time = h.state().session.model.publish_sets["Issue"].folders[0].entries[0].last_export.as_ref().unwrap();
    assert!(time.ended_ms >= time.started_ms);
    assert!(h.query_by_label_contains("Estimated reexport:").is_some());
    assert!(dir.join("PDF").join("Level 1.pdf").is_file(), "{}", h.state().status.text);
    let written = std::fs::read_dir(dir.join("PDF")).unwrap().count();
    assert_eq!(written, 2, "the plan as PDF and as its other format");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn publishing_sets_and_their_print_sets_transfer_as_xml() {
    let mut h = publisher("Issue");
    press(&mut h, "Add Folder");
    press(&mut h, "Add Print Set");
    lowest(&mut h, "Save");
    let xml = crate::publisher::transfer_xml(&h.state().session.model).unwrap();
    assert!(xml.starts_with("<?xml"));
    assert!(xml.contains("<PrintSet") && xml.contains("<PublishingSet"));
    let path = folder("transfer").with_extension("xml");
    std::fs::write(&path, xml).unwrap();
    let (name, missing) = h.state_mut().import_publisher_xml(&path).unwrap();
    assert_eq!(missing, 0);
    assert_eq!(name.as_deref(), Some("Issue (2)"));
    assert_eq!(h.state().session.model.publish_sets["Issue (2)"].folders[0].entries[0].print_set, "Plans");
    // Another project has different GUIDs for the same named views; titles reconnect them.
    let mut other = app(PROJECT);
    assert_ne!(h.state().session.model.print_sets["Plans"].views, other.state().session.model.print_sets["Plans"].views);
    let (name, missing) = other.state_mut().import_publisher_xml(&path).unwrap();
    assert_eq!((name.as_deref(), missing), (Some("Issue"), 0));
    assert_eq!(other.state().session.model.publish_sets["Issue"].folders[0].entries[0].print_set, "Plans");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn publisher_opens_from_view_tab_and_project_browser() {
    let mut h = app(PROJECT);
    press(&mut h, "View");
    from_list(&mut h, "Windows", "Publisher");
    assert!(h.state().publisher.is_some());
    press(&mut h, "Close");
    let node = h.get_all_by_label("Level 1").find(|n| n.rect().center().x < 290.0 && n.accesskit_node().role() == Role::Button).unwrap();
    node.click_secondary();
    h.run();
    press(&mut h, "Publisher…");
    assert!(h.state().publisher.is_some());
}

#[test]
fn publish_saves_what_was_changed_and_asks_where_when_the_set_has_no_destination() {
    let mut h = publisher("Issue");
    press(&mut h, "Add Folder");
    press(&mut h, "Add Print Set");
    // Not saved, no destination: Publish saves it, then asks for the folder.
    press(&mut h, "Publish");
    assert_eq!(h.state().session.model.publish_sets["Issue"].len(), 1, "{}", h.state().status.text);
    let dir = folder("asked");
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a folder was asked for");
    tx.send(Some(vec![dir.clone()])).unwrap();
    h.run();
    until(&mut h, |a| !a.exporting());
    assert!(dir.join("PDF").join("Level 1.pdf").is_file(), "{}", h.state().status.text);
    assert_eq!(h.state().session.model.publish_sets["Issue"].folder, "", "the folder asked for is this time's, not the set's");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_print_set_is_made_from_a_row_and_a_deleted_one_leaves_it() {
    let mut h = publisher("Issue");
    press(&mut h, "Add Folder");
    press(&mut h, "Add Print Set");
    // New Print Set… of the row's list opens Print Sets on a set not made yet.
    h.get_all_by_role(Role::ComboBox).find(|n| n.value().as_deref() == Some("Plans")).unwrap().click();
    h.run();
    press(&mut h, "New Print Set…");
    assert!(h.state().print_sets.is_some(), "Print Sets is open over the Publisher");
    press(&mut h, "Check All");
    type_into(&mut h, "New name", "Everything");
    press(&mut h, "Create");
    press(&mut h, "OK");
    assert!(h.state().print_sets.is_none());
    assert!(h.state().session.model.print_sets.contains_key("Everything"), "{}", h.state().status.text);
    h.run();
    let d = h.state().publisher.as_ref().unwrap();
    assert_eq!(d.draft.folders[0].entries[0].print_set, "Everything", "the row holds the set just made");
    lowest(&mut h, "Save");
    assert_eq!(h.state().session.model.publish_sets["Issue"].folders[0].entries[0].print_set, "Everything");
    // Renamed, it is still the row's; deleted, the row goes.
    h.state_mut().session.execute("print_set", &json!({"name": "Everything", "rename": "All"})).unwrap();
    h.run();
    assert_eq!(h.state().publisher.as_ref().unwrap().draft.folders[0].entries[0].print_set, "All");
    h.state_mut().session.execute("print_set", &json!({"name": "All", "delete": true})).unwrap();
    h.run();
    let d = h.state().publisher.as_ref().unwrap();
    assert!(d.draft.folders[0].entries.is_empty() && !d.changed());
}

#[test]
fn file_names_are_put_together_of_tokens_and_a_pdf_can_be_one_file() {
    let mut h = publisher("Issue");
    h.state_mut().session.execute("project_info", &json!({"number": "2026-014"})).unwrap();
    press(&mut h, "Add Folder");
    press(&mut h, "Add Print Set");
    press(&mut h, "File Names…");
    assert!(h.query_by_label("File Names").is_some());
    // Tokens picked one after the other, the separator between them.
    press(&mut h, "Project Number");
    press(&mut h, "ViewName");
    assert_eq!(h.state().publisher.as_ref().unwrap().rule.as_ref().unwrap().template, "{Project Number} - {ViewName}");
    assert!(h.query_by_label("For example: 2026-014 - Level 1.pdf").is_some(), "a sample of what it makes");
    // One file of the whole print set, named of the project's tokens.
    pick(&mut h, Role::CheckBox, "Combine into one file");
    type_into(&mut h, "File name", "{Project Number} {PrintSet}");
    assert!(h.query_by_label("For example: 2026-014 Plans.pdf").is_some());
    press(&mut h, "OK");
    let d = h.state().publisher.as_ref().unwrap();
    let e = &d.draft.folders[0].entries[0];
    assert_eq!(e.name.as_deref(), Some("{Project Number} - {ViewName}"));
    assert_eq!(e.combine.as_deref(), Some("{Project Number} {PrintSet}"));
    assert!(d.rule.is_none());
    // A token no project has is said in the row, in the window, before anything is published.
    h.state_mut().publisher.as_mut().unwrap().draft.folders[0].entries[0].combine = Some("{Nope}".to_string());
    h.run();
    assert!(h.query_by_label_contains("isn't a token").is_some());
    assert_eq!(crate::publisher::with_token("", "A", " - "), "{A}");
    assert_eq!(crate::publisher::with_token("x/", "A", " - "), "x/{A}");
    assert_eq!(crate::publisher::with_token("{A}", "B", "_"), "{A}_{B}");
}

#[test]
fn long_file_name_preview_stays_below_its_button_and_within_the_folder() {
    let mut h = publisher("Issue");
    let number = "PROJECT-".repeat(10);
    h.state_mut().session.execute("project_info", &json!({"number": number})).unwrap();
    press(&mut h, "Add Folder");
    press(&mut h, "Add Print Set");
    h.state_mut().publisher.as_mut().unwrap().draft.folders[0].entries[0].name = Some("{Project Number} - {ViewName}".into());
    h.run();
    let button = h.get_by_label("File Names…").rect();
    let preview = h.get_by_label_contains("PROJECT-PROJECT-").rect();
    assert!(preview.top() >= button.bottom(), "preview is on the line below the file name controls");
    assert!(preview.width() <= 281.0, "long names stay inside the preview width: {}", preview.width());
}

#[test]
fn an_entrys_settings_are_the_export_dialogs_and_cancel_puts_them_back() {
    let mut h = publisher("Issue");
    press(&mut h, "Add Folder");
    press(&mut h, "Add Print Set");
    press(&mut h, "Settings…");
    assert!(h.query_by_label("PDF Settings").is_some(), "the window is named for the format");
    pick(&mut h, Role::RadioButton, "Greyscale");
    let colour = |h: &Harness<BimApp>| h.state().publisher.as_ref().unwrap().draft.folders[0].entries[0].options.page.colour;
    assert_eq!(colour(&h), bimcraft_core::export::page::Colour::Grey);
    press(&mut h, "Cancel");
    assert_eq!(colour(&h), bimcraft_core::export::page::Colour::Colour, "Cancel puts them back");
    press(&mut h, "Settings…");
    pick(&mut h, Role::RadioButton, "Black and white");
    press(&mut h, "OK");
    assert_eq!(colour(&h), bimcraft_core::export::page::Colour::Black);
    lowest(&mut h, "Save");
    let saved = &h.state().session.model.publish_sets["Issue"].folders[0].entries[0];
    assert_eq!(saved.options.page.colour, bimcraft_core::export::page::Colour::Black);
}

#[test]
fn sets_are_duplicated_renamed_reverted_and_deleted_from_the_list() {
    let mut h = publisher("Issue");
    press(&mut h, "Add Folder");
    press(&mut h, "Add Print Set");
    lowest(&mut h, "Save");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "Name of the copy", "Tender");
    press(&mut h, "Duplicate");
    let m = &h.state().session.model;
    assert_eq!(m.publish_sets["Tender"], m.publish_sets["Issue"], "{}", h.state().status.text);
    assert_eq!(h.state().publisher.as_ref().unwrap().current.as_deref(), Some("Tender"));
    // A name taken is said in the window, and nothing changes.
    press(&mut h, "Rename…");
    type_into(&mut h, "Rename to", "Issue");
    press(&mut h, "Rename");
    assert!(h.state().publisher.as_ref().unwrap().error.as_ref().is_some_and(|e| e.contains("already")));
    type_into(&mut h, "Rename to", "Tender 2");
    press(&mut h, "Rename");
    assert!(h.state().session.model.publish_sets.contains_key("Tender 2"));
    // A change not saved shows as one, and Revert drops it.
    press(&mut h, "×");
    assert!(h.query_by_label("Tender 2 *").is_some());
    press(&mut h, "Revert changes");
    assert_eq!(h.state().publisher.as_ref().unwrap().draft.len(), 1);
    press(&mut h, "Delete");
    let m = &h.state().session.model;
    assert!(!m.publish_sets.contains_key("Tender 2") && m.publish_sets.contains_key("Issue"));
    assert_eq!(h.state().publisher.as_ref().unwrap().current.as_deref(), Some("Issue"));
    press(&mut h, "Close");
    assert!(h.state().publisher.is_none());
}
