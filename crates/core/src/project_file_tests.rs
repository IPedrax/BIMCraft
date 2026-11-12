//! A project file as `save` writes it (`json_text`): a key a line, a list of numbers on one line, read back to the model
//! that was saved; and a project `open` would refuse isn't written, the file on disk left as it was.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Model;

/// A folder of its own for `tag`.
fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("bimcraft-project-file-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The sample house, and a meshed family placed in it: the lists a project holds, of points, faces, names and objects.
fn meshed_house(tag: &str) -> Session {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let d = dir(tag);
    // A tetrahedron, a metre each way.
    std::fs::write(d.join("tetra.obj"), "o Tetra\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nf 1 3 2\nf 1 2 4\nf 2 3 4\nf 1 4 3\n").unwrap();
    s.execute("import_mesh", &json!({"path": d.join("tetra.obj"), "at": [2000, 2000]})).unwrap();
    s
}

#[test]
fn a_saved_project_has_its_lists_of_numbers_on_a_line_and_reads_back_the_same() {
    let s = meshed_house("lists");
    let text = s.model.to_json().unwrap();
    // No line holds a number alone, as serde_json's pretty printer wrote a mesh: a mesh has a triangle a line.
    let alone: Vec<&str> = text.lines().filter(|l| l.trim().trim_end_matches(',').parse::<f64>().is_ok()).take(3).collect();
    assert!(alone.is_empty(), "{alone:?}");
    let triangle = |l: &str| l.trim().trim_end_matches(',').starts_with("[[") && l.matches('[').count() == 4;
    assert!(text.lines().any(triangle), "a mesh's triangle on a line");
    // The JSON is the one the pretty printer wrote, laid out otherwise, and shorter.
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(text.len() < serde_json::to_string_pretty(&v).unwrap().len());
    // Read back it is the model that was saved: saved again, the same text, byte for byte.
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.to_json().unwrap(), text);
}

#[test]
fn a_project_open_would_refuse_isnt_saved_and_the_file_on_disk_stays_as_it_was() {
    let mut s = meshed_house("refused");
    let path = dir("refused").join("house.bimc");
    s.execute("save", &json!({"path": path})).unwrap();
    let before = std::fs::read(&path).unwrap();
    // One more wall makes the project longer than the file `open` would now take.
    s.execute("wall", &json!({"points": [[0, 20000], [6000, 20000]]})).unwrap();
    crate::commands::SAVE_LIMIT.with(|l| l.set(Some(before.len() as u64 + 1)));
    let refused = s.execute("save", &json!({"path": path}));
    crate::commands::SAVE_LIMIT.with(|l| l.set(None));
    let e = refused.unwrap_err();
    assert!(e.contains("it isn't saved"), "{e}");
    assert_eq!(std::fs::read(&path).unwrap(), before, "the file saved before is untouched");
    // Under the limit again, it saves.
    s.execute("save", &json!({"path": path})).unwrap();
    assert!(std::fs::read(&path).unwrap().len() > before.len());
}
