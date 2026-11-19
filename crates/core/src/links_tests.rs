//! Manage Links through the commands every front end drives: `links` lists each file the project reads from elsewhere, by
//! kind, with its path, its status and the instances that place it.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A folder of this test's own, empty.
fn dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-links-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A box of walls saved in `dir` as `name` (a .bimc or an .ifc): its path.
fn architect(dir: &std::path::Path, name: &str) -> String {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    let path = dir.join(name).to_string_lossy().into_owned();
    run(&mut s, "save", json!({"path": path}));
    path
}

/// A DXF of one line, in `dir`: its path.
fn drawing(dir: &std::path::Path, name: &str) -> String {
    let text = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1000\n21\n0\n0\nENDSEC\n0\nEOF\n";
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

fn ids(row: &Value) -> Vec<u64> {
    row["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

#[test]
fn two_links_of_one_file_are_one_row_with_two_instances() {
    let dir = dir("two");
    let path = architect(&dir, "Architecture.bimc");
    let mut s = Session::new();
    let a = run(&mut s, "link", json!({"path": path}))["id"].as_u64().unwrap();
    let b = run(&mut s, "link", json!({"path": path, "name": "Architecture East", "at": [20_000, 0]}))["id"].as_u64().unwrap();
    let v = run(&mut s, "links", json!({}));
    let rows = v["projects"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{v}");
    let row = &rows[0];
    assert_eq!(row["name"], "Architecture");
    assert_eq!(row["path"], path);
    assert_eq!(row["path_type"], "absolute");
    assert_eq!(row["status"], "loaded");
    assert_eq!(row["instances"], 2);
    assert_eq!(ids(row), vec![a, b]);
    assert_eq!(row["names"], json!(["Architecture", "Architecture East"]), "each instance by its own name");
    // Every tab is there, the empty ones too, so a front end draws each.
    for kind in ["ifc", "cad", "pdf", "images", "clouds", "models"] {
        assert_eq!(v[kind], json!([]), "{kind}: {v}");
    }
}

#[test]
fn a_link_whose_file_is_gone_is_not_found() {
    let dir = dir("gone");
    let path = architect(&dir, "Gone.bimc");
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": path}));
    std::fs::remove_file(&path).unwrap();
    let v = run(&mut s, "links", json!({}));
    assert_eq!(v["projects"][0]["status"], "not found", "{v}");
}

#[test]
fn a_link_saved_beside_the_project_is_relative() {
    let dir = dir("relative");
    architect(&dir, "Structure.bimc");
    let mut s = Session::new();
    run(&mut s, "save", json!({"path": dir.join("Host.bimc").to_string_lossy()}));
    run(&mut s, "link", json!({"path": "Structure.bimc"}));
    let v = run(&mut s, "links", json!({}));
    let row = &v["projects"][0];
    assert_eq!((row["path"].clone(), row["path_type"].clone()), (json!("Structure.bimc"), json!("relative")), "{v}");
    assert_eq!(row["resolved"], dir.join("Structure.bimc").to_string_lossy().as_ref());
    assert_eq!(row["status"], "loaded");
}

#[test]
fn an_ifc_link_is_on_the_ifc_tab() {
    let dir = dir("ifc");
    let path = architect(&dir, "Survey.ifc");
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": path}));
    let v = run(&mut s, "links", json!({}));
    assert_eq!(v["projects"], json!([]), "{v}");
    assert_eq!((v["ifc"][0]["name"].clone(), v["ifc"][0]["instances"].clone()), (json!("Survey"), json!(1)), "{v}");
}

#[test]
fn cad_drawings_are_linked_or_imported_with_their_instances() {
    let dir = dir("cad");
    let linked = drawing(&dir, "Site.dxf");
    let imported = drawing(&dir, "Detail.dxf");
    let mut s = Session::new();
    let a = run(&mut s, "import_cad", json!({"path": linked, "link": true}))["id"].as_u64().unwrap();
    let b = run(&mut s, "import_cad", json!({"cad": "Site", "at": [5000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "import_cad", json!({"path": imported}));
    let v = run(&mut s, "links", json!({"kind": "cad"}));
    assert!(v.get("projects").is_none(), "`kind` answers its tab alone: {v}");
    let rows = v["cad"].as_array().unwrap();
    let site = rows.iter().find(|r| r["name"] == "Site").unwrap();
    assert_eq!((site["status"].clone(), site["instances"].clone(), ids(site)), (json!("loaded"), json!(2), vec![a, b]), "{v}");
    assert_eq!(site["path"], linked);
    let detail = rows.iter().find(|r| r["name"] == "Detail").unwrap();
    assert_eq!(detail["status"], "imported", "{v}");
    assert_eq!(detail["path"], Value::Null, "an import keeps no path: {v}");
}

#[test]
fn a_kind_it_has_no_tab_for_is_refused() {
    let mut s = Session::new();
    let e = s.execute("links", &json!({"kind": "dwf"})).unwrap_err();
    assert!(e.contains("projects, ifc, cad, pdf, images, clouds or models"), "{e}");
}

/// How many walls the views draw: the project's and its links'.
fn walls_seen(s: &Session) -> usize {
    let seen = crate::link::seen(&s.model);
    seen.elements.values().filter(|e| matches!(e, crate::model::Element::Wall(_))).count()
}

#[test]
fn an_unloaded_link_draws_nothing_and_stays_unloaded_when_the_project_opens() {
    let dir = dir("unload");
    let path = architect(&dir, "Architecture.bimc");
    let mut s = Session::new();
    let a = run(&mut s, "link", json!({"path": path}))["id"].as_u64().unwrap();
    run(&mut s, "link", json!({"path": path, "name": "Architecture East", "at": [20_000, 0]}));
    assert_eq!(walls_seen(&s), 8);
    // Unloading one instance unloads its file, as Revit unloads a link's type: both instances go.
    let v = run(&mut s, "unload", json!({"id": a}));
    assert_eq!(v["unloaded"], json!(["Architecture", "Architecture East"]), "{v}");
    assert_eq!(walls_seen(&s), 0);
    let row = run(&mut s, "links", json!({"kind": "projects"}))["projects"][0].clone();
    assert_eq!((row["status"].clone(), row["instances"].clone()), (json!("unloaded"), json!(2)), "it stays where it stands: {row}");
    // Saved and opened again, it isn't read.
    let host = dir.join("Host.bimc").to_string_lossy().into_owned();
    run(&mut s, "save", json!({"path": host}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": host}));
    assert_eq!(walls_seen(&t), 0, "an unloaded link isn't read when the project opens");
    assert!(t.model.loaded.is_empty());
    // Reload loads it again, both instances.
    let v = run(&mut t, "reload", json!({"id": a}));
    assert_eq!(v["reloaded"], json!(["Architecture", "Architecture East"]), "{v}");
    assert_eq!(walls_seen(&t), 8);
    assert_eq!(run(&mut t, "links", json!({"kind": "projects"}))["projects"][0]["status"], "loaded");
}

#[test]
fn unload_is_undone_and_reload_of_all_leaves_an_unloaded_link_alone() {
    let dir = dir("unload-undo");
    let a_path = architect(&dir, "A.bimc");
    let b_path = architect(&dir, "B.bimc");
    let mut s = Session::new();
    let a = run(&mut s, "link", json!({"path": a_path}))["id"].as_u64().unwrap();
    run(&mut s, "link", json!({"path": b_path, "at": [20_000, 0]}));
    run(&mut s, "unload", json!({"id": a}));
    assert_eq!(walls_seen(&s), 4);
    let v = run(&mut s, "reload", json!({}));
    assert_eq!(v["reloaded"], json!(["B"]), "reloading every link reads those that are loaded: {v}");
    assert_eq!(walls_seen(&s), 4);
    run(&mut s, "undo", json!({}));
    assert_eq!(walls_seen(&s), 8, "undo loads it again");
}

#[test]
fn unload_names_a_link() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}));
    let wall = *s.model.elements.keys().last().unwrap();
    let e = s.execute("unload", &json!({"id": wall})).unwrap_err();
    assert!(e.contains("is not a link"), "{e}");
}

#[test]
fn reload_from_reads_another_file_for_every_instance() {
    let dir = dir("reload-from");
    let old = architect(&dir, "Scheme A.bimc");
    // Scheme B is one wall.
    let mut b = Session::new();
    run(&mut b, "wall", json!({"points": [[0, 0], [5000, 0]]}));
    let new = dir.join("Scheme B.bimc").to_string_lossy().into_owned();
    run(&mut b, "save", json!({"path": new}));
    let mut s = Session::new();
    let a = run(&mut s, "link", json!({"path": old}))["id"].as_u64().unwrap();
    run(&mut s, "link", json!({"path": old, "name": "East", "at": [20_000, 0]}));
    let v = run(&mut s, "reload_from", json!({"id": a, "path": new}));
    assert_eq!((v["path"].clone(), v["links"].clone()), (json!(new), json!(["Scheme A", "East"])), "{v}");
    assert_eq!(walls_seen(&s), 2, "both instances draw the new file");
    let rows = run(&mut s, "links", json!({"kind": "projects"}))["projects"].clone();
    assert_eq!((rows[0]["name"].clone(), rows[0]["instances"].clone()), (json!("Scheme B"), json!(2)), "{rows}");
    assert_eq!(s.model.loaded.keys().collect::<Vec<_>>(), vec![&new], "the old file's model is let go");
    // A file that can't be read changes nothing.
    let e = s.execute("reload_from", &json!({"id": a, "path": dir.join("missing.bimc").to_string_lossy()})).unwrap_err();
    assert!(e.starts_with("reload_from"), "{e}");
    assert_eq!(run(&mut s, "links", json!({"kind": "projects"}))["projects"][0]["path"], new);
    // Undone, it reads the old file again.
    run(&mut s, "undo", json!({}));
    assert_eq!(walls_seen(&s), 8);
}

#[test]
fn reload_from_takes_an_unloaded_link_back() {
    let dir = dir("reload-from-unloaded");
    let old = architect(&dir, "A.bimc");
    let new = architect(&dir, "B.bimc");
    let mut s = Session::new();
    let a = run(&mut s, "link", json!({"path": old}))["id"].as_u64().unwrap();
    run(&mut s, "unload", json!({"id": a}));
    run(&mut s, "reload_from", json!({"id": a, "path": new}));
    assert_eq!(walls_seen(&s), 4);
    assert_eq!(run(&mut s, "links", json!({"kind": "projects"}))["projects"][0]["status"], "loaded");
}

#[test]
fn reload_cad_from_another_drawing() {
    let dir = dir("cad-from");
    let first = drawing(&dir, "Site.dxf");
    let text = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1000\n21\n0\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n0\n21\n1000\n0\nENDSEC\n0\nEOF\n";
    let second = dir.join("Site rev B.dxf");
    std::fs::write(&second, text).unwrap();
    let second = second.to_string_lossy().into_owned();
    let mut s = Session::new();
    run(&mut s, "import_cad", json!({"path": first, "link": true}));
    let v = run(&mut s, "reload_cad", json!({"cad": "Site", "path": second}));
    assert_eq!((v["changed"].clone(), v["entities"].clone()), (json!(true), json!(2)), "{v}");
    assert_eq!(run(&mut s, "links", json!({"kind": "cad"}))["cad"][0]["path"], second);
    // An imported drawing can be reloaded from a file too: it is linked to it from then on.
    let imported = drawing(&dir, "Detail.dxf");
    run(&mut s, "import_cad", json!({"path": imported}));
    run(&mut s, "reload_cad", json!({"cad": "Detail", "path": imported}));
    let rows = run(&mut s, "links", json!({"kind": "cad"}))["cad"].clone();
    let detail = rows.as_array().unwrap().iter().find(|r| r["name"] == "Detail").unwrap().clone();
    assert_eq!(detail["status"], "loaded", "{rows}");
}

/// An OBJ of one object, `name`, a triangle `size` m wide, in `dir`: its path.
fn model(dir: &std::path::Path, file: &str, name: &str, size: f64) -> String {
    let text = format!("o {name}\nv 0 0 0\nv {size} 0 0\nv 0 0 -{size}\nf 1 2 3\n");
    let path = dir.join(file);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn a_3d_model_is_listed_with_its_families_and_reloaded() {
    let dir = dir("models");
    let path = model(&dir, "Bench.obj", "Bench", 1.0);
    let mut s = Session::new();
    let v = run(&mut s, "import_mesh", json!({"path": path}));
    let component = v["components"][0].as_u64().unwrap();
    let rows = run(&mut s, "links", json!({"kind": "models"}))["models"].clone();
    assert_eq!(rows.as_array().unwrap().len(), 1, "{rows}");
    let row = &rows[0];
    assert_eq!(
        (row["name"].clone(), row["families"].clone(), row["status"].clone()),
        (json!("Bench.obj"), json!(["Bench"]), json!("loaded"))
    );
    assert_eq!((row["instances"].clone(), ids(row)), (json!(1), vec![component]));
    let before = s.model.families["Bench"].forms[0].mesh.clone();
    // The file changes: the triangle is twice as wide. Reload rebuilds the family about where it stood.
    model(&dir, "Bench.obj", "Bench", 2.0);
    let v = run(&mut s, "reload_mesh", json!({"path": path}));
    assert_eq!((v["reloaded"].clone(), v["missing"].clone()), (json!(["Bench"]), json!([])), "{v}");
    let after = s.model.families["Bench"].forms[0].mesh.clone();
    assert_ne!(before, after);
    assert_eq!(before[0][0], after[0][0], "the corner the file didn't move stays where it was");
    // Reload From another file, whose object of that name it reads.
    let other = model(&dir, "Bench v2.obj", "Bench", 3.0);
    let v = run(&mut s, "reload_mesh", json!({"path": path, "from": other}));
    assert_eq!(v["path"], other, "{v}");
    assert_eq!(run(&mut s, "links", json!({"kind": "models"}))["models"][0]["path"], other);
    // A file without the object leaves the family as it was.
    let empty = model(&dir, "Chair.obj", "Chair", 1.0);
    let v = run(&mut s, "reload_mesh", json!({"path": other, "from": empty}));
    assert_eq!(v["missing"], json!(["Bench"]), "{v}");
    std::fs::remove_file(&empty).unwrap();
    let e = s.execute("reload_mesh", &json!({"path": "nowhere.obj"})).unwrap_err();
    assert!(e.contains("no family"), "{e}");
}
