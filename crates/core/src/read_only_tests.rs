//! A session that may only read (`Session::read_only`): the window's agents when the user lets them only read, and
//! `bimcraft-cli mcp --read-only`. What would change the project, replace it, step through its history or write over its
//! file is refused and leaves it as it was; what reads it answers.

use std::sync::Arc;

use serde_json::json;

use crate::engine::{Session, commands};

/// What lets the test's session edit, which each refusal ends with.
const HOW: &str = "the test lets it edit";

/// A house with a door, a room and a sheet, an undo step to take back, saved to a file of its own; and that file.
fn house(name: &str) -> (Session, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("bimcraft-read-only-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("house.bimcraft");
    let mut s = Session::new();
    s.script(
        "wall {\"points\": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], \"closed\": true}\n\
         door {\"at\": [2000, 0]}\n\
         room {\"point\": [4000, 3000], \"name\": \"Living\"}\n\
         sheet {}\n\
         place {\"sheet\": \"A101\", \"view\": \"Level 1\"}",
    )
    .unwrap();
    s.execute("save", &json!({"path": file})).unwrap();
    (s, file)
}

/// What a session that may only read must keep: its project, its undo and redo history, its file, the phase and option it makes
/// things in.
type Kept = (Arc<crate::model::Model>, Vec<String>, usize, Option<std::path::PathBuf>, Option<String>, Option<String>);

fn kept(s: &Session) -> Kept {
    (
        Arc::clone(&s.model),
        s.history().into_iter().map(str::to_string).collect(),
        s.steps(true).len(),
        s.path.clone(),
        s.phase.clone(),
        s.option.clone(),
    )
}

fn same(s: &Session, k: &Kept) {
    assert!(Arc::ptr_eq(&k.0, &s.model) || *k.0 == *s.model, "the project changed");
    let history: Vec<String> = s.history().into_iter().map(str::to_string).collect();
    assert_eq!((&k.1, k.2, &k.3, &k.4, &k.5), (&history, s.steps(true).len(), &s.path, &s.phase, &s.option));
}

#[test]
fn a_session_that_may_only_read_refuses_what_would_change_the_project_and_changes_nothing() {
    let (mut s, file) = house("refuses");
    let saved = std::fs::read(&file).unwrap();
    // An undo step taken back, so that redo has one too.
    s.execute("level", &json!({"elevation": 6000})).unwrap();
    s.execute("undo", &json!({})).unwrap();
    s.read_only = Some(HOW);
    let before = kept(&s);
    let wall = json!({"points": [[0, 9000], [4000, 9000]]});
    for (command, params) in [
        ("wall", wall.clone()),
        ("WA", wall.clone()),
        ("delete", json!({"ids": [100002]})),
        ("modify", json!({"id": 100002, "height": 2500})),
        ("schedule", json!({"name": "Doors by Level", "category": "doors"})),
        ("undo", json!({})),
        ("redo", json!({})),
        ("open", json!({"path": file})),
        ("new", json!({})),
        ("highrise", json!({"storeys": 1})),
        ("recover", json!({"path": file})),
        ("insert", json!({"path": file})),
        ("phase", json!({"now": "Existing"})),
        // Its own file is not written over, by a save or by anything else given its path.
        ("save", json!({})),
        ("save", json!({"path": file})),
        ("save", json!({"path": file.parent().unwrap().join(".").join("house.bimcraft")})),
        ("render", json!({"view": "Level 1", "width": 16, "height": 16, "path": file})),
        ("schedule", json!({"category": "doors", "path": file})),
        ("export", json!({"format": "pdf", "view": "A101", "path": file})),
    ] {
        let e = s.execute(command, &params).unwrap_err();
        assert!(e.contains("read-only for agents") && e.ends_with(HOW), "{command} {params}: {e}");
        same(&s, &before);
    }
    // What each says: what it would do, and what lets it.
    let e = s.execute("wall", &wall).unwrap_err();
    assert!(e.starts_with("`wall` would change the project"), "{e}");
    assert!(s.execute("open", &json!({"path": file})).unwrap_err().starts_with("`open` would replace the project"));
    assert!(s.execute("save", &json!({})).unwrap_err().starts_with("`save` would write over the project's own file"));
    // A script holding an edit stops at it, by its line, and an atomic one keeps nothing of it.
    let text = "inspect\nwall {\"points\": [[0, 9000], [4000, 9000]]}\ninspect";
    let e = s.script(text).unwrap_err();
    assert!(e.starts_with("line 2 (wall): `wall` would change the project"), "{e}");
    same(&s, &before);
    let e = s.script_atomic(text).unwrap_err();
    assert!(e.contains("read-only for agents"), "{e}");
    same(&s, &before);
    assert_eq!(std::fs::read(&file).unwrap(), saved, "the project's file is as it was");
    // Let edit again, the same session does.
    s.read_only = None;
    s.execute("wall", &wall).unwrap();
    assert_eq!(s.history().last(), Some(&"Wall"));
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn a_session_that_may_only_read_reads_schedules_renders_and_exports_to_new_files() {
    let (mut s, file) = house("reads");
    let dir = file.parent().unwrap().to_path_buf();
    s.read_only = Some(HOW);
    let before = kept(&s);
    let counts = s.execute("inspect", &json!({})).unwrap();
    assert_eq!((counts["counts"]["wall"].as_u64(), counts["counts"]["door"].as_u64()), (Some(4), Some(1)), "{counts}");
    assert_eq!(s.execute("elements", &json!({"category": "rooms"})).unwrap()["elements"][0]["name"], "Living");
    // A schedule of the project's is read, and its quantities written to a file of their own.
    let doors = s.execute("schedule", &json!({"category": "doors", "path": dir.join("doors.csv")})).unwrap();
    assert_eq!(doors["rows"].as_array().map(Vec::len), Some(1), "{doors}");
    assert!(crate::bimc::read_to_string(dir.join("doors.csv")).unwrap().lines().count() >= 2);
    let plan = s.execute("render", &json!({"view": "Level 1", "width": 64, "height": 64})).unwrap();
    assert!(plan["pngBase64"].is_string(), "{plan}");
    // Exports, the sheets to PDF among them; a copy of the project saved as another file leaves the session with its own.
    for name in ["house.ifc", "sheets.pdf", "plan.png", "copy.bimcraft"] {
        let to = dir.join(name);
        s.execute("save", &json!({"path": to})).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(std::fs::metadata(&to).is_ok_and(|m| m.len() > 0), "{name} is written");
    }
    assert!(std::fs::read(dir.join("sheets.pdf")).unwrap().starts_with(b"%PDF"));
    // Views and sheets exported to new files: one at a path, a file each in a folder.
    s.execute("export", &json!({"format": "pdf", "view": "A101", "path": dir.join("a101.pdf")})).unwrap();
    let r = s.execute("export", &json!({"format": "svg", "views": ["Level 1", "A101"], "folder": dir.join("out")})).unwrap();
    assert_eq!(r["written"].as_array().map(Vec::len), Some(2), "{r}");
    assert!(s.execute("export_formats", &json!({})).is_ok());
    // What only reports reads: the project's families, its technical standard, its phases.
    for (command, params) in
        [("family", json!({})), ("standard", json!({})), ("phase", json!({})), ("measure", json!({"from": [0, 0], "to": [3000, 4000]}))]
    {
        s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"));
    }
    // A script of reads runs, all or nothing too.
    assert_eq!(s.script_atomic("inspect\nschedule {\"category\": \"rooms\"}").unwrap().len(), 2);
    same(&s, &before);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn whatever_a_command_is_given_a_session_that_may_only_read_keeps_its_project() {
    // Every command, given what makes most of them do something, on a house: each one answers or is refused, and the project,
    // its history, its file and what the session makes things in are as they were. A command added later is in it too.
    let (mut base, file) = house("every");
    base.execute("level", &json!({"elevation": 6000})).unwrap();
    base.execute("undo", &json!({})).unwrap();
    base.read_only = Some(HOW);
    let payloads = [
        json!({}),
        json!({"points": [[0, 9000], [4000, 9000]]}),
        json!({"points": [[0, 9000], [4000, 9000], [4000, 12000]], "closed": true}),
        json!({"at": [2000, 0]}),
        json!({"at": [4000, 3000]}),
        json!({"point": [4000, 3000]}),
        json!({"id": 100002}),
        json!({"id": 100002, "height": 2500}),
        json!({"ids": [100002]}),
        json!({"ids": [100002, 100003], "by": [100, 0]}),
        json!({"name": "Read"}),
        json!({"name": "Read", "category": "doors"}),
        json!({"view": 100000}),
        json!({"view": "Level 1", "scale": 50}),
        json!({"elevation": 6000}),
        json!({"start": [0, 9000], "end": [4000, 9000]}),
        json!({"boundary": [[0, 0], [1000, 0], [1000, 1000]]}),
        json!({"text": "x", "at": [0, 0]}),
        json!({"now": "Existing"}),
        json!({"count": 1}),
    ];
    let before = kept(&base);
    let cmds: Vec<_> = commands().iter().collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let (answered, refused) = std::thread::scope(|scope| {
        let shares: Vec<_> = (0..threads)
            .map(|share| {
                let (cmds, payloads, base, before) = (&cmds, &payloads, &base, &before);
                scope.spawn(move || {
                    let (mut answered, mut refused) = (0, 0);
                    for cmd in cmds.iter().skip(share).step_by(threads) {
                        for p in payloads {
                            // A picture as small as it comes: the default size would take the test's time.
                            let mut p = p.clone();
                            if cmd.id == "render" {
                                p["width"] = json!(16);
                                p["height"] = json!(16);
                            }
                            let mut s = base.clone();
                            match s.execute(cmd.id, &p) {
                                Ok(_) => answered += 1,
                                Err(e) if e.contains("read-only for agents") => refused += 1,
                                Err(_) => {}
                            }
                            assert!(Arc::ptr_eq(&before.0, &s.model) || *before.0 == *s.model, "{} {p} changed the project", cmd.id);
                            let history: Vec<String> = s.history().into_iter().map(str::to_string).collect();
                            assert_eq!(
                                (&before.1, before.2, &before.3, &before.4, &before.5),
                                (&history, s.steps(true).len(), &s.path, &s.phase, &s.option),
                                "{} {p}",
                                cmd.id
                            );
                        }
                    }
                    (answered, refused)
                })
            })
            .collect();
        shares.into_iter().map(|h| h.join().unwrap()).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
    });
    assert!(answered > 100 && refused > 100, "the sweep reads and is refused: {answered} answered, {refused} refused");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn reading_lends_a_session_read_only_for_what_it_runs_and_gives_it_back() {
    let (mut s, file) = house("reading");
    let before = kept(&s);
    let wall = json!({"points": [[0, 9000], [4000, 9000]]});
    let e = s.reading(HOW, |s| s.execute("wall", &wall)).unwrap_err();
    assert!(e.contains("read-only for agents") && e.ends_with(HOW), "{e}");
    let counts = s.reading(HOW, |s| s.execute("inspect", &json!({}))).unwrap();
    assert_eq!(counts["counts"]["wall"], 4);
    // What it runs is held to it however it gets at the session, not only by a command.
    let e = s.reading(HOW, |s| {
        s.model_mut().name = "Changed".into();
        Ok(())
    });
    assert!(e.is_err_and(|e| e.contains("read-only for agents")));
    same(&s, &before);
    assert_eq!(s.read_only, None, "given back");
    s.execute("wall", &wall).unwrap();
    // A session that may only read keeps what it was told lets it edit.
    s.read_only = Some("the server's");
    let e = s.reading(HOW, |s| s.execute("wall", &wall)).unwrap_err();
    assert!(e.ends_with("the server's"), "{e}");
    assert_eq!(s.read_only, Some("the server's"));
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

/// A command refused for changing the project puts back the model it found, and with it that the model passed the check: the
/// next command checks only what it changes, as it would have.
#[test]
fn a_refused_change_leaves_the_model_checked_as_it_was() {
    let (mut s, _) = house("verified");
    s.execute("move", &json!({"ids": [], "by": [0, 0]})).ok();
    assert!(s.verified_now());
    s.read_only = Some(HOW);
    let e = s.execute("wall", &json!({"points": [[0, 9000], [3000, 9000]]})).unwrap_err();
    assert!(e.contains("would change the project"), "{e}");
    assert!(s.verified_now(), "the model put back isn't the checked one");
}
