//! The example plugin in `plugins/area-summary`, built with the guest SDK into `area_summary.wasm` and run by the same
//! host a user's plugin runs in: what `docs/areas/plugins.md` tells a new author works end to end.

// A test target of its own: the workspace's lints allow these in `#[cfg(test)]` modules, which an integration
// test's whole crate is.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use bimcraft_core::engine::Session;
use bimcraft_core::sample::SAMPLE;
use bimcraft_plugin::{Plugins, Status};
use serde_json::{Value, json};

fn sample_folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn session_with_sample() -> Session {
    let mut s = Session::new();
    s.script(SAMPLE).unwrap();
    s
}

#[test]
fn the_example_plugin_is_loaded_and_its_functions_run_the_model() {
    let plugins = Plugins::load_from(Some(&sample_folder()));
    let sample = plugins.plugins.iter().find(|p| p.manifest.id == "area-summary").expect("the example plugin");
    assert_eq!(sample.status, Status::Loaded, "{:?}", sample.status);
    let mut s = session_with_sample();
    plugins.register(&mut s);
    assert!(s.external_command("plugin:area-summary:summary").is_some());

    // Reading: no undo step, the rooms' areas summed from the model.
    let before = s.history().len();
    let summary = s.execute("plugin:area-summary:summary", &json!({})).unwrap();
    assert!(summary["rooms"].as_u64().is_some_and(|n| n >= 4), "{summary}");
    assert!(summary["total_area_m2"].as_f64().is_some_and(|a| a > 0.0), "{summary}");
    assert_eq!(s.history().len(), before, "a read makes no undo step");

    // One command: one undo step, which takes back the note it wrote.
    let texts = |s: &Session| s.model.elements.values().filter(|e| matches!(e, bimcraft_core::model::Element::Text(_))).count();
    let texts_before = texts(&s);
    let written: Value = s.execute("plugin:area-summary:write_note", &json!({"at": [0, 0]})).unwrap();
    assert!(written["note"].as_u64().is_some(), "{written}");
    assert_eq!(s.history().len(), before + 1, "one step");
    assert_eq!(texts(&s), texts_before + 1, "the note is in the model");
    assert_eq!(s.history().last().copied(), Some("Area Summary: Write Area Note"));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(texts(&s), texts_before, "the whole call is taken back");

    // A project with no rooms is the plugin's own error: the call fails, and nothing changes.
    let mut empty = Session::new();
    plugins.register(&mut empty);
    let (model, steps) = ((*empty.model).clone(), empty.history().len());
    assert!(empty.execute("plugin:area-summary:write_note", &json!({"at": [0, 0]})).is_err());
    assert_eq!(empty.history().len(), steps);
    assert_eq!(*empty.model, model);
}

#[test]
fn a_plugin_command_obeys_a_session_that_may_only_read() {
    let plugins = Plugins::load_from(Some(&sample_folder()));
    let mut s = session_with_sample();
    plugins.register(&mut s);
    s.read_only = Some("the test says so");
    // A read answers.
    let summary = s.execute("plugin:area-summary:summary", &json!({})).unwrap();
    assert!(summary["rooms"].as_u64().is_some_and(|n| n >= 4), "{summary}");
    // A change is refused after the fact, and the whole call is taken back.
    let (model, history) = ((*s.model).clone(), s.history().len());
    let e = s.execute("plugin:area-summary:write_note", &json!({"at": [0, 0]})).unwrap_err();
    assert!(e.contains("read-only for agents"), "{e}");
    assert_eq!(s.history().len(), history);
    assert_eq!(*s.model, model);
}
