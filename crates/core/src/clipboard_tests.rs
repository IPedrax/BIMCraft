//! The clipboard: Copy and Cut set elements aside as they were, and Paste brings them in as new ones — where they were, on
//! another level when one is asked for, and wherever asked otherwise.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// The id of the one element `v` made.
fn one(v: &Value) -> Id {
    v["ids"][0].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

#[test]
fn copy_and_paste_make_new_elements_where_they_were() {
    let mut s = Session::new();
    let w = one(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": "Level 1"})));
    run(&mut s, "clipboard_copy", json!({"ids": [w], "level": "Level 1"}));
    let copy = one(&run(&mut s, "clipboard_paste", json!({})));
    assert_ne!(copy, w, "a new element");
    let (a, b) = (s.model.wall(w).unwrap().clone(), s.model.wall(copy).unwrap().clone());
    assert_eq!((a.start, a.end), (b.start, b.end), "the copy stands where the wall was");
    assert_eq!(a.level, b.level, "on its own level");
    assert_ne!(s.model.guid(w), s.model.guid(copy), "with a GUID of its own");
}

#[test]
fn paste_on_another_level_moves_the_elements_there_and_keeps_their_height() {
    let mut s = Session::new();
    let w = one(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": "Level 1"})));
    run(&mut s, "level", json!({"name": "Level 3", "elevation": 6000}));
    run(&mut s, "clipboard_copy", json!({"ids": [w], "level": "Level 1"}));
    let copy = one(&run(&mut s, "clipboard_paste", json!({"level": "Level 3"})));
    let pasted = s.model.wall(copy).unwrap().clone();
    assert_eq!(s.model.level(pasted.level).map(|l| l.name.as_str()), Some("Level 3"), "it stands on Level 3");
    // Its top was Level 2, which would be Level 3 too: it keeps the height it had instead.
    assert_eq!(pasted.top_level, None);
    assert_eq!(pasted.height, 3000.0);
    let old = s.model.wall(w).unwrap();
    assert_eq!((pasted.start, pasted.end), (old.start, old.end), "at the same X and Y");
}

#[test]
fn paste_moves_the_elements_by_the_offset_it_is_given() {
    let mut s = Session::new();
    let w = one(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": "Level 1"})));
    run(&mut s, "clipboard_copy", json!({"ids": [w], "level": "Level 1"}));
    let copy = one(&run(&mut s, "clipboard_paste", json!({"by": [1000, 500]})));
    let pasted = s.model.wall(copy).unwrap();
    assert_eq!((pasted.start, pasted.end), (P2(1000.0, 500.0), P2(5000.0, 500.0)));
}

#[test]
fn cut_takes_the_elements_out_and_paste_brings_them_back() {
    let mut s = Session::new();
    let w = one(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": "Level 1"})));
    run(&mut s, "clipboard_cut", json!({"ids": [w], "level": "Level 1"}));
    assert!(s.model.wall(w).is_none(), "cut took the wall out");
    let back = one(&run(&mut s, "clipboard_paste", json!({})));
    assert!(s.model.wall(back).is_some(), "and paste brought it back");
}

#[test]
fn a_pasted_wall_brings_a_door_of_it_and_the_door_is_hosted_in_the_copy() {
    let mut s = Session::new();
    let w = one(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": "Level 1"})));
    let d = one(&run(&mut s, "door", json!({"at": [2000, 0], "level": "Level 1"})));
    run(&mut s, "clipboard_copy", json!({"ids": [w, d], "level": "Level 1"}));
    let made = run(&mut s, "clipboard_paste", json!({}));
    let ids: Vec<Id> = made["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    let copy_wall = ids.iter().copied().find(|id| s.model.wall(*id).is_some()).unwrap();
    let copy_door = ids.iter().copied().find(|id| matches!(s.model.elements.get(*id), Some(Element::Door(_)))).unwrap();
    let host = match s.model.elements.get(copy_door) {
        Some(Element::Door(door)) => door.host,
        _ => panic!("the door pasted"),
    };
    assert_eq!(s.model.guid(copy_wall), host, "the copied door is hosted in the copied wall");
}

#[test]
fn paste_without_a_copy_is_refused() {
    let mut s = Session::new();
    let e = s.execute("clipboard_paste", &json!({})).unwrap_err();
    assert!(e.contains("nothing was copied"), "{e}");
}
