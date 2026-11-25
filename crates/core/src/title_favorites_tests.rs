//! A title block kept as a favorite: in the user's file, with the pictures of its logos, for every project to take.

use std::path::Path;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::favorites::Favorites;
use crate::model::Element;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// A project with title block `ESCRITORIO` (a frame, a field and a red logo) and the favorites file `file` in `dir`.
fn with_stamp(dir: &Path) -> (Session, String) {
    let logo = dir.join("logo.png");
    let mut pm = tiny_skia::Pixmap::new(40, 20).unwrap();
    pm.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
    std::fs::write(&logo, pm.encode_png().unwrap()).unwrap();
    let mut s = Session::new();
    run(
        &mut s,
        "title_block",
        json!({
            "name": "ESCRITORIO", "width": 100, "height": 30, "sizes": ["A3", "A4"],
            "lines": [{"points": [[0, 0], [100, 0], [100, 30], [0, 30]], "closed": true, "pen": 8}],
            "logos": [{"path": logo, "at": [2, 2], "width": 20}]
        }),
    );
    (s, dir.join("favorites.json").to_string_lossy().into_owned())
}

/// The pictures a new project holds: the starter materials' (#536).
fn starter() -> usize {
    Session::new().model.images.len()
}

#[test]
fn a_title_block_is_kept_with_its_logo_and_taken_into_another_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, file) = with_stamp(dir.path());
    let saved =
        run(&mut a, "favorite", json!({"action": "save", "kind": "title_block", "name": "ESCRITORIO", "as": "Office A3", "path": file}));
    assert_eq!(saved["saved"], "Office A3", "{saved}");
    let listed = run(&mut a, "favorite", json!({"path": file}));
    assert_eq!(
        listed["title_blocks"],
        json!([{"name": "Office A3", "width": 100.0, "height": 30.0, "sizes": ["A3", "A4"], "logos": 1}]),
        "{listed}"
    );
    assert_eq!(listed["materials"], json!([]), "and the other kinds are as they were");
    // Another project takes it, its picture with it, and a sheet of its paper draws with it.
    let mut b = Session::new();
    assert!(b.model.title_blocks.is_empty() && b.model.images.len() == starter());
    run(&mut b, "favorite", json!({"action": "load", "kind": "title_block", "name": "Office A3", "path": file}));
    assert_eq!(b.model.title_blocks["Office A3"], a.model.title_blocks["ESCRITORIO"]);
    assert_eq!(b.model.images, a.model.images, "the picture of its logo came with it");
    run(&mut b, "sheet", json!({"number": "A101"}));
    let drawn = b.model.elements.values().find_map(|e| match e {
        Element::Sheet(sh) => b.model.title_block_of(sh).map(|(n, _)| n.to_string()),
        _ => None,
    });
    assert_eq!(drawn.as_deref(), Some("Office A3"));
    // Loading twice under other names shares the one picture; a name the project has is its own unless it's to be replaced.
    run(&mut b, "favorite", json!({"action": "load", "kind": "title_block", "name": "Office A3", "as": "Copy", "path": file}));
    assert_eq!((b.model.title_blocks.len(), b.model.images.len()), (2, starter() + 1));
    let e = b.execute("favorite", &json!({"action": "load", "kind": "title_block", "name": "Office A3", "path": file})).unwrap_err();
    assert!(e.contains("replace"), "{e}");
    run(&mut b, "title_block", json!({"name": "Office A3", "width": 120}));
    run(&mut b, "favorite", json!({"action": "load", "kind": "title_block", "name": "Office A3", "replace": true, "path": file}));
    assert_eq!(b.model.title_blocks["Office A3"].width, 100.0, "replaced by the favorite");
    // It is one undo step, and an empty project's picture goes when the block that held it does.
    run(&mut b, "undo", json!({}));
    assert_eq!(b.model.title_blocks["Office A3"].width, 120.0);
    run(&mut b, "title_block", json!({"name": "Office A3", "delete": true}));
    run(&mut b, "title_block", json!({"name": "Copy", "delete": true}));
    assert_eq!(b.model.images.len(), starter());
    // Deleted from the favorites it's gone from them, and the project it was saved from keeps its own.
    run(&mut a, "favorite", json!({"action": "delete", "kind": "title_block", "name": "Office A3", "path": file}));
    assert_eq!(run(&mut a, "favorite", json!({"path": file}))["title_blocks"], json!([]));
    assert!(a.model.title_blocks.contains_key("ESCRITORIO") && a.model.images.len() == starter() + 1);
}

#[test]
fn a_title_block_that_is_not_there_or_is_not_sound_is_refused_and_says_what_to_fix() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, file) = with_stamp(dir.path());
    let before = s.model.clone();
    for (p, says) in [
        (json!({"action": "save", "kind": "title_block", "name": "Nothing", "path": file}), "no title block `Nothing`"),
        (json!({"action": "load", "kind": "title_block", "name": "Nothing", "path": file}), "no favorite title block"),
        (json!({"action": "delete", "kind": "title_block", "name": "Nothing", "path": file}), "no favorite title block"),
        (json!({"action": "save", "kind": "title_block", "path": file}), "`name` is required"),
    ] {
        let e = s.execute("favorite", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(*s.model, *before);
    // A file that holds one with a logo of a picture it lacks, or a name a sheet means none by, is refused whole.
    let tb = json!({"width": 10, "height": 10});
    let logo = json!({"image": "img-nothing", "at": [0, 0], "width": 5, "height": 5});
    for (text, says) in [
        (json!({"title_blocks": {"X": {"title_block": {"width": 10, "height": 10, "logos": [logo]}}}}), "picture"),
        (json!({"title_blocks": {"none": {"title_block": tb}}}), "none"),
        (json!({"title_blocks": {"": {"title_block": tb}}}), "name"),
        (json!({"title_blocks": {"X": {"title_block": {"width": 0, "height": 10}}}}), "width"),
        (json!({"title_blocks": {"X": {"title_block": tb, "extra": 1}}}), "unknown field"),
    ] {
        std::fs::write(&file, text.to_string()).unwrap();
        let e = Favorites::read(Path::new(&file)).unwrap_err();
        assert!(e.contains(says), "{text}: {e}");
    }
    // A project takes none of a favorites file's that was not sound, and keeps its own.
    let e = s.execute("favorite", &json!({"action": "load", "kind": "title_block", "name": "X", "path": file})).unwrap_err();
    assert!(e.contains("favorites"), "{e}");
}
