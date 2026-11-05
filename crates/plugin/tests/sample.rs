//! The guest SDK end to end: a plugin built with `bimcraft-plugin-sdk` is run by the same host a user's plugin runs in.
//!
//! The module isn't in the repository: `crates/plugin/tests/sample-plugin` is built for `wasm32-unknown-unknown`
//! before the tests run, and they load that build (`bimcraft_plugin::testing`). Without it — no target, no build —
//! they are skipped with what to run; in CI a skip is a failure, so the build is always checked there.

// A test target of its own: the workspace's lints allow these in `#[cfg(test)]` modules, which an integration
// test's whole crate is.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use bimcraft_core::engine::Session;
use bimcraft_core::sample::SAMPLE;
use bimcraft_plugin::testing::sample_wasm_or_skip;
use bimcraft_plugin::{Plugins, Status};
use serde_json::{Value, json};

/// A folder with the built module and the manifest the tests drive it by, or none when it wasn't built.
fn plugins() -> Option<(Plugins, tempfile::TempDir)> {
    let module = sample_wasm_or_skip()?;
    let dir = tempfile::tempdir().unwrap();
    let manifest = json!({
        "id": "sample",
        "version": "0.1.0",
        "name": "Sample",
        "description": "The rooms' areas, read by the guest SDK.",
        "abi": 1,
        "module": "sample_plugin.wasm",
        "functions": [
            {"id": "summary", "label": "Summary", "tab": "Sample"},
            {"id": "write_note", "label": "Write Note", "tab": "Sample"},
            {"id": "fail", "label": "Fail", "tab": "Sample"}
        ]
    });
    let path = dir.path().join("sample");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("plugin.json"), serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    // The plugin's module is the built one, placed where its manifest names it.
    let plugins = Plugins::load_from_with_module(Some(dir.path()), module);
    Some((plugins, dir))
}

fn session_with_sample(plugins: &Plugins) -> Session {
    let mut s = Session::new();
    s.script(SAMPLE).unwrap();
    plugins.register(&mut s);
    s
}

#[test]
fn the_sdk_s_built_plugin_is_loaded_and_its_functions_run_the_model() {
    let Some((plugins, _dir)) = plugins() else { return };
    let sample = plugins.plugins.iter().find(|p| p.manifest.id == "sample").expect("the sample plugin");
    assert_eq!(sample.status, Status::Loaded, "{:?}", sample.status);
    let mut s = session_with_sample(&plugins);
    assert!(s.external_command("plugin:sample:summary").is_some());

    // Reading: no undo step, the rooms' areas summed from the model.
    let before = s.history().len();
    let summary = s.execute("plugin:sample:summary", &json!({})).unwrap();
    assert!(summary["rooms"].as_u64().is_some_and(|n| n >= 4), "{summary}");
    assert!(summary["total_area_m2"].as_f64().is_some_and(|a| a > 0.0), "{summary}");
    assert_eq!(s.history().len(), before, "a read makes no undo step");

    // One command: one undo step, which takes back the note it wrote.
    let texts = |s: &Session| s.model.elements.values().filter(|e| matches!(e, bimcraft_core::model::Element::Text(_))).count();
    let texts_before = texts(&s);
    let written: Value = s.execute("plugin:sample:write_note", &json!({"at": [0, 0]})).unwrap();
    assert!(written["note"].as_u64().is_some(), "{written}");
    assert_eq!(s.history().len(), before + 1, "one step");
    assert_eq!(texts(&s), texts_before + 1, "the note is in the model");
    assert_eq!(s.history().last().copied(), Some("Sample: Write Note"));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(texts(&s), texts_before, "the whole call is taken back");

    // A project with no rooms is the plugin's own error: the call fails, and nothing changes.
    let mut empty = Session::new();
    plugins.register(&mut empty);
    let (model, steps) = ((*empty.model).clone(), empty.history().len());
    assert!(empty.execute("plugin:sample:write_note", &json!({"at": [0, 0]})).is_err());
    assert_eq!(empty.history().len(), steps);
    assert_eq!(*empty.model, model);
}

#[test]
fn a_change_a_failed_call_made_is_taken_back() {
    let Some((plugins, _dir)) = plugins() else { return };
    let mut s = session_with_sample(&plugins);
    let (model, steps) = ((*s.model).clone(), s.history().len());
    let e = s.execute("plugin:sample:fail", &json!({})).unwrap_err();
    assert!(e.contains("then failed"), "{e}");
    assert_eq!(s.history().len(), steps, "no step is left behind");
    assert_eq!(*s.model, model, "the level it made is gone");
}

#[test]
fn a_plugin_command_obeys_a_session_that_may_only_read() {
    let Some((plugins, _dir)) = plugins() else { return };
    let mut s = session_with_sample(&plugins);
    s.read_only = Some("the test says so");
    // A read answers.
    let summary = s.execute("plugin:sample:summary", &json!({})).unwrap();
    assert!(summary["rooms"].as_u64().is_some_and(|n| n >= 4), "{summary}");
    // A change is refused after the fact, and the whole call is taken back.
    let (model, history) = ((*s.model).clone(), s.history().len());
    let e = s.execute("plugin:sample:write_note", &json!({"at": [0, 0]})).unwrap_err();
    assert!(e.contains("read-only for agents"), "{e}");
    assert_eq!(s.history().len(), history);
    assert_eq!(*s.model, model);
}
