//! Plugins in the window: their tab on the ribbon, its buttons, and the Plugins window that switches one off.

use std::path::Path;

use bimcraft_core::sample::SAMPLE;
use bimcraft_plugin::Plugins;
use serde_json::json;

use crate::plugins::{ReportFormat, report_parts};

use super::*;

/// A folder whose module is the sample plugin's build named beside it: the app's own `refresh` then finds it,
/// as it would a plugin a user built. Without the build it writes nothing and says so: the window's tests then skip.
fn folder(path: &Path) -> Option<()> {
    let module = bimcraft_plugin::testing::sample_wasm_or_skip()?;
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
    Some(())
}

/// The window's plugins for `path`, read as the app reads them.
/// The window with the folder's plugins, or none when the sample plugin wasn't built: a test then skips.
fn window_with_plugins(path: &Path) -> Option<Harness<'static, BimApp>> {
    bimcraft_plugin::testing::sample_wasm_or_skip()?;
    Some(app_with_plugins(path))
}

/// What the Plugins window's row and a ribbon button call, without the drawing: the grant (as `permit_plugin` keeps it)
/// and a function run as its button runs it. The window itself is what the tests above draw.
impl BimApp {
    fn set_permission_for_test(&mut self, id: &str, permission: &str, granted: bool) {
        self.plugins.set_permission(id, permission, granted).unwrap();
        self.plugins.register(&mut self.session);
        self.plugin_tabs = self.plugins.tabs();
    }

    fn run_plugin_for_test(&mut self, id: &str, label: &str, ctx: &egui::Context) {
        self.run_plugin_function(id, label.to_string(), ctx);
    }
}

fn app_with_plugins(path: &Path) -> Harness<'static, BimApp> {
    let plugins = Plugins::load_from(Some(path));
    assert!(plugins.plugins[0].status.is_loaded(), "{:?}", plugins.plugins[0].status);
    let mut h = app(SAMPLE);
    h.state_mut().use_plugins(plugins);
    h.run();
    h
}

/// Gives the folder's sample plugin the panel of the specification's shape: a label, a level, a button, a table and a
/// read-only text. Its functions are the ones the built module has, so its calls run.
fn add_panel(path: &Path) -> Option<()> {
    let manifest = path.join("sample").join("plugin.json");
    let mut value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&manifest).ok()?).ok()?;
    value["abi"] = json!(2);
    value["panels"] = json!([{
        "id": "areas",
        "title": "Area Summary",
        "dock": "right",
        "on_open": "summary",
        "refresh": "summary",
        "widgets": [
            {"type": "label", "text": "Totals by level"},
            {"type": "level", "id": "level", "label": "Level"},
            {"type": "button", "id": "run", "label": "Summarise", "function": "summary"},
            {"type": "table", "id": "rows", "columns": [{"key": "what", "label": "What"}]},
            {"type": "text", "id": "note", "label": "Summary", "readonly": true}
        ]
    }]);
    std::fs::write(&manifest, serde_json::to_string_pretty(&value).ok()?).ok()
}

/// The palette of the panel named `name`, when the registry has it.
fn panel_palette(name: &str) -> Option<crate::dock::Palette> {
    crate::dock::Palette::named(name, &crate::dock::Palette::known())
}

#[test]
fn the_plugins_window_asks_for_file_access_and_the_user_grants_it() {
    // The row is built from the manifest alone: it needs no built module.
    let dir = tempfile::tempdir().unwrap();
    let Some(()) = folder(dir.path()) else { return };
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
fn a_plugin_function_that_needs_a_file_asks_for_it_in_the_system_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let Some(()) = folder(dir.path()) else { return };
    // The same as the plugin's own `open_file`: a function whose manifest makes `path` required.
    let manifest = dir.path().join("sample").join("plugin.json");
    let mut value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    value["permissions"] = json!(["files"]);
    value["functions"].as_array_mut().unwrap().push(json!({
        "id": "open_file", "label": "Open File", "tab": "Areas", "panel": "Rooms", "order": 3,
        "params": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}
    }));
    std::fs::write(&manifest, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let Some(mut h) = window_with_plugins(dir.path()) else { return };
    let project = dir.path().join("Other.bimcraft");
    std::fs::write(&project, h.state().session.model.to_json().unwrap()).unwrap();

    // Without the permission the click says what to allow and begins no dialog at all: a folder the plugin may not
    // read is not put in front of the user.
    let ctx = h.ctx.clone();
    let status = {
        let app = h.state_mut();
        app.run_plugin_for_test("plugin:sample:open_file", "Open File", &ctx);
        app.status.text.clone()
    };
    assert!(crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).is_none(), "no file dialog without the permission");
    assert!(status.contains("Allow file access"), "the window says what to do: {status}");
    assert!(status.contains("Sample"), "and names the plugin: {status}");
    assert!(h.state().plugin_pick.is_none(), "nothing waits for a file");

    // Allowed, the click asks for the file and runs the function with it.
    h.state_mut().set_permission_for_test("sample", "files", true);
    let ctx = h.ctx.clone();
    h.state_mut().run_plugin_for_test("plugin:sample:open_file", "Open File", &ctx);
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("the click begins a file dialog");
    tx.send(Some(vec![project.clone()])).unwrap();
    h.run();
    h.run();
    assert!(!h.state().status.error, "{}", h.state().status.text);
    assert!(h.state().session.path.as_deref() == Some(project.as_path()), "the project is the file the dialog gave");
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
    // File > New > Project: New is a list of Project and Family here.
    new_project(&mut h);
    assert!(h.state().session.model.name == "Project", "a new project");
    assert_eq!(h.state().plugin_tabs.len(), 1, "the plugin's tab stays");
    assert_eq!(h.state().plugins.plugins[0].status, bimcraft_plugin::Status::Loaded);
    assert!(h.state().session.external_command("plugin:sample:summary").is_some(), "its commands stay registered");
    assert!(h.query_by_label("Areas").is_some(), "and the tab is drawn again");
}

fn open_report(h: &mut Harness<'static, BimApp>, title: &str, text: &str, format: ReportFormat) {
    h.state_mut().open_plugin_report(title.to_string(), text, format);
    h.run();
}

fn opened_pages(h: &Harness<BimApp>) -> Vec<String> {
    h.output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| if let egui::OutputCommand::OpenUrl(u) = c { Some(u.url.clone()) } else { None })
        .collect()
}

#[test]
fn a_plain_string_report_is_still_text() {
    let mut h = app(SAMPLE);
    open_report(&mut h, "Report", "Levels: 2\nWalls: 12", ReportFormat::Text);
    assert!(h.query_by_label("Levels: 2").is_some());
    assert!(h.query_by_label("Walls: 12").is_some());
}

#[test]
fn markdown_report_renders_headings_lists_and_tables() {
    let mut h = app(SAMPLE);
    let source = "# Report\n\n## Levels\n\n- Ground\n- First\n\n| Category | Count |\n|---|---:|\n| Walls | 12 |\n";
    open_report(&mut h, "Report", source, ReportFormat::Markdown);
    // The dialog's title and the H1 both read `Report`.
    assert!(h.query_all_by_label("Report").count() >= 2);
    assert!(h.query_by_label("Levels").is_some());
    assert!(h.query_by_label("Ground").is_some());
    assert!(h.query_by_label("Category").is_some(), "the table's header is a cell, not a line of its source");
    assert!(h.query_by_label("Walls").is_some());
    assert!(h.query_by_label("| Category | Count |").is_none(), "the source is not drawn as monospace lines");
}

#[test]
fn an_unknown_report_format_is_refused_and_only_a_string_or_an_object_is_a_report() {
    assert!(report_parts(&json!({"text": "x", "format": "html"})).is_err());
    assert!(report_parts(&json!({})).is_err(), "an empty object has no text");
    assert!(report_parts(&json!({"text": 7})).is_err());
    assert!(report_parts(&json!(7)).is_err());
    let (text, format) = report_parts(&json!("plain")).unwrap();
    assert_eq!((text.as_str(), format), ("plain", ReportFormat::Text));
    let (text, format) = report_parts(&json!({"text": "# Report", "format": "markdown"})).unwrap();
    assert_eq!((text.as_str(), format), ("# Report", ReportFormat::Markdown));
    let (_, format) = report_parts(&json!({"text": "plain"})).unwrap();
    assert_eq!(format, ReportFormat::Text, "a missing format is text");
}

#[test]
fn report_text_longer_than_the_limit_is_cut_once_and_warned() {
    let mut h = app(SAMPLE);
    let source = "a".repeat(300 * 1024);
    open_report(&mut h, "Report", &source, ReportFormat::Text);
    let report = h.state().plugin_report.as_ref().expect("the report is open");
    assert_eq!(report.source().len(), 256 * 1024);
    assert!(h.query_by_label_contains("cut").is_some(), "the window says it was cut");
}

#[test]
fn broken_markdown_falls_back_to_plain_text() {
    let mut h = app(SAMPLE);
    open_report(&mut h, "Report", "```\nnever closed\n", ReportFormat::Markdown);
    assert!(h.query_by_label_contains("couldn't be read").is_some());
    assert!(h.query_by_label("```").is_some(), "the source is shown as text");
}

#[test]
fn copy_takes_the_source_markdown_not_the_rendered_text() {
    let mut h = app(SAMPLE);
    let source = "# Report\n\n| Category | Count |\n|---|---:|\n| Walls | 12 |\n";
    open_report(&mut h, "Report", source, ReportFormat::Markdown);
    // The ribbon has a Copy tool too: the report's button is the one below it.
    let copy = h.get_all_by_label("Copy").max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap();
    copy.click();
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| if let egui::OutputCommand::CopyText(text) = c { Some(text.clone()) } else { None });
    assert_eq!(copied.as_deref(), Some(source));
}

#[test]
fn report_links_with_javascript_or_file_scheme_are_not_active() {
    let mut h = app(SAMPLE);
    open_report(&mut h, "Report", "[click](javascript:alert(1)) and [file](file:///etc/passwd)", ReportFormat::Markdown);
    h.get_by_label("click").click();
    h.step();
    assert!(opened_pages(&h).is_empty(), "javascript: is never opened");
    h.get_by_label("file").click();
    h.step();
    assert!(opened_pages(&h).is_empty(), "file: is never opened");
}

#[test]
fn report_links_with_https_open_on_a_click() {
    let mut h = app(SAMPLE);
    open_report(&mut h, "Report", "[site](https://example.org/report)", ReportFormat::Markdown);
    h.get_by_label("site").click();
    h.step();
    assert_eq!(opened_pages(&h), ["https://example.org/report"]);
}

#[test]
fn report_html_and_images_stay_text() {
    let mut h = app(SAMPLE);
    open_report(&mut h, "Report", "<script>alert(1)</script> ![alt](https://example.org/a.png)", ReportFormat::Markdown);
    assert!(h.query_by_label_contains("<script>alert(1)</script>").is_some());
    assert!(h.query_by_label_contains("![alt](https://example.org/a.png)").is_some());
    assert!(opened_pages(&h).is_empty());
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

#[test]
fn a_plugin_panel_opens_with_its_widgets_and_its_button_runs_the_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let Some(()) = folder(dir.path()) else { return };
    let Some(()) = add_panel(dir.path()) else { return };
    let Some(mut h) = window_with_plugins(dir.path()) else { return };
    // The plugin's panel has joined the palettes the window knows, by its stable name.
    let palette = panel_palette("plugin:sample:areas").expect("the plugin's panel is in the registry");
    h.state_mut().show_palette(palette, true);
    h.run();
    assert!(h.query_by_label("Totals by level").is_some(), "the panel's label is drawn");
    assert!(h.query_all_by_label("Level").count() >= 2, "its level caption and its combo box");
    // The button calls the plugin's function with the panel's values: one call, and this one only reads.
    let before = h.state().session.history().len();
    h.get_by_label("Summarise").click();
    h.run();
    assert!(!h.state().status.error, "{}", h.state().status.text);
    assert_eq!(h.state().session.history().len(), before, "a read makes no undo step");
}

#[test]
fn a_panel_of_a_disabled_plugin_is_kept_in_the_layout_and_comes_back_when_it_is_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let Some(()) = folder(dir.path()) else { return };
    let Some(()) = add_panel(dir.path()) else { return };
    let Some(mut h) = window_with_plugins(dir.path()) else { return };
    let palette = panel_palette("plugin:sample:areas").expect("the panel is registered");
    h.state_mut().show_palette(palette, true);
    h.run();
    assert!(h.state().docking.shown(palette));
    press(&mut h, "Manage");
    press(&mut h, "Plugins");
    press(&mut h, "Disable");
    h.run();
    h.run();
    assert!(panel_palette("plugin:sample:areas").is_none(), "the disabled plugin's palette leaves the registry");
    assert!(h.state().docking.unknown.iter().any(|name| name == "plugin:sample:areas"), "its name stays in the layout");
    assert!(!h.state().docking.shown(palette), "and it is no longer drawn");
    press(&mut h, "Enable");
    h.run();
    h.run();
    let palette = panel_palette("plugin:sample:areas").expect("the palette is registered again");
    assert!(h.state().docking.shown(palette), "and back where it was");
}

#[test]
fn a_wide_panel_does_not_widen_the_dock() {
    let dir = tempfile::tempdir().unwrap();
    let Some(()) = folder(dir.path()) else { return };
    let Some(()) = add_panel(dir.path()) else { return };
    // A table wider than the dock: its columns may be any width a plugin declares, and they scroll, not shove the view.
    let manifest = dir.path().join("sample").join("plugin.json");
    let mut value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    value["panels"][0]["widgets"][3]["columns"] = json!([
        {"key": "a", "label": "A", "width": 200},
        {"key": "b", "label": "B", "width": 200},
        {"key": "c", "label": "C", "width": 200}
    ]);
    std::fs::write(&manifest, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let Some(mut h) = window_with_plugins(dir.path()) else { return };
    let palette = panel_palette("plugin:sample:areas").expect("the panel is registered");
    h.state_mut().show_palette(palette, true);
    h.run();
    h.run();
    let dock = 1280.0 - h.state().view_rect.right();
    assert!(dock <= 330.0, "the dock keeps its width, not {dock}");
}
