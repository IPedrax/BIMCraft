//! A script tried and taken back (`Session::script_dry_run`): what it would make, change and take away, and the warnings the
//! model would have, with the project, its history and its files left as they were.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::engine::Session;

/// A room in four walls, and an undo step to take back.
fn house() -> Session {
    let mut s = Session::new();
    s.script(
        "wall {\"points\": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], \"closed\": true}\n\
         room {\"point\": [4000, 3000], \"name\": \"Living\"}",
    )
    .unwrap();
    s
}

/// The undo history's labels, owned, so the session may run on while they're held.
fn history(s: &Session) -> Vec<String> {
    s.history().into_iter().map(str::to_string).collect()
}

fn ids(v: &Value) -> Vec<u64> {
    v.as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap()).collect()
}

fn categories(v: &Value) -> Vec<&str> {
    v.as_array().unwrap().iter().map(|e| e["category"].as_str().unwrap()).collect()
}

#[test]
fn a_dry_run_says_what_a_script_would_do_and_leaves_the_project_as_it_was() {
    let mut s = house();
    let (model, steps) = (Arc::clone(&s.model), history(&s));
    let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    let room = s.model.elements.iter().find(|(_, e)| e.category() == "room").map(|(id, _)| *id).unwrap();
    let text = format!(
        "level {{\"elevation\": 3000}}\n\
         modify {{\"id\": {room}, \"name\": \"Lounge\"}}\n\
         delete {{\"ids\": [{wall}]}}"
    );
    let r = s.script_dry_run(&text).unwrap();
    assert_eq!(r["dry_run"], true);
    assert_eq!(r["results"].as_array().unwrap().len(), 3, "each line's result, as a script gives them");
    assert_eq!(categories(&r["made"]), ["level"]);
    assert!(ids(&r["changed"]).contains(&room), "{r}");
    assert_eq!(ids(&r["removed"]), [wall]);
    assert_eq!(r["removed"][0]["guid"], json!(s.model.elements.guid(wall).unwrap()), "what goes is named by its GUID too");
    let warnings = r["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("not enclosed")), "{r}");
    // Nothing of it stays: the very model, the history, and the next id.
    assert!(Arc::ptr_eq(&s.model, &model));
    assert_eq!(history(&s), steps);
    assert!(s.model.warnings().is_empty());
    // Undo still takes back the step before the dry run.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.elements.iter().filter(|(_, e)| e.category() == "room").count(), 0);
}

#[test]
fn a_dry_run_that_fails_names_the_line_and_changes_nothing() {
    let mut s = house();
    let model = Arc::clone(&s.model);
    let e = s.script_dry_run("level {\"elevation\": 3000}\nnope").unwrap_err();
    assert!(e.contains("line 2"), "{e}");
    assert!(Arc::ptr_eq(&s.model, &model));
}

#[test]
fn a_dry_run_opens_and_writes_no_file_and_keeps_the_history() {
    let mut s = house();
    let (model, steps) = (Arc::clone(&s.model), history(&s));
    let dir = std::env::temp_dir().join(format!("bimcraft-dry-run-{}", std::process::id()));
    let file = dir.join("never.bimcraft");
    for line in [
        format!("save {{\"path\": {}}}", json!(file)),
        "save".to_string(),
        "undo".to_string(),
        "new".to_string(),
        format!("save {{\"path\": {}}}", json!(dir.join("never.ifc"))),
    ] {
        let e = s.script_dry_run(&format!("level {{\"elevation\": 3000}}\n{line}")).unwrap_err();
        assert!(e.contains("line 2") && e.contains("dry run"), "{line}: {e}");
    }
    assert!(!file.exists() && !dir.join("never.ifc").exists());
    assert!(Arc::ptr_eq(&s.model, &model));
    assert_eq!(history(&s), steps);
}

#[test]
fn a_session_that_may_only_read_may_still_try_a_script() {
    let mut s = house();
    s.read_only = Some("the test lets it edit");
    let model = Arc::clone(&s.model);
    let r = s.script_dry_run("level {\"elevation\": 3000}").unwrap();
    assert_eq!(categories(&r["made"]), ["level"]);
    assert!(Arc::ptr_eq(&s.model, &model));
    assert!(s.execute("level", &json!({"elevation": 3000})).is_err(), "and still may not edit");
}

#[test]
fn a_dry_run_of_something_that_changes_no_element_says_so() {
    let mut s = house();
    let r = s.script_dry_run("phase {\"name\": \"Later\"}\ninspect").unwrap();
    assert_eq!((ids(&r["made"]).len(), ids(&r["changed"]).len(), ids(&r["removed"]).len()), (0, 0, 0));
    assert_eq!(r["other_changes"], true, "the phases changed, which no element list shows: {r}");
    let r = s.script_dry_run("inspect").unwrap();
    assert_eq!(r["other_changes"], false);
}

#[test]
fn a_dry_run_looks_at_each_command_as_it_runs_with_the_params_it_is_given() {
    // A file a command reaches only deep in its params, which no look at the line's first word finds: refused as the line
    // comes, before it runs, and the line before it is taken back with the rest.
    let mut s = house();
    let (model, steps) = (Arc::clone(&s.model), history(&s));
    let picture = std::env::temp_dir().join(format!("bimcraft-dry-run-texture-{}.png", std::process::id()));
    let text = format!("level {{\"elevation\": 3000}}\nmaterial {{\"name\": \"Brick\", \"texture\": {{\"path\": {}}}}}", json!(picture));
    let e = s.script_dry_run(&text).unwrap_err();
    assert!(e.contains("line 2") && e.contains("a file") && e.contains("dry run"), "{e}");
    assert!(Arc::ptr_eq(&s.model, &model));
    assert_eq!(history(&s), steps);
    // And once the dry run is over, the session runs every command again.
    s.execute("level", &json!({"elevation": 3000})).unwrap();
}
