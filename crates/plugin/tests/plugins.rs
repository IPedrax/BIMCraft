//! The sandbox and the folder: a plugin runs core commands, and a hang, a trap, a memory overrun, a wrong reply,
//! too many commands or a command it may not run is an error of the call, never a crash or a changed model.

// A test target of its own: the workspace's lints allow these in `#[cfg(test)]` modules, which an integration
// test's whole crate is.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use bimcraft_core::engine::{Session, catalog};
use bimcraft_plugin::{Level, Log, Plugins, Status, runtime::Runtime};
use serde_json::{Value, json};

fn wat(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    wat::parse_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn runtime(name: &str) -> Runtime {
    Runtime::new(&wat(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn invoke(rt: &Runtime, s: &mut Session, files: bool, request: Value) -> Result<Value, String> {
    let log = Arc::new(Log::new());
    rt.invoke(s, "test", "run", files, &request, &log)
}

fn level(name: &str) -> Value {
    json!({"command": "level", "params": {"elevation": 1200, "name": name}})
}

fn levels(s: &Session) -> usize {
    s.model.levels().len()
}

#[test]
fn a_plugin_function_reaches_a_command_and_gets_its_json() {
    let rt = runtime("forward.wat");
    let mut s = Session::new();
    let before = levels(&s);
    let reply = invoke(&rt, &mut s, false, level("Roof")).unwrap();
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(reply["result"]["name"], json!("Roof"));
    assert_eq!(levels(&s), before + 1, "the command made a level through Session::execute");
}

#[test]
fn a_hang_is_stopped_by_the_fuel_limit_and_the_session_stays_up() {
    let rt = runtime("loop.wat");
    let mut s = Session::new();
    let started = std::time::Instant::now();
    let e = invoke(&rt, &mut s, false, level("Roof")).unwrap_err();
    assert!(e.contains("failed") || e.contains("fuel"), "{e}");
    assert!(started.elapsed() < std::time::Duration::from_secs(30), "the limit stopped it");
    // The session is untouched and still works.
    assert_eq!(levels(&s), 2);
    s.execute("level", &json!({"elevation": 900})).unwrap();
    assert_eq!(levels(&s), 3);
}

#[test]
fn a_trap_is_an_error_not_a_crash() {
    let rt = runtime("trap.wat");
    let mut s = Session::new();
    let e = invoke(&rt, &mut s, false, level("Roof")).unwrap_err();
    assert!(e.contains("failed") || e.contains("unreachable") || e.contains("trap"), "{e}");
    assert_eq!(levels(&s), 2);
}

#[test]
fn a_reply_that_is_not_json_is_an_error() {
    let rt = runtime("badjson.wat");
    let mut s = Session::new();
    let e = invoke(&rt, &mut s, false, json!({})).unwrap_err();
    assert!(e.contains("isn't JSON"), "{e}");
}

#[test]
fn more_memory_than_the_limit_is_an_error() {
    let rt = runtime("huge_memory.wat");
    let mut s = Session::new();
    let e = invoke(&rt, &mut s, false, json!({})).unwrap_err();
    assert!(e.contains("64 MB") || e.contains("memory") || e.contains("limit"), "{e}");
}

#[test]
fn the_files_permission_is_checked_before_the_command_runs() {
    let rt = runtime("forward.wat");
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("saved.bimcraft");
    let request = json!({"command": "save", "params": {"path": path}});
    let mut s = Session::new();
    let reply = invoke(&rt, &mut s, false, request.clone()).unwrap();
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert!(reply["error"].as_str().unwrap_or_default().contains("files"), "{reply}");
    assert!(!path.exists(), "no file is written without the permission");
    // With the permission the same call writes it.
    let reply = invoke(&rt, &mut s, true, request).unwrap();
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert!(path.exists());
}

#[test]
fn a_plugin_cannot_import_the_file_system() {
    let e = match Runtime::new(&wat("imports_wasi.wat")) {
        Ok(_) => panic!("a WASI import must be refused"),
        Err(e) => e,
    };
    assert!(e.contains("wasi_snapshot_preview1") && e.contains("bimcraft"), "{e}");
}

#[test]
fn a_command_storm_stops_at_the_call_s_budget() {
    let rt = runtime("storm.wat");
    let mut s = Session::new();
    let request = json!({"command": "level", "params": {}});
    let reply = invoke(&rt, &mut s, false, request).unwrap();
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert!(reply["error"].as_str().unwrap_or_default().contains("1000 commands"), "{reply}");
}

// --- the folder, the manifests and the command registry ---

fn write_plugin(root: &Path, id: &str, module: &str, functions: Value, permissions: Value) {
    let folder = root.join(id);
    std::fs::create_dir_all(&folder).unwrap();
    let manifest = json!({
        "id": id,
        "version": "1.0.0",
        "name": id,
        "abi": 1,
        "permissions": permissions,
        "functions": functions
    });
    std::fs::write(folder.join("plugin.json"), serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    std::fs::write(folder.join("plugin.wasm"), wat(module)).unwrap();
}

fn functions() -> Value {
    json!([{"id": "run", "label": "Run"}])
}

fn folder() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "good", "forward.wat", functions(), json!([]));
    root
}

#[test]
fn a_plugin_in_the_folder_registers_its_command_and_the_catalog_marks_it() {
    let root = folder();
    let plugins = Plugins::load_from(Some(root.path()));
    assert_eq!(plugins.plugins.len(), 1);
    assert_eq!(plugins.plugins[0].status, Status::Loaded);
    let mut s = Session::new();
    plugins.register(&mut s);
    assert!(s.external_command("plugin:good:run").is_some());
    let entry = s.catalog().as_array().unwrap().iter().find(|c| c["id"] == json!("plugin:good:run")).cloned().unwrap();
    assert_eq!(entry["plugin"], json!("good"));
    // The static catalog is unchanged: no `plugin` field, and nothing else differs.
    assert!(catalog().as_array().unwrap().iter().all(|c| c.get("plugin").is_none()));
    let reply = s.execute("plugin:good:run", &level("Roof")).unwrap();
    assert_eq!(reply["ok"], json!(true));
}

#[test]
fn one_call_is_one_undo_step_with_the_plugin_s_label() {
    let root = folder();
    let plugins = Plugins::load_from(Some(root.path()));
    let mut s = Session::new();
    plugins.register(&mut s);
    let before = s.history().len();
    s.execute("plugin:good:run", &level("Roof")).unwrap();
    assert_eq!(s.history().len(), before + 1, "one step");
    assert_eq!(s.history().last().copied(), Some("good: Run"));
    assert_eq!(levels(&s), 3);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(levels(&s), 2, "the step takes back the whole call");
}

#[test]
fn a_failed_call_leaves_no_step_and_no_change() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "partial", "partial_trap.wat", functions(), json!([]));
    let plugins = Plugins::load_from(Some(root.path()));
    let mut s = Session::new();
    plugins.register(&mut s);
    let (before_model, before_history) = ((*s.model).clone(), s.history().len());
    let e = s.execute("plugin:partial:run", &level("Roof")).unwrap_err();
    assert!(!e.is_empty());
    assert_eq!(s.history().len(), before_history, "no undo step");
    assert_eq!(*s.model, before_model, "the model is as it was");
}

#[test]
fn without_plugins_the_catalog_and_the_history_are_exactly_as_before() {
    let mut s = Session::new();
    let expected = serde_json::to_value(catalog()).unwrap();
    assert_eq!(serde_json::to_value(s.catalog()).unwrap(), expected);
    assert!(s.history().is_empty());
    let e = s.execute("plugin:good:run", &json!({})).unwrap_err();
    assert!(e.contains("unknown command"), "{e}");
}

#[test]
fn a_broken_plugin_does_not_stop_the_others() {
    let root = folder();
    let broken = root.path().join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("plugin.json"), "{ not json").unwrap();
    let plugins = Plugins::load_from(Some(root.path()));
    assert_eq!(plugins.plugins.len(), 1, "the good one is loaded");
    assert_eq!(plugins.plugins[0].manifest.id, "good");
    assert_eq!(plugins.broken.len(), 1);
    assert!(!plugins.log().is_empty(), "the refusal is in the log");
}

#[test]
fn a_plugin_that_will_not_load_says_why_and_does_not_stop_the_others() {
    let root = folder();
    write_plugin(root.path(), "wasi", "imports_wasi.wat", functions(), json!([]));
    let plugins = Plugins::load_from(Some(root.path()));
    let wasi = plugins.plugins.iter().find(|p| p.manifest.id == "wasi").unwrap();
    assert!(matches!(wasi.status, Status::Failed(_)), "{:?}", wasi.status);
    let good = plugins.plugins.iter().find(|p| p.manifest.id == "good").unwrap();
    assert_eq!(good.status, Status::Loaded);
}

#[test]
fn switching_a_plugin_off_removes_its_commands_and_tabs_until_it_is_on_again() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(
        root.path(),
        "good",
        "forward.wat",
        json!([{"id": "run", "label": "Run", "tab": "Good Tab", "panel": "Good Panel", "order": 1}]),
        json!([]),
    );
    let mut plugins = Plugins::load_from(Some(root.path()));
    assert_eq!(plugins.tabs().len(), 1);
    assert_eq!(plugins.tabs()[0].name.as_deref(), Some("Good Tab"));
    plugins.set_enabled("good", false).unwrap();
    let mut s = Session::new();
    plugins.register(&mut s);
    assert!(s.external_command("plugin:good:run").is_none());
    assert!(plugins.tabs().is_empty());
    assert!(!s.catalog().as_array().unwrap().iter().any(|c| c["plugin"] == json!("good")));
    // The state is kept in the folder, so the next read agrees.
    let again = Plugins::load_from(Some(root.path()));
    assert_eq!(again.plugins[0].status, Status::Disabled);
    // And on again.
    let mut plugins = again;
    plugins.set_enabled("good", true).unwrap();
    let mut s = Session::new();
    plugins.register(&mut s);
    assert!(s.external_command("plugin:good:run").is_some());
}

#[test]
fn without_a_declared_tab_functions_go_in_the_common_section() {
    let root = folder();
    let plugins = Plugins::load_from(Some(root.path()));
    let tabs = plugins.tabs();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].name, None, "the common Plugins section");
    assert_eq!(tabs[0].panels[0].name, "good");
    assert_eq!(tabs[0].panels[0].commands[0].id, "plugin:good:run");
}

#[test]
fn a_plugin_log_is_kept_for_the_window() {
    let root = folder();
    let plugins = Plugins::load_from(Some(root.path()));
    let log = Arc::new(Log::new());
    log.note("good", Level::Info, "hello");
    let runtime = runtime("forward.wat");
    let mut s = Session::new();
    let request = json!({"command": "save", "params": {"path": "/nonexistent/x.bimcraft"}});
    let _ = runtime.invoke(&mut s, "good", "run", false, &request, &log);
    assert!(log.of("good").iter().any(|e| e.level == Level::Warning), "{:?}", log.of("good"));
    assert!(!plugins.log().is_empty());
}
