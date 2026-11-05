//! Plugins in the window: their tab on the ribbon, its buttons, and the Plugins window that switches one off.

use std::path::Path;

use bimcraft_core::sample::SAMPLE;
use bimcraft_plugin::Plugins;
use serde_json::json;

use super::*;

/// A folder whose module is the sample plugin's build named beside it: the app's own `refresh` then finds it,
/// as it would a plugin a user built. Without the build the window's tests are skipped.
fn folder(path: &Path) {
    let Some(module) = bimcraft_plugin::testing::sample_wasm_or_skip() else { return };
    let plugin = path.join("sample");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("sample_plugin.wasm"), module).unwrap();
    let manifest = json!({
        "id": "sample",
        "version": "0.1.0",
        "name": "Sample",
        "abi": 1,
        "module": "sample_plugin.wasm",
        "functions": [
            {"id": "summary", "label": "Area Summary", "tab": "Areas", "panel": "Rooms", "order": 1},
            {"id": "write_note", "label": "Write Area Note", "tab": "Areas", "panel": "Rooms", "order": 2}
        ]
    });
    std::fs::write(plugin.join("plugin.json"), serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
}

/// The window's plugins for `path`, read as the app reads them.
/// The window with the folder's plugins, or none when the sample plugin wasn't built: a test then skips.
fn window_with_plugins(path: &Path) -> Option<Harness<'static, BimApp>> {
    bimcraft_plugin::testing::sample_wasm_or_skip()?;
    Some(app_with_plugins(path))
}

fn app_with_plugins(path: &Path) -> Harness<'static, BimApp> {
    let plugins = Plugins::load_from(Some(path));
    assert!(plugins.plugins[0].status.is_loaded(), "{:?}", plugins.plugins[0].status);
    let mut h = app(SAMPLE);
    h.state_mut().use_plugins(plugins);
    h.run();
    h
}

#[test]
fn the_plugins_window_asks_for_file_access_and_the_user_grants_it() {
    // The row is built from the manifest alone: it needs no built module.
    let dir = tempfile::tempdir().unwrap();
    folder(dir.path());
    // The manifest asks for the permission; nothing is granted until the user says so.
    let manifest = dir.path().join("sample").join("plugin.json");
    let mut value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    value["permissions"] = json!(["files"]);
    value["description"] = json!("Counts the rooms' areas and writes the total back.");
    std::fs::write(&manifest, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let mut h = app(SAMPLE);
    h.state_mut().use_plugins(Plugins::load_from(Some(dir.path())));
    h.run();
    press(&mut h, "Manage");
    press(&mut h, "Plugins");
    h.run();
    // The window says which plugin asks and why, and that it may not yet.
    assert!(h.query_by_label("File access").is_some(), "the window asks");
    assert!(h.query_by_label_contains("Counts the rooms").is_some(), "with the plugin's own reason");
    assert!(!h.state().plugins.permission_granted("sample", "files"));
    press(&mut h, "Allow file access");
    h.run();
    h.run();
    assert!(dir.path().join("state.json").is_file(), "the grant is kept beside the folder's plugins");
    assert!(h.query_by_label("Revoke file access").is_some(), "and the row says it is allowed");
    h.get_by_label("Revoke file access").click();
    h.run();
    h.run();
    let kept = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
    assert!(!kept.contains("files"), "taken back: {kept}");
}

#[test]
fn find_lists_a_plugin_s_function_and_runs_it_as_its_button_does() {
    let dir = tempfile::tempdir().unwrap();
    folder(dir.path());
    let Some(mut h) = window_with_plugins(dir.path()) else { return };
    // Ctrl+K finds the plugin's function by its label and the plugin's name.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    h.run();
    assert!(h.state().find.is_some(), "Ctrl+K opens Find");
    h.event(egui::Event::Text("summ".to_string()));
    h.run();
    h.get_by_label_contains("Summary · Sample").click();
    h.run();
    // Enter runs the first match, which is the plugin's function: it answers from the model, as the ribbon button does.
    key(&mut h, egui::Key::Enter);
    h.run();
    assert!(!h.state().status.error, "{}", h.state().status.text);
    assert!(h.state().find.is_none(), "and Find closes");
}

#[test]
fn a_plugin_tab_is_on_the_ribbon_and_its_button_runs_the_plugin() {
    let dir = tempfile::tempdir().unwrap();
    folder(dir.path());
    let Some(mut h) = window_with_plugins(dir.path()) else { return };
    assert!(h.query_by_label("Areas").is_some(), "the plugin's tab is in the ribbon");
    press(&mut h, "Areas");
    let before = h.state().session.history().len();
    press(&mut h, "Area Summary");
    assert!(!h.state().status.error, "{}", h.state().status.text);
    assert_eq!(h.state().session.history().len(), before, "a read makes no undo step");
    // The plugin's command is a command of the session, with its plugin marked for agents.
    let catalog = h.state().session.catalog();
    let entry = catalog.as_array().unwrap().iter().find(|c| c["id"] == json!("plugin:sample:summary")).cloned().unwrap();
    assert_eq!(entry["plugin"], json!("sample"));
}

#[test]
fn a_new_project_keeps_the_plugins_their_tab_and_their_commands() {
    let dir = tempfile::tempdir().unwrap();
    folder(dir.path());
    let Some(mut h) = window_with_plugins(dir.path()) else { return };
    assert!(h.query_by_label("Areas").is_some());
    press(&mut h, "File");
    h.get_by_label_contains("New").click();
    h.run();
    assert!(h.state().session.model.name == "Project", "a new project");
    assert_eq!(h.state().plugin_tabs.len(), 1, "the plugin's tab stays");
    assert_eq!(h.state().plugins.plugins[0].status, bimcraft_plugin::Status::Loaded);
    assert!(h.state().session.external_command("plugin:sample:summary").is_some(), "its commands stay registered");
    assert!(h.query_by_label("Areas").is_some(), "and the tab is drawn again");
}

#[test]
fn the_plugins_window_lists_a_plugin_and_switches_it_off_so_its_tab_goes() {
    let dir = tempfile::tempdir().unwrap();
    folder(dir.path());
    let Some(mut h) = window_with_plugins(dir.path()) else { return };
    press(&mut h, "Manage");
    press(&mut h, "Plugins");
    let body = h.state().plugins.log().iter().map(|e| e.text.clone()).collect::<Vec<_>>().join(" ");
    assert!(body.contains("loaded"), "the window has the log: {body}");
    press(&mut h, "Disable");
    h.run();
    assert_eq!(h.state().plugin_tabs.len(), 0, "the disabled plugin's tab is gone");
    assert!(h.state().session.external_command("plugin:sample:summary").is_none());
    assert!(h.state().plugins.plugins[0].status == bimcraft_plugin::Status::Disabled);
    // Switched back on, the folder is read again — this time with the built module, as an app's own read would
    // have it (`use_plugins` after the state change; the file names a module the folder doesn't keep).
    press(&mut h, "Enable");
    h.run();
    h.run();
    assert_eq!(h.state().plugin_tabs.len(), 1, "and back with it");
    assert!(h.state().session.external_command("plugin:sample:summary").is_some());
}
