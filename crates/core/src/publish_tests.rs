//! Publishing sets and `publish`, driven as an agent drives them: what a set holds, what is refused, and the files it writes.

use crate::engine::Session;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A box of walls with a room, sheets A101 and A102 holding plans, and two print sets: Sheets (both) and Plans (Level 1).
fn project() -> Session {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           room {"point": [4000, 3000], "name": "Living"}
           sheet {"name": "Plans"}
           sheet {"name": "Details"}
           place {"sheet": "A101", "view": "Level 1"}
           print_set {"name": "Sheets", "views": ["A101", "A102"]}
           print_set {"name": "Plans", "views": ["Level 1"]}"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    s
}

/// A folder of its own for a test's files.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-publish-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The paths a report says were written, relative to `dir`, with `/` between folders, sorted.
fn written(r: &Value, dir: &Path) -> Vec<String> {
    let all = r["written"].as_array().cloned().unwrap_or_default();
    let mut out: Vec<String> = all
        .iter()
        .filter_map(|w| w["path"].as_str().map(PathBuf::from))
        .map(|p| p.strip_prefix(dir).unwrap_or(&p).iter().map(|c| c.to_string_lossy().to_string()).collect::<Vec<_>>().join("/"))
        .collect();
    out.sort();
    out
}

#[test]
fn a_set_writes_each_folders_print_sets_as_their_formats() {
    let mut s = project();
    let dir = folder("tree");
    run(
        &mut s,
        "publish_set",
        json!({"name": "Issue", "folders": [
            {"name": "PDF", "entries": [{"print_set": "Sheets", "format": "pdf", "combine": "{PublishSet} - {PrintSet}"}]},
            {"name": "CAD/DXF", "entries": [
                {"print_set": "Sheets", "format": "dxf"},
                {"print_set": "Plans", "format": "dxf", "name": "{ViewName} ({PrintSet})"}
            ]},
            {"name": "", "entries": [{"print_set": "Sheets", "format": "svg", "name": "{SheetNumber}"}]}
        ]}),
    );
    let r = run(&mut s, "publish", json!({"name": "Issue", "folder": dir}));
    assert_eq!(
        written(&r, &dir),
        [
            "A101.svg",
            "A102.svg",
            "CAD/DXF/A101 - Plans.dxf",
            "CAD/DXF/A102 - Details.dxf",
            "CAD/DXF/Level 1 (Plans).dxf",
            "PDF/Issue - Sheets.pdf"
        ],
        "{r}"
    );
    assert_eq!(r["failed"], json!([]), "{r}");
    assert_eq!(r["published"].as_array().map(Vec::len), Some(4), "every completed entry has a timing");
    let time = s.model.publish_sets["Issue"].folders[0].entries[0].last_export.as_ref().unwrap();
    assert!(time.ended_ms >= time.started_ms);
    let pdf = String::from_utf8_lossy(&std::fs::read(dir.join("PDF/Issue - Sheets.pdf")).unwrap_or_default()).to_string();
    assert_eq!(pdf.matches("/Type/Page/").count(), 2, "both sheets are pages of the one file");
    // Published again, its files are written over, not numbered.
    let again = run(&mut s, "publish", json!({"name": "Issue", "folder": dir}));
    assert_eq!(written(&again, &dir), written(&r, &dir));
    s.read_only = Some("test read only");
    let unchanged = std::sync::Arc::clone(&s.model);
    let readonly = run(&mut s, "publish", json!({"name": "Issue", "folder": dir}));
    assert_eq!(written(&readonly, &dir), written(&r, &dir));
    assert!(std::sync::Arc::ptr_eq(&unchanged, &s.model), "a read-only publish leaves the project unchanged");
    s.read_only = None;
    // Unless the set says to keep what is there.
    run(&mut s, "publish_set", json!({"name": "Issue", "overwrite": false}));
    let kept = run(&mut s, "publish", json!({"name": "Issue", "folder": dir}));
    assert!(written(&kept, &dir).contains(&"A101 (2).svg".to_string()), "{kept}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_print_set_is_in_a_folder_once_for_each_format() {
    let mut s = project();
    let twice = json!({"name": "Issue", "folders": [{"name": "Out", "entries": [
        {"print_set": "Sheets", "format": "pdf"}, {"print_set": "Sheets", "format": "PDF"}]}]});
    let e = s.execute("publish_set", &twice).unwrap_err();
    assert!(e.contains("once for each format"), "{e}");
    // As two formats, and as one format in two folders, it is taken.
    let r = run(
        &mut s,
        "publish_set",
        json!({"name": "Issue", "folders": [
            {"name": "Out", "entries": [{"print_set": "Sheets", "format": "pdf"}, {"print_set": "Sheets", "format": "dxf"}]},
            {"name": "Other", "entries": [{"print_set": "Sheets", "format": "pdf"}]}
        ]}),
    );
    assert_eq!(r["entries"], 3, "{r}");
    // A format is kept by its id, whatever it was given as.
    run(&mut s, "publish_set", json!({"name": "Ifc", "folders": [{"name": "M", "entries": [{"print_set": "Sheets", "format": "ifc"}]}]}));
    assert_eq!(s.model.publish_sets["Ifc"].folders[0].entries[0].format, "ifc4");
    for (bad, says) in [
        (json!({"name": "X", "folders": [{"name": "A", "entries": [{"print_set": "Nope", "format": "pdf"}]}]}), "no print set"),
        (json!({"name": "X", "folders": [{"name": "A", "entries": [{"print_set": "Sheets", "format": "doc"}]}]}), "isn't a format"),
        (json!({"name": "X", "folders": [{"name": "A", "entries": [{"print_set": "Sheets", "format": "u3d"}]}]}), "isn't written yet"),
        (json!({"name": "X", "folders": [{"name": "../up", "entries": []}]}), ".."),
        (json!({"name": "X", "folders": [{"name": "A", "entries": []}, {"name": "a/", "entries": []}]}), "twice"),
        (
            json!({"name": "X", "folders": [{"name": "A", "entries": [{"print_set": "Sheets", "format": "pdf", "name": "a}b"}]}]}),
            "closes no token",
        ),
        (json!({"name": "X", "folders": [{"name": "A", "entries": [{"print_set": "Sheets", "format": "pdf", "pages": 2}]}]}), "folders"),
        (json!({"name": "Issue", "rename": "Ifc"}), "already"),
        (json!({"name": "Nope", "delete": true}), "no publishing set"),
        (json!({"name": "Nope", "duplicate": "Other"}), "no publishing set"),
    ] {
        let e = s.execute("publish_set", &bad).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
    assert!(!s.model.publish_sets.contains_key("X"));
}

#[test]
fn a_set_is_a_library_entry_renamed_duplicated_saved_and_listed() {
    let mut s = project();
    run(
        &mut s,
        "publish_set",
        json!({"name": "Issue", "folder": "Published", "folders": [{"name": "PDF", "entries": [
            {"print_set": "Sheets", "format": "pdf", "options": {"page": {"paper": "A3", "margin": 10}, "pdf": {"archive": "a2b"}}}]}]}),
    );
    let id = s.model.library.id(&crate::library::Kind::PublishSet, "Issue");
    assert!(id.is_some());
    run(&mut s, "publish_set", json!({"name": "Issue", "rename": "Tender"}));
    assert_eq!(s.model.library.id(&crate::library::Kind::PublishSet, "Tender"), id, "renamed, it keeps its id");
    let r = run(&mut s, "publish_set", json!({"name": "Tender", "duplicate": "Tender 2"}));
    assert_eq!(r["entries"], 1, "{r}");
    assert_ne!(s.model.library.id(&crate::library::Kind::PublishSet, "Tender 2"), id);
    let listed = run(&mut s, "publish_set", json!({}));
    let first = &listed["publish_sets"][0];
    assert_eq!((first["name"].as_str(), first["folder"].as_str()), (Some("Tender"), Some("Published")), "{listed}");
    assert_eq!(first["folders"][0]["entries"][0]["options"]["page"]["paper"], "A3", "{listed}");
    // A print set renamed is still the one meant, here and in a file.
    run(&mut s, "print_set", json!({"name": "Sheets", "rename": "All Sheets"}));
    assert_eq!(s.model.publish_sets["Tender"].folders[0].entries[0].print_set, "All Sheets");
    let dir = folder("saved");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("p.bimcraft");
    run(&mut s, "save", json!({"path": file}));
    let text = crate::bimc::read_to_string(&file).unwrap_or_default();
    assert!(!text.contains("\"print_set\":\"All Sheets\""), "a file holds the print set's GUID, not its name");
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": file}));
    assert_eq!(t.model.publish_sets, s.model.publish_sets);
    // Beside the project's file: a folder that isn't absolute.
    let r = run(&mut t, "publish", json!({"name": "Tender"}));
    assert_eq!(written(&r, &dir), ["Published/PDF/A101 - Plans.pdf", "Published/PDF/A102 - Details.pdf"], "{r}");
    // A print set deleted leaves every folder it was in; undone, it is back in them.
    run(&mut s, "print_set", json!({"name": "All Sheets", "delete": true}));
    assert!(s.model.publish_sets["Tender"].is_empty() && s.model.publish_sets["Tender 2"].is_empty());
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.publish_sets["Tender"].len(), 1);
    run(&mut s, "publish_set", json!({"name": "Tender", "delete": true}));
    assert!(!s.model.publish_sets.contains_key("Tender"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn publish_says_what_it_cant_write_and_goes_on() {
    let mut s = project();
    run(&mut s, "print_set", json!({"name": "Empty", "views": []}));
    // No set, no destination, nothing in it.
    assert!(s.execute("publish", &json!({"name": "Nope"})).unwrap_err().contains("no publishing set"));
    run(&mut s, "publish_set", json!({"name": "Bare"}));
    let dir = folder("partial");
    assert!(s.execute("publish", &json!({"name": "Bare"})).unwrap_err().contains("no destination"));
    assert!(s.execute("publish", &json!({"name": "Bare", "folder": dir})).unwrap_err().contains("nothing to publish"));
    run(&mut s, "publish_set", json!({"name": "Bare", "folder": "out"}));
    assert!(s.execute("publish", &json!({"name": "Bare"})).unwrap_err().contains("isn't saved yet"));
    // A schedule's format of sheets, an empty print set and a parameter no project has: each said, the rest written.
    run(
        &mut s,
        "publish_set",
        json!({"name": "Mixed", "folders": [{"name": "Out", "entries": [
            {"print_set": "Sheets", "format": "csv"},
            {"print_set": "Empty", "format": "pdf"},
            {"print_set": "Plans", "format": "svg", "name": "{No Such Parameter}"},
            {"print_set": "Plans", "format": "png"}
        ]}]}),
    );
    let r = run(&mut s, "publish", json!({"name": "Mixed", "folder": dir}));
    assert_eq!(written(&r, &dir), ["Out/Level 1.png"], "{r}");
    let failed: Vec<String> = r["failed"].as_array().cloned().unwrap_or_default().iter().map(|f| f["error"].to_string()).collect();
    assert_eq!(failed.len(), 3, "{r}");
    assert!(failed[0].contains("holds nothing CSV writes") && failed[1].contains("holds nothing"), "{failed:?}");
    assert!(failed[2].contains("No Such Parameter"), "{failed:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn file_names_take_project_information_and_project_parameters() {
    let mut s = project();
    run(&mut s, "parameter", json!({"name": "Permit Number", "categories": ["project"]}));
    run(&mut s, "parameter", json!({"name": "Stage", "kind": "integer", "categories": ["project"]}));
    run(
        &mut s,
        "project_info",
        json!({"number": "2026-014", "client": "Ana Lima", "status": "Tender", "address": "Rua das Flores 12\nRecife PE",
               "params": {"Permit Number": "PR/88", "Stage": 3}}),
    );
    let listed = run(&mut s, "export_formats", json!({}));
    for token in ["PrintSet", "PublishSet", "Project Number", "Client Name", "Permit Number", "Stage"] {
        assert!(listed["tokens"].as_array().is_some_and(|t| t.contains(&json!(token))), "{token}: {listed}");
    }
    run(
        &mut s,
        "publish_set",
        json!({"name": "Issue", "folders": [{"name": "{Project Status}", "entries": [
            {"print_set": "Sheets", "format": "svg",
             "name": "{Project Number}_{SheetNumber}_{Permit Number}_S{Stage}_{Client Name}/{Project Address}"}]}]}),
    );
    let dir = folder("tokens");
    let r = run(&mut s, "publish", json!({"name": "Issue", "folder": dir}));
    // A folder's name is a name, not a template; a value's slash is never a folder; an address is one line.
    assert_eq!(
        written(&r, &dir),
        [
            "{Project Status}/2026-014_A101_PR-88_S3_Ana Lima/Rua das Flores 12, Recife PE.svg",
            "{Project Status}/2026-014_A102_PR-88_S3_Ana Lima/Rua das Flores 12, Recife PE.svg"
        ],
        "{r}"
    );
    // `export` takes them too; one not set is empty.
    run(&mut s, "project_info", json!({"client": ""}));
    let r = run(&mut s, "export", json!({"format": "svg", "view": "A101", "folder": dir, "name": "x{Client Name}-{Permit Number}"}));
    assert_eq!(written(&r, &dir), ["x-PR-88.svg"], "{r}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_cancelled_publish_keeps_what_it_wrote_whole_and_stops() {
    let s = {
        let mut s = project();
        run(
            &mut s,
            "publish_set",
            json!({"name": "Issue", "folders": [{"name": "A", "entries": [
                {"print_set": "Sheets", "format": "svg"}, {"print_set": "Sheets", "format": "png"}]}]}),
        );
        s
    };
    let dir = folder("cancel");
    let steps = std::sync::Mutex::new(Vec::new());
    let stop = std::sync::atomic::AtomicBool::new(false);
    let cancelled = || stop.load(std::sync::atomic::Ordering::Relaxed);
    // Stopped once the first entry's two sheets are done.
    let progress = |done: usize, total: usize, _: &str| {
        steps.lock().unwrap().push((done, total));
        if done >= 2 {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    };
    let ctx = crate::export::Ctx { cancelled: &cancelled, progress: &progress };
    let r = crate::publish::run(&s.model, &s.exports, "Issue", &dir, &ctx).unwrap();
    assert!(r.cancelled);
    assert_eq!(r.written.len(), 2, "{r:?}");
    assert!(r.written.iter().all(|w| w.format == "svg" && std::fs::read(&w.path).is_ok_and(|b| b.starts_with(b"<"))));
    // The steps are the whole set's: four, never one entry's two.
    assert!(steps.lock().unwrap().iter().all(|(_, total)| *total == 4), "{steps:?}");
    let _ = std::fs::remove_dir_all(dir);
}
