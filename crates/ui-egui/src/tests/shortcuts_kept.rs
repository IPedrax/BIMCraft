//! The shortcuts the user made in Keyboard Shortcuts (KS), kept in `settings.json` so they last past the window: what is written, what
//! the next window does with it, what Restore Defaults leaves, and what a file that was edited by hand or damaged cannot make the
//! window do.

use super::*;
use crate::settings::{MAX_SHORTCUTS, Settings};
use egui_kittest::kittest::NodeT;

fn kept(file: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
}

/// A shortcut made in the dialog as a user makes it: Add Shortcut, its keys typed, its action picked, Add.
fn add_in_the_dialog(h: &mut Harness<BimApp>, keys: &str, action: &str) {
    shortcut(h, "KS");
    assert!(h.state().show_shortcuts);
    press(h, "Add Shortcut");
    h.state_mut().shortcuts_dialog.new_action = action.to_string();
    type_beside(h, "Shortcut Keys:", keys);
    press(h, "Add");
    assert!(h.state().shortcuts_dialog.error.is_none(), "{:?}", h.state().shortcuts_dialog.error);
}

/// Replaces the text of the field on the row of `label`, to its right, as a user selecting all and typing does: the dialog's label is
/// not tied to its field.
fn type_beside(h: &mut Harness<BimApp>, label: &str, text: &str) {
    let at = h.get_by_label(label).rect();
    // Of the fields on that row, to its right, the nearest (BIMCraft's Inspector has others on the row, further right).
    let field = h
        .get_all_by_role(Role::TextInput)
        .filter(|n| n.rect().left() >= at.right() && (n.rect().center().y - at.center().y).abs() < 6.0)
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .map(|n| n.accesskit_node().id())
        .unwrap();
    h.get_by(|n| n.id() == field).click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by(|n| n.id() == field).type_text(text);
    h.run();
}

#[test]
fn a_shortcut_made_is_kept_in_the_settings_and_works_in_the_next_window() {
    for (profile, build) in [("Rev", app as fn(&str) -> Harness<'static, BimApp>), ("BIMCraft", bimcraft)] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let mut h = build("");
        h.state_mut().use_settings(file.clone());
        assert!(h.state().custom_shortcuts.is_empty(), "{profile}: none until one is made");
        add_in_the_dialog(&mut h, "ZW", "Door");
        h.run();
        assert_eq!(h.state().custom_shortcuts.get("ZW").map(String::as_str), Some("Door"), "{profile}");
        assert_eq!(kept(&file)["shortcuts"], json!({"ZW": "Door"}), "{profile}: written as it is made");

        // The next window, built from those settings, has it, and the key does what it was made to do.
        let mut again = build("");
        again.state_mut().use_settings(file.clone());
        again.run();
        assert_eq!(again.state().custom_shortcuts.get("ZW").map(String::as_str), Some("Door"), "{profile}");
        shortcut(&mut again, "ZW");
        assert!(matches!(again.state().tool, Tool::Door), "{profile}: the key made last time picks the Door tool");
        assert_eq!(kept(&file)["shortcuts"], json!({"ZW": "Door"}), "{profile}: and starting did not rewrite it");
    }
}

#[test]
fn restore_defaults_in_the_dialog_is_kept_too() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    add_in_the_dialog(&mut h, "ZW", "Door");
    h.run();
    assert_eq!(kept(&file)["shortcuts"], json!({"ZW": "Door"}));

    press(&mut h, "Restore Defaults");
    assert!(h.state().custom_shortcuts.is_empty());
    assert_eq!(kept(&file)["shortcuts"], json!({}), "the file has none again");
    press(&mut h, "Close");
    shortcut(&mut h, "WA");
    assert!(matches!(h.state().tool, Tool::Wall(_)), "and the defaults work");

    let mut last = app("");
    last.state_mut().use_settings(file);
    assert!(last.state().custom_shortcuts.is_empty(), "the next window starts with the defaults");
    shortcut(&mut last, "ZW");
    assert!(matches!(last.state().tool, Tool::Modify), "ZW is nothing again");
}

#[test]
fn a_key_edited_in_the_dialog_replaces_the_old_one_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    add_in_the_dialog(&mut h, "ZW", "Door");
    h.run();
    h.state_mut().shortcuts_dialog.editing_action = Some("Door".to_string());
    h.state_mut().shortcuts_dialog.edit_keys = "ZQ".to_string();
    h.run();
    // (The Quick Access Toolbar's Save is named alike, and higher up.)
    let save =
        h.get_all_by_label("Save").max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).map(|n| n.accesskit_node().id()).unwrap();
    // And the row of Door is down the dialog's list, out of its view until it is scrolled to.
    h.get_by(|n| n.id() == save).scroll_to_me();
    h.run();
    h.get_by(|n| n.id() == save).click();
    h.run();
    assert_eq!(kept(&file)["shortcuts"], json!({"ZQ": "Door"}));
}

#[test]
fn file_new_and_open_keep_the_shortcuts_and_do_not_wipe_them_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    h.state_mut().custom_shortcuts.insert("ZW".to_string(), "Wall".to_string());
    h.run();
    h.state_mut().reopened();
    h.run();
    assert_eq!(h.state().custom_shortcuts.get("ZW").map(String::as_str), Some("Wall"), "still there in the new project");
    shortcut(&mut h, "ZW");
    assert!(matches!(h.state().tool, Tool::Wall(_)));
    assert_eq!(kept(&file)["shortcuts"], json!({"ZW": "Wall"}), "and in the file");
}

#[test]
fn a_settings_file_keeps_only_two_character_keys_that_name_an_action() {
    let base = default_settings();
    let read = |shortcuts: Value| Settings::from_json(&json!({"shortcuts": shortcuts}), base.clone()).shortcuts;
    let keys = |m: &std::collections::BTreeMap<String, String>| m.keys().cloned().collect::<Vec<_>>();

    let found = read(json!({
        "ZW": "Wall",
        "zf": "Floor",
        "Q": "Door",
        "ABC": "Door",
        "Z!": "Door",
        "ZD": "no such action",
        "ZE": 5,
        "ZG": null,
        "ZP": "Parede",
        "ZT": ["Wall"],
    }));
    // A lower-case key is read as the upper-case one; an action is only its own English name, as it was written.
    assert_eq!(keys(&found), ["ZF", "ZW"]);
    assert_eq!(found.get("ZF").map(String::as_str), Some("Floor"));

    for hostile in [json!("Wall"), json!(5), json!(null), json!(["ZW", "Wall"]), json!(true)] {
        assert!(read(hostile.clone()).is_empty(), "{hostile}: the base's (none)");
    }
    let some = Settings { shortcuts: [("ZW".to_string(), "Wall".to_string())].into(), ..base.clone() };
    assert_eq!(Settings::from_json(&json!({"shortcuts": 5}), some.clone()).shortcuts, some.shortcuts, "not an object: the base's stay");
    assert!(Settings::from_json(&json!({"shortcuts": {}}), some.clone()).shortcuts.is_empty(), "an empty object restores the defaults");
    assert_eq!(Settings::from_json(&json!({}), some.clone()).shortcuts, some.shortcuts, "no key: the base's stay");
}

#[test]
fn a_settings_file_with_thousands_of_shortcuts_keeps_a_bounded_few() {
    let base = default_settings();
    let many: serde_json::Map<String, Value> = (0..36 * 36)
        .map(|n| {
            let digit = |d: usize| char::from_digit(d as u32, 36).unwrap().to_ascii_uppercase();
            (format!("{}{}", digit(n / 36), digit(n % 36)), json!("Wall"))
        })
        .collect();
    assert_eq!(many.len(), 1296);
    let found = Settings::from_json(&json!({"shortcuts": many}), base).shortcuts;
    assert!(found.len() <= MAX_SHORTCUTS, "{} kept", found.len());
    assert!(!found.is_empty());
}

fn default_settings() -> Settings {
    let mut h = app("");
    h.state_mut().use_settings(tempfile::tempdir().unwrap().path().join("settings.json"));
    h.state().settings_kept.clone().unwrap()
}
