//! The shortcuts the user made in Keyboard Shortcuts (KS), kept in `settings.json` so they last past the window: what is written, what
//! the next window does with it, what Reset All leaves, and what a file edited by hand or damaged cannot make the window do.

use super::*;
use crate::settings::Settings;
use crate::shortcuts::{Config, Shortcuts, parse_binding, resolve_action};

fn kept(file: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
}

/// A binding made in the dialog as a user makes it: the action picked, the combination captured, Apply.
fn add_in_the_dialog(h: &mut Harness<BimApp>, keys: &str, action: &str) {
    shortcut(h, "KS");
    assert!(h.state().show_shortcuts);
    let id = resolve_action(action).unwrap();
    let binding = parse_binding(keys).unwrap();
    h.state_mut().shortcuts_dialog.pending.add_binding(&id, binding, true).unwrap();
    press(h, "Apply Changes");
}

fn has(h: &Harness<BimApp>, keys: &str, action: &str) -> bool {
    let id = resolve_action(action).unwrap();
    let binding = parse_binding(keys).unwrap();
    h.state().shortcuts.bindings(&id).contains(&binding)
}

#[test]
fn a_shortcut_made_is_kept_in_the_settings_and_works_in_the_next_window() {
    for (profile, build) in [("Rev", app as fn(&str) -> Harness<'static, BimApp>), ("BIMCraft", bimcraft)] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let mut h = build("");
        h.state_mut().use_settings(file.clone());
        assert!(!has(&h, "ZW", "Door"), "{profile}: not until one is made");
        add_in_the_dialog(&mut h, "ZW", "Door");
        h.run();
        assert!(has(&h, "ZW", "Door"), "{profile}");
        assert_eq!(kept(&file)["shortcuts"]["version"], json!(2), "{profile}: written in version 2");

        let mut again = build("");
        again.state_mut().use_settings(file.clone());
        again.run();
        assert!(has(&again, "ZW", "Door"), "{profile}");
        shortcut(&mut again, "ZW");
        assert!(matches!(again.state().tool, Tool::Door), "{profile}: the key made last time picks the Door tool");
    }
}

#[test]
fn reset_all_in_the_dialog_is_kept_too() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    add_in_the_dialog(&mut h, "ZW", "Door");
    h.run();
    assert!(has(&h, "ZW", "Door"));

    press(&mut h, "Reset All");
    h.state_mut().shortcuts_dialog.pending = Shortcuts::defaults(crate::shortcuts::Profile::BimCraft);
    press(&mut h, "Apply Changes");
    assert!(!has(&h, "ZW", "Door"));
    press(&mut h, "Close");
    shortcut(&mut h, "WA");
    assert!(matches!(h.state().tool, Tool::Wall(_)), "and the defaults work");
}

#[test]
fn a_key_edited_in_the_dialog_replaces_the_old_one_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    add_in_the_dialog(&mut h, "ZW", "Door");
    h.run();
    // The pending set swaps one binding for another, then Apply writes it.
    let id = resolve_action("Door").unwrap();
    h.state_mut().shortcuts_dialog.pending.remove_binding(&id, parse_binding("ZW").unwrap());
    h.state_mut().shortcuts_dialog.pending.add_binding(&id, parse_binding("ZQ").unwrap(), true).unwrap();
    press(&mut h, "Apply Changes");
    assert!(has(&h, "ZQ", "Door") && !has(&h, "ZW", "Door"));
}

#[test]
fn file_new_and_open_keep_the_shortcuts_and_do_not_wipe_them_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    let id = resolve_action("Wall").unwrap();
    h.state_mut().shortcuts.add_binding(&id, parse_binding("ZW").unwrap(), true).unwrap();
    h.run();
    h.state_mut().reopened();
    h.run();
    assert!(has(&h, "ZW", "Wall"), "still there in the new project");
    shortcut(&mut h, "ZW");
    assert!(matches!(h.state().tool, Tool::Wall(_)));
    assert_eq!(kept(&file)["shortcuts"]["version"], json!(2), "and in the file");
}

#[test]
fn a_settings_file_keeps_version_two_and_migrates_the_old_object() {
    let base = default_settings();
    let read = |shortcuts: Value| Settings::from_json(&json!({"shortcuts": shortcuts}), base.clone()).shortcuts;

    let found = read(json!({
        "version": 2,
        "profile": "custom",
        "bindings": [
            {"action": "cmd.wall", "keys": ["WA", "Ctrl+Shift+E"]},
            {"action": "cmd.door", "keys": ["DR"]}
        ]
    }));
    assert_eq!(found.version, 2);
    assert_eq!(found.profile, crate::shortcuts::Profile::Custom);
    assert_eq!(found.bindings.len(), 2);

    // The old object's keys become sequences over the base profile.
    let old = read(json!({"ZW": "Wall"}));
    assert_eq!(old.version, 2);
    assert!(old.bindings.iter().any(|(action, keys)| action == "cmd.wall" && keys.iter().any(|k| k == "ZW")));

    // Something that is not an object keeps the base.
    for hostile in [json!("Wall"), json!(5), json!(null), json!(["ZW", "Wall"]), json!(true)] {
        assert_eq!(read(hostile.clone()), base.shortcuts, "{hostile}");
    }
}

#[test]
fn a_settings_file_with_thousands_of_bindings_keeps_a_bounded_few() {
    let base = default_settings();
    let mut bindings = Vec::new();
    for n in 0..5000 {
        bindings.push(json!({"action": "cmd.wall", "keys": [format!("Ctrl+{}", n % 10)]}));
    }
    let found = Settings::from_json(&json!({"shortcuts": {"version": 2, "bindings": bindings}}), base).shortcuts;
    assert!(found.bindings.len() <= crate::settings::MAX_SHORTCUTS);
    assert!(!found.bindings.is_empty());
}

#[test]
fn a_binding_assigned_twice_belongs_to_the_first_action() {
    let config = Config {
        version: 2,
        profile: crate::shortcuts::Profile::Custom,
        bindings: vec![("cmd.move".to_string(), vec!["Ctrl+D".to_string()]), ("cmd.copy".to_string(), vec!["Ctrl+D".to_string()])],
    };
    let set = Shortcuts::from_config(&config);
    assert_eq!(set.binding_owner(parse_binding("Ctrl+D").unwrap()).map(|s| s.to_string()), Some("cmd.move".to_string()));
    assert!(set.bindings("cmd.copy").is_empty());
}

fn default_settings() -> Settings {
    let mut h = app("");
    h.state_mut().use_settings(tempfile::tempdir().unwrap().path().join("settings.json"));
    h.state().settings_kept.clone().unwrap()
}
