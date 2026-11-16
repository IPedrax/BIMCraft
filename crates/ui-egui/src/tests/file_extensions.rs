//! Discussion 310: a project is saved as `.bimc` (a typed `.bimt` kept), a family is a `.bimf`, and what was
//! written under the old names (`.bimcraft`, a family's `.json`) still opens, in every way a file opens, and is never rewritten.
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use bimcraft_core::files;

use super::*;
use crate::OsEvent;
use crate::revit::open_extensions;

/// The system's file dialog the last press began, answered with `path`.
fn answer(h: &mut Harness<BimApp>, path: &Path) {
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a dialog was begun");
    tx.send(Some(vec![path.to_path_buf()])).unwrap();
    h.run();
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn save_as_names_a_project_bimc_and_keeps_the_extension_typed_when_it_is_one_offered() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = app(BOX);
    let ctx = h.ctx.clone();
    for (typed, saved) in [("plan", "plan.bimc"), ("kept.bimc", "kept.bimc"), ("shell.bimt", "shell.bimt"), ("Up.BIMC", "Up.BIMC")] {
        h.state_mut().save_as(&ctx, None);
        answer(&mut h, &dir.path().join(typed));
        assert_eq!(h.state().session.path.as_deref(), Some(dir.path().join(saved).as_path()), "{typed}");
        assert!(dir.path().join(saved).exists(), "{saved}");
        assert!(!h.state().dirty(), "{saved} is the project's file now");
    }
}

#[test]
fn a_project_from_before_the_new_names_opens_every_way_a_file_opens_and_is_not_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new();
    s.script(BOX).unwrap();
    let names = ["house.bimcraft", "house.bimc", "house.bimt", "house.bimb"];
    for name in names {
        s.execute("save", &json!({"path": dir.path().join(name)})).unwrap();
    }
    let old = dir.path().join("house.bimcraft");
    let (bytes, modified) = (std::fs::read(&old).unwrap(), std::fs::metadata(&old).unwrap().modified().unwrap());
    let before = listing(dir.path());
    let mut h = app(SAMPLE);
    h.state_mut().keep_recent_in(dir.path().join("recent.json"));
    for name in names {
        let path = dir.path().join(name);
        assert!(h.state_mut().open_path(&path), "{name}: {}", h.state().status.text);
        assert_eq!(walls(&h).len(), 4, "{name}");
        assert_eq!(h.state().session.path.as_deref(), Some(path.as_path()), "the project keeps the file's own name");
    }
    // The system's open event, and a file dropped on the window, are the same as File > Open.
    let inbox: Rc<RefCell<Vec<OsEvent>>> = Rc::default();
    let queue = Rc::clone(&inbox);
    h.state_mut().os_events = Some(Box::new(move || queue.borrow_mut().drain(..).collect()));
    h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [0, 9000]]})).unwrap();
    inbox.borrow_mut().push(OsEvent::Open(old.clone()));
    h.run();
    press(&mut h, "Don't Save");
    assert_eq!(walls(&h).len(), 4);
    assert_eq!(h.state().session.path.as_deref(), Some(old.as_path()));
    let recent: Vec<String> = h.state().recent.labels();
    for name in names {
        assert!(recent.iter().any(|r| r == name), "Open Recent lists {name}: {recent:?}");
    }
    assert_eq!(recent[0], "house.bimcraft", "opened last, so first");
    assert_eq!(std::fs::read(&old).unwrap(), bytes);
    assert_eq!(std::fs::metadata(&old).unwrap().modified().unwrap(), modified);
    let mut after = listing(dir.path());
    after.retain(|n| n != "recent.json");
    assert_eq!(after, before, "nothing renamed, nothing made beside it");
}

#[test]
fn the_open_and_link_dialogs_list_every_project_extension_and_the_family_dialogs_both_of_theirs() {
    let open = open_extensions(false);
    for e in ["bimc", "bimt", "bimb", "bimcraft", "revcraft", "ifc"] {
        assert!(open.contains(&e), "File > Open lists .{e}: {open:?}");
    }
    assert_eq!(open[0], "bimc", "the project's own comes first");
    let link = crate::links::link_extensions();
    for e in ["bimc", "bimt", "bimcraft", "ifc"] {
        assert!(link.contains(&e), "Link Model lists .{e}: {link:?}");
    }
    assert_eq!(files::FAMILIES, ["bimf", "json"], "Save Family offers the first; Load Family and Open Family read both");
}

/// A family file as Save Family writes one, under `name`.
fn stool(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let mut s = Session::new();
    s.execute(
        "family",
        &json!({"name": "Stool", "category": "furniture", "types": {"Low": {}},
                "forms": [{"profile": [[0, 0], [400, 0], [400, 400], [0, 400]], "to": 450}]}),
    )
    .unwrap();
    s.execute("family", &json!({"name": "Stool", "save": path.to_string_lossy()})).unwrap();
    path
}

#[test]
fn a_bimf_file_the_system_opens_is_a_family_document_and_leaves_an_unsaved_project_alone() {
    let dir = tempfile::tempdir().unwrap();
    let file = stool(dir.path(), "Stool.bimf");
    let mut h = app(BOX);
    let inbox: Rc<RefCell<Vec<OsEvent>>> = Rc::default();
    let queue = Rc::clone(&inbox);
    h.state_mut().os_events = Some(Box::new(move || queue.borrow_mut().drain(..).collect()));
    h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [0, 9000]]})).unwrap();
    assert!(h.state().dirty());
    inbox.borrow_mut().push(OsEvent::Open(file.clone()));
    h.run();
    assert_eq!(h.state().active_family().map(|ed| ed.title().to_string()).as_deref(), Some("Stool"));
    assert!(h.query_by_label_contains("Save changes").is_none(), "the project is not replaced, so it is not asked about");
    assert_eq!(walls(&h).len(), 5);
    // The same file through File > Open's path, with the project behind it as it was.
    let mut h = app(BOX);
    assert!(h.state_mut().open_path(&file), "{}", h.state().status.text);
    assert_eq!(h.state().active_family().map(|ed| ed.title().to_string()).as_deref(), Some("Stool"));
    assert_eq!(walls(&h).len(), 4);
    // The family an earlier version saved, a `.json`, is still a family to open from the Family Editor.
    let old = stool(dir.path(), "Old Stool.json");
    let mut h = app(BOX);
    h.state_mut().open_family_file(&old);
    assert_eq!(h.state().active_family().map(|ed| ed.title().to_string()).as_deref(), Some("Stool"));
}

#[test]
fn a_project_is_saved_plain_unless_compressed_save_is_on_and_opens_either_way_and_the_choice_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    std::fs::write(&settings, "{}").unwrap();
    let mut h = app(BOX);
    h.state_mut().use_settings(settings.clone());
    assert!(!h.state().compressed_save, "off at first");
    let plain = dir.path().join("plain.bimc");
    assert!(h.state_mut().save_to(&plain));
    assert!(!files_zipped(&plain), "plain JSON by default");
    // Settings > Files (and the File menu) turn it on: Save and Save As zip the project from then on.
    h.state_mut().compressed_save = true;
    h.run();
    let zipped = dir.path().join("zipped.bimc");
    assert!(h.state_mut().save_to(&zipped));
    assert!(files_zipped(&zipped));
    // An export takes no such choice.
    let ifc = dir.path().join("box.ifc");
    assert!(h.state_mut().save_to(&ifc));
    assert!(std::fs::read_to_string(&ifc).unwrap().starts_with("ISO-10303-21"));
    // Both open, in a window whose setting is either.
    for (path, on) in [(&plain, true), (&zipped, false)] {
        let mut again = app("");
        again.state_mut().compressed_save = on;
        assert!(again.state_mut().open_path(path), "{}", again.state().status.text);
        assert_eq!(walls(&again).len(), 4, "{}", path.display());
    }
    // Kept in the user's settings, for the next start.
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(kept["compressed_save"], json!(true));
    let mut next = app("");
    next.state_mut().use_settings(settings);
    assert!(next.state().compressed_save);
}

fn files_zipped(path: &Path) -> bool {
    bimcraft_core::bimc::is_zip(&std::fs::read(path).unwrap())
}

/// Settings > Files with Compressed save, for a person to look at: `BIMCRAFT_SETTINGS_DUMP=<folder>` writes it light and dark.
#[test]
fn compressed_save_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_SETTINGS_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let mut h = app(BOX);
        if dark {
            h.ctx.set_theme(egui::Theme::Dark);
        }
        h.state_mut().settings_page = Some(crate::settings::Page::Files);
        h.run();
        h.render().unwrap().save(Path::new(&folder).join(format!("settings-files-{look}.png"))).unwrap();
    }
}
