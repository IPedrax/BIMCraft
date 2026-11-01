//! The shortcuts the user made, kept in `settings.json` so they last past the window: what is written, what is read back, and
//! what a file that was edited by hand or damaged cannot make the window do.

use super::*;
use crate::settings::Settings;
use serde_json::json;

#[test]
fn a_shortcut_made_is_kept_in_the_settings_and_works_in_the_next_window() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    assert!(h.state().custom_shortcuts.is_empty(), "none until one is made");
    h.state_mut().custom_shortcuts.insert("ZW".to_string(), "Wall".to_string());
    h.run();
    let kept: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(kept["shortcuts"], json!({"ZW": "Wall"}));

    let mut again = app("");
    again.state_mut().use_settings(file.clone());
    assert_eq!(again.state().custom_shortcuts.get("ZW").map(String::as_str), Some("Wall"));
    shortcut(&mut again, "ZW");
    assert!(matches!(again.state().tool, Tool::Wall(_)), "the key made last time draws a wall");

    // Restore Defaults is kept too: the next start has none.
    again.state_mut().custom_shortcuts.clear();
    again.run();
    let mut last = app("");
    last.state_mut().use_settings(file);
    assert!(last.state().custom_shortcuts.is_empty());
}

#[test]
fn a_settings_file_keeps_only_two_character_keys_that_name_an_action() {
    let base = default_settings();
    let read = |shortcuts: serde_json::Value| Settings::from_json(&json!({"shortcuts": shortcuts}), base.clone()).shortcuts;
    let keys = |m: &std::collections::BTreeMap<String, String>| m.keys().cloned().collect::<Vec<_>>();

    let kept = read(json!({
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
    assert_eq!(keys(&kept), ["ZF", "ZW"]);
    assert_eq!(kept.get("ZF").map(String::as_str), Some("Floor"));

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
    let many: serde_json::Map<String, serde_json::Value> = (0..36 * 36)
        .map(|n| {
            let digit = |d: usize| char::from_digit(d as u32, 36).unwrap().to_ascii_uppercase();
            (format!("{}{}", digit(n / 36), digit(n % 36)), json!("Wall"))
        })
        .collect();
    assert_eq!(many.len(), 1296);
    let kept = Settings::from_json(&json!({"shortcuts": many}), base).shortcuts;
    assert!(kept.len() <= crate::settings::MAX_SHORTCUTS, "{} kept", kept.len());
    assert!(!kept.is_empty());
}

fn default_settings() -> Settings {
    let mut h = app("");
    h.state_mut().use_settings(tempfile::tempdir().unwrap().path().join("settings.json"));
    h.state().settings_kept.clone().unwrap()
}
