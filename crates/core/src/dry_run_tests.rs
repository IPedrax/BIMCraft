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

/// What the favorites are where no `path` is given ([`crate::favorites::default_path`]): in the user's own folder, outside the
/// project. Run in a child of this test with `BIMCRAFT_FAVORITES` at a file of the test's own, so the user's are never touched,
/// even by a dry run that wrongly let a line through.
#[test]
fn a_dry_run_leaves_the_users_favorites_as_they_were() {
    const CHILD: &str = "BIMCRAFT_DRY_RUN_FAVORITES_TEST";
    if let Some(file) = std::env::var_os(CHILD).map(std::path::PathBuf::from) {
        assert_eq!(crate::favorites::default_path(), Some(file.clone()), "the favorites are the test's, not the user's");
        let mut s = house();
        s.execute("material", &json!({"name": "My Oak", "color": "#a0703c"})).unwrap();
        // A material called `save`, whose GUID a command reads as its name: an `action` that is `save` once it runs.
        s.execute("material", &json!({"name": "save", "color": "#808080"})).unwrap();
        let save = s.model.library.guid(&crate::library::Kind::Material, "save").unwrap().to_string();
        let refused = |s: &mut Session, line: &str| {
            let e = s.script_dry_run(&format!("level {{\"elevation\": 3000}}\n{line}")).unwrap_err();
            assert!(e.contains("line 2 (favorite)") && e.contains("would write a file") && e.contains("dry run"), "{line}: {e}");
            e
        };
        // None kept yet: saving one is refused and makes no file.
        let e = refused(&mut s, "favorite {\"action\": \"save\", \"name\": \"My Oak\"}");
        assert!(e.contains("run the script without `dry_run`"), "it says what to do: {e}");
        refused(&mut s, "favorite {\"action\": \"save\", \"kind\": \"material\", \"name\": \"My Oak\", \"as\": \"Oak\"}");
        assert!(!file.exists(), "a dry run wrote the favorites");
        // One kept: deleting it, or saving over it, is refused and leaves the file as it was.
        s.execute("favorite", &json!({"action": "save", "name": "My Oak"})).unwrap();
        let saved = s.execute("favorite", &json!({"action": save, "name": "My Oak", "as": "Oak by GUID"})).unwrap();
        assert_eq!(saved["saved"], "Oak by GUID", "the GUID saves, run for real: {saved}");
        let kept = std::fs::read(&file).unwrap();
        let (model, steps) = (Arc::clone(&s.model), history(&s));
        assert!(refused(&mut s, "favorite {\"action\": \"delete\", \"name\": \"My Oak\"}").contains("would write a file"));
        refused(&mut s, "favorite {\"action\": \"save\", \"name\": \"My Oak\"}");
        // Whatever else a line asks of them (bringing one in, an action spelt otherwise) is refused too: a dry run only lists.
        refused(&mut s, "favorite {\"action\": \"load\", \"name\": \"My Oak\", \"as\": \"Oak 2\"}");
        refused(&mut s, "favorite {\"action\": \"Delete\", \"name\": \"My Oak\"}");
        refused(&mut s, &format!("favorite {{\"action\": \"{save}\", \"name\": \"My Oak\", \"as\": \"Oak again\"}}"));
        assert_eq!(std::fs::read(&file).unwrap(), kept, "a dry run changed the favorites");
        assert!(Arc::ptr_eq(&s.model, &model));
        assert_eq!(history(&s), steps);
        // Listing them only reads, which a dry run may.
        let r = s.script_dry_run("favorite\nfavorite {\"action\": \"list\", \"kind\": \"material\"}").unwrap();
        assert_eq!(r["results"][0]["materials"][0]["name"], "My Oak", "{r}");
        assert_eq!(r["results"][1]["materials"][0]["name"], "My Oak", "{r}");
        assert_eq!(std::fs::read(&file).unwrap(), kept);
        return;
    }
    let dir = std::env::temp_dir().join(format!("bimcraft-dry-run-favorites-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("favorites.json");
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "dry_run_tests::a_dry_run_leaves_the_users_favorites_as_they_were", "--nocapture", "--test-threads=1"])
        .env(CHILD, &file)
        .env("BIMCRAFT_FAVORITES", &file)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{said}{}", String::from_utf8_lossy(&out.stderr));
    assert!(said.contains("1 passed"), "the child ran the test: {said}");
}
