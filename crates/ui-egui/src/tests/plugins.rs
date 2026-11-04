//! Plugins in the window: their tab on the ribbon, its buttons, and the Plugins window that switches one off.

use std::path::Path;

use bimcraft_core::sample::SAMPLE;
use bimcraft_plugin::Plugins;
use serde_json::json;

use super::*;

const WASM: &[u8] = include_bytes!("../../../../plugins/area-summary/area_summary.wasm");

/// A folder with the example module under a manifest that declares the `Areas` tab.
fn folder(path: &Path) {
    let plugin = path.join("area-summary");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("area_summary.wasm"), WASM).unwrap();
    let manifest = json!({
        "id": "area-summary",
        "version": "0.1.0",
        "name": "Area Summary",
        "abi": 1,
        "module": "area_summary.wasm",
        "functions": [
            {"id": "summary", "label": "Area Summary", "tab": "Areas", "panel": "Rooms", "order": 1},
            {"id": "write_note", "label": "Write Area Note", "tab": "Areas", "panel": "Rooms", "order": 2}
        ]
    });
    std::fs::write(plugin.join("plugin.json"), serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
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
fn a_plugin_tab_is_on_the_ribbon_and_its_button_runs_the_plugin() {
    let dir = tempfile::tempdir().unwrap();
    folder(dir.path());
    let mut h = app_with_plugins(dir.path());
    assert!(h.query_by_label("Areas").is_some(), "the plugin's tab is in the ribbon");
    press(&mut h, "Areas");
    let before = h.state().session.history().len();
    press(&mut h, "Area Summary");
    assert!(!h.state().status.error, "{}", h.state().status.text);
    assert_eq!(h.state().session.history().len(), before, "a read makes no undo step");
    // The plugin's command is a command of the session, with its plugin marked for agents.
    let catalog = h.state().session.catalog();
    let entry = catalog.as_array().unwrap().iter().find(|c| c["id"] == json!("plugin:area-summary:summary")).cloned().unwrap();
    assert_eq!(entry["plugin"], json!("area-summary"));
}

#[test]
fn a_new_project_keeps_the_plugins_their_tab_and_their_commands() {
    let dir = tempfile::tempdir().unwrap();
    folder(dir.path());
    let mut h = app_with_plugins(dir.path());
    assert!(h.query_by_label("Areas").is_some());
    press(&mut h, "File");
    h.get_by_label_contains("New").click();
    h.run();
    assert!(h.state().session.model.name == "Project", "a new project");
    assert_eq!(h.state().plugin_tabs.len(), 1, "the plugin's tab stays");
    assert_eq!(h.state().plugins.plugins[0].status, bimcraft_plugin::Status::Loaded);
    assert!(h.state().session.external_command("plugin:area-summary:summary").is_some(), "its commands stay registered");
    assert!(h.query_by_label("Areas").is_some(), "and the tab is drawn again");
}

#[test]
fn the_plugins_window_lists_a_plugin_and_switches_it_off_so_its_tab_goes() {
    let dir = tempfile::tempdir().unwrap();
    folder(dir.path());
    let mut h = app_with_plugins(dir.path());
    press(&mut h, "Manage");
    press(&mut h, "Plugins");
    let body = h.state().plugins.log().iter().map(|e| e.text.clone()).collect::<Vec<_>>().join(" ");
    assert!(body.contains("loaded"), "the window has the log: {body}");
    press(&mut h, "Disable");
    h.run();
    assert_eq!(h.state().plugin_tabs.len(), 0, "the disabled plugin's tab is gone");
    assert!(h.state().session.external_command("plugin:area-summary:summary").is_none());
    assert!(h.state().plugins.plugins[0].status == bimcraft_plugin::Status::Disabled);
    press(&mut h, "Enable");
    h.run();
    assert_eq!(h.state().plugin_tabs.len(), 1, "and back with it");
    assert!(h.state().session.external_command("plugin:area-summary:summary").is_some());
}
