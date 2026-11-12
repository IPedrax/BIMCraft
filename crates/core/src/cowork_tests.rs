//! Co-Work's engine side (`cowork.rs`): a guest's copy kept in step by deltas alone, what a guest's command may do on the host,
//! and each person undoing their own edits.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::cowork::{Delta, Put, apply, delta, run_for_guest};
use crate::engine::Session;
use crate::model::{Id, Model};

/// The sample house, as a host opens it.
fn house() -> Session {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    s
}

/// A wall along y, well away from the house; its id.
fn wall(s: &mut Session, y: i64) -> Id {
    let id = s.model.next_id;
    s.execute("wall", &json!({"points": [[0, y], [6000, y]]})).unwrap();
    assert!(s.model.elements.contains_key(id), "the wall is {id}");
    id
}

/// `cmd` run by `who`, as the host runs a session's commands.
fn by(s: &mut Session, who: &str, cmd: &str, params: Value) -> Result<Value, String> {
    s.author = Some(who.into());
    s.execute(cmd, &params)
}

fn json_of(m: &Model) -> String {
    m.to_json().unwrap()
}

#[test]
fn a_copy_fed_only_deltas_stays_the_hosts_model() {
    let mut host = house();
    let mut copy = (*host.model).clone();
    let w = host.model.next_id;
    let steps: Vec<(&str, Value, bool)> = vec![
        ("wall", json!({"points": [[0, 40000], [6000, 40000]]}), true),
        ("door", json!({"at": [3000, 40000]}), true),
        ("move", json!({"ids": [w], "by": [0, 500]}), true),
        ("type", json!({"category": "wall", "name": "Co-Work 150", "thickness": 150}), false),
        ("wall", json!({"points": [[0, 46000], [6000, 46000]], "type": "Co-Work 150"}), true),
        ("undo", json!({}), true),
        ("redo", json!({}), true),
        ("delete", json!({"ids": [w]}), true),
        ("undo", json!({"count": 2}), true),
    ];
    for (cmd, params, only_elements) in steps {
        let before = Arc::clone(&host.model);
        host.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd} {params}: {e}"));
        let d = delta(&before, &host.model).unwrap();
        if only_elements {
            assert!(matches!(d, Delta::Elements { .. }), "{cmd} changed only elements, so only they travel");
        } else {
            assert!(matches!(d, Delta::Whole { .. }), "{cmd} changed a type, so the whole model travels");
        }
        // Over the wire, as JSON.
        let d: Delta = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        copy = apply(&copy, &d).unwrap_or_else(|e| panic!("{cmd}: {e}"));
        assert_eq!(json_of(&copy), json_of(&host.model), "after {cmd} {params}, the copy is the host's model");
    }
}

#[test]
fn nothing_changed_is_an_empty_delta() {
    let host = house();
    let d = delta(&host.model, &host.model).unwrap();
    assert_eq!(d, Delta::Elements { next_id: host.model.next_id, put: vec![], gone: vec![] });
    assert_eq!(json_of(&apply(&host.model, &d).unwrap()), json_of(&host.model));
}

#[test]
fn a_delta_that_does_not_fit_never_changes_the_copy_and_says_why() {
    let mut host = house();
    let copy = (*host.model).clone();
    let wall_id = wall(&mut host, 40000);
    let door_id = host.model.next_id;
    host.execute("door", &json!({"at": [3000, 40000]})).unwrap();
    let with_door = (*host.model).clone();
    let guid = |m: &Model, id: Id| m.elements.guid(id).unwrap();
    let put = |m: &Model, id: Id| Put { id, guid: guid(m, id), element: m.elements.get(id).unwrap().clone() };
    let any = with_door.elements.iter().map(|(id, _)| *id).find(|id| *id != wall_id && *id != door_id).unwrap();
    let next = with_door.next_id;
    let bad = [
        // Gone, but the copy never had it.
        Delta::Elements { next_id: copy.next_id, put: vec![], gone: vec![wall_id] },
        // Another element's GUID.
        Delta::Elements { next_id: next, put: vec![Put { guid: guid(&copy, any), ..put(&with_door, wall_id) }], gone: vec![] },
        // A door with no wall to stand in.
        Delta::Elements { next_id: next, put: vec![put(&with_door, door_id)], gone: vec![] },
        // The next id behind one the copy has, or past where a project's ids end.
        Delta::Elements { next_id: 1, put: vec![], gone: vec![] },
        Delta::Elements { next_id: Id::MAX, put: vec![], gone: vec![] },
        Delta::Elements { next_id: next, put: vec![Put { id: crate::ids::LINKED + 5, ..put(&with_door, wall_id) }], gone: vec![] },
        // Not a model at all.
        Delta::Whole { model: "{not json".into() },
        Delta::Whole { model: "[]".into() },
    ];
    for d in bad {
        let kept = json_of(&copy);
        let e = apply(&copy, &d).expect_err(&format!("{d:?}"));
        assert!(!e.is_empty());
        assert_eq!(json_of(&copy), kept, "the copy is as it was");
    }
    // What isn't a delta is refused before it's one.
    for text in [
        r#"{"kind": "elements", "next_id": 5, "put": [], "gone": [], "extra": 1}"#,
        r#"{"kind": "everything"}"#,
        r#"{"kind": "whole"}"#,
        r#"{"kind": "elements", "next_id": -1, "put": [], "gone": []}"#,
    ] {
        assert!(serde_json::from_str::<Delta>(text).is_err(), "{text}");
    }
}

#[test]
fn a_guests_command_never_touches_the_hosts_disk_nor_replaces_the_project() {
    let mut s = house();
    s.author = Some("Ana".into());
    let w = wall(&mut s, 40000);
    let dir = std::env::temp_dir().join(format!("bimcraft-cowork-{}", std::process::id()));
    let file = dir.join("taken.bimcraft");
    let kept = Arc::clone(&s.model);
    let none = (None, None);
    for (cmd, params, says) in [
        ("save", json!({"path": file.to_string_lossy()}), "file"),
        ("import_cad", json!({"path": file.to_string_lossy()}), "file"),
        ("modify", json!({"id": w, "path": "/etc/passwd"}), "file"),
        ("new", json!({}), "replace"),
        ("OPEN", json!({"path": file.to_string_lossy()}), "replace"),
    ] {
        let e = run_for_guest(&mut s, "Bo", cmd, &params, none.clone(), true).expect_err(cmd);
        assert!(e.contains(says), "{cmd}: {e}");
    }
    assert!(!file.exists(), "nothing was written");
    assert!(Arc::ptr_eq(&kept, &s.model), "nothing changed");
    // An edit goes through, as Bo's step; the session is the host's again after it.
    run_for_guest(&mut s, "Bo", "wall", &json!({"points": [[0, 46000], [6000, 46000]]}), (Some("Nowhere".into()), None), true).unwrap();
    assert_eq!(s.steps(false)[0].by.as_deref(), Some("Bo"));
    assert_eq!((s.author.as_deref(), s.phase.as_deref(), s.option.as_deref()), (Some("Ana"), None, None));
    // When the host lets guests only look, what reads answers and what edits is refused, saying so.
    let kept = Arc::clone(&s.model);
    let e = run_for_guest(&mut s, "Bo", "wall", &json!({"points": [[0, 52000], [6000, 52000]]}), none.clone(), false).unwrap_err();
    assert!(e.contains("only look"), "{e}");
    assert!(run_for_guest(&mut s, "Bo", "inspect", &json!({}), none, false).is_ok());
    assert!(Arc::ptr_eq(&kept, &s.model));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undo_takes_back_only_your_own_edits() {
    let mut s = house();
    s.author = Some("Ana".into());
    let a = wall(&mut s, 40000);
    let c = wall(&mut s, 43000);
    s.author = Some("Bo".into());
    let b = wall(&mut s, 46000);
    // Ana's last edit, under Bo's.
    by(&mut s, "Ana", "undo", json!({})).unwrap();
    assert!(s.model.elements.contains_key(a) && !s.model.elements.contains_key(c) && s.model.elements.contains_key(b));
    let top = s.steps(false)[0];
    assert!(top.label.starts_with("Undo") && top.by.as_deref() == Some("Ana"), "{top:?}");
    // Again: the one before it, past her own undo.
    by(&mut s, "Ana", "undo", json!({})).unwrap();
    assert!(!s.model.elements.contains_key(a) && s.model.elements.contains_key(b));
    let e = by(&mut s, "Ana", "undo", json!({})).unwrap_err();
    assert!(e.contains("nothing of yours"), "{e}");
    // Bo's is his.
    by(&mut s, "Bo", "undo", json!({})).unwrap();
    assert!(!s.model.elements.contains_key(b));
}

#[test]
fn undo_takes_back_several_of_your_edits_at_once_or_none() {
    let mut s = house();
    s.author = Some("Ana".into());
    let (a, c) = (wall(&mut s, 40000), wall(&mut s, 43000));
    s.author = Some("Bo".into());
    let b = wall(&mut s, 46000);
    by(&mut s, "Ana", "undo", json!({"count": 2})).unwrap();
    assert!(!s.model.elements.contains_key(a) && !s.model.elements.contains_key(c) && s.model.elements.contains_key(b));
    // Bo has one: two is refused whole.
    let kept = Arc::clone(&s.model);
    assert!(by(&mut s, "Bo", "undo", json!({"count": 2})).is_err());
    assert!(Arc::ptr_eq(&kept, &s.model) && s.model.elements.contains_key(b));
}

#[test]
fn an_undo_fails_and_names_who_changed_the_same_element_after_it() {
    let mut s = house();
    s.author = Some("Ana".into());
    let a = wall(&mut s, 40000);
    by(&mut s, "Bo", "move", json!({"ids": [a], "by": [0, 500]})).unwrap();
    let (kept, steps) = (Arc::clone(&s.model), s.history().len());
    let e = by(&mut s, "Ana", "undo", json!({})).unwrap_err();
    assert!(e.contains("Bo") && e.contains(&a.to_string()), "{e}");
    assert!(Arc::ptr_eq(&kept, &s.model) && s.history().len() == steps, "nothing changed");
    // Bo takes his back, and Ana's goes through.
    by(&mut s, "Bo", "undo", json!({})).unwrap();
    by(&mut s, "Ana", "undo", json!({})).unwrap();
    assert!(!s.model.elements.contains_key(a));
}

#[test]
fn an_undo_that_would_break_what_came_after_names_who_made_it() {
    let mut s = house();
    s.author = Some("Ana".into());
    let a = wall(&mut s, 40000);
    by(&mut s, "Bo", "door", json!({"at": [3000, 40000]})).unwrap();
    let kept = Arc::clone(&s.model);
    let e = by(&mut s, "Ana", "undo", json!({})).unwrap_err();
    assert!(e.contains("Bo"), "{e}");
    assert!(Arc::ptr_eq(&kept, &s.model) && s.model.elements.contains_key(a));
}

#[test]
fn redo_brings_back_what_your_undo_took_and_a_new_edit_of_yours_ends_it() {
    let mut s = house();
    s.author = Some("Ana".into());
    let a = wall(&mut s, 40000);
    let guid = s.model.elements.guid(a);
    by(&mut s, "Bo", "wall", json!({"points": [[0, 46000], [6000, 46000]]})).unwrap();
    by(&mut s, "Ana", "undo", json!({})).unwrap();
    assert!(!s.model.elements.contains_key(a));
    assert!(by(&mut s, "Bo", "redo", json!({})).unwrap_err().contains("nothing of yours"), "Bo undid nothing");
    by(&mut s, "Ana", "redo", json!({})).unwrap();
    assert_eq!(s.model.elements.guid(a), guid, "the same wall, by id and GUID");
    assert!(s.steps(false)[0].label.starts_with("Redo"));
    by(&mut s, "Ana", "undo", json!({})).unwrap();
    assert!(!s.model.elements.contains_key(a));
    wall(&mut s, 52000);
    let e = by(&mut s, "Ana", "redo", json!({})).unwrap_err();
    assert!(e.contains("nothing of yours"), "{e}");
}

#[test]
fn a_settings_change_is_undone_unless_someone_changed_settings_after_it() {
    let mut s = house();
    let types = |s: &Session| s.model.types.wall.len();
    let n = types(&s);
    by(&mut s, "Ana", "type", json!({"category": "wall", "name": "Ana 150", "thickness": 150})).unwrap();
    let b = {
        s.author = Some("Bo".into());
        wall(&mut s, 46000)
    };
    by(&mut s, "Ana", "undo", json!({})).unwrap();
    assert_eq!(types(&s), n, "her type is gone");
    assert!(s.model.elements.contains_key(b), "Bo's wall stays");
    by(&mut s, "Ana", "type", json!({"category": "wall", "name": "Ana 150", "thickness": 150})).unwrap();
    by(&mut s, "Bo", "type", json!({"category": "wall", "name": "Bo 250", "thickness": 250})).unwrap();
    let e = by(&mut s, "Ana", "undo", json!({})).unwrap_err();
    assert!(e.contains("Bo"), "{e}");
}

#[test]
fn a_guests_undo_on_the_host_is_the_guests_own() {
    let mut s = house();
    s.author = Some("Ana".into());
    let a = wall(&mut s, 40000);
    let b = s.model.next_id;
    let none = (None, None);
    run_for_guest(&mut s, "Bo", "wall", &json!({"points": [[0, 46000], [6000, 46000]]}), none.clone(), true).unwrap();
    wall(&mut s, 52000);
    run_for_guest(&mut s, "Bo", "undo", &json!({}), none, true).unwrap();
    assert!(s.model.elements.contains_key(a) && !s.model.elements.contains_key(b));
    assert_eq!(s.author.as_deref(), Some("Ana"));
}

#[test]
fn outside_a_session_undo_and_redo_are_as_they_always_were() {
    let mut s = house();
    let a = wall(&mut s, 40000);
    s.execute("undo", &json!({})).unwrap();
    assert!(!s.model.elements.contains_key(a));
    assert_eq!(s.steps(true).len(), 1, "on the redo stack, as before");
    s.execute("redo", &json!({})).unwrap();
    assert!(s.model.elements.contains_key(a));
    assert!(s.steps(false).iter().all(|st| st.by.is_none()));
}
