//! Frozen rooms: a room that keeps the boundary it had when frozen, so the engine never searches the walls for it again.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Element;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// A 4000 x 3000 box of walls on their centre lines, with two rooms parted by a separation line: west of x = 2000, a frozen
/// one, east of it, one that follows the walls. Returns the walls' ids, the frozen room and the other.
fn two_rooms(s: &mut Session) -> (Vec<u64>, u64, u64) {
    let walls = run(s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
    let walls = walls["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    run(s, "room_separator", json!({"points": [[2000, 0], [2000, 3000]]}));
    let frozen = run(s, "room", json!({"point": [1000, 1500], "name": "Frozen", "frozen": true}))["id"].as_u64().unwrap();
    let live = run(s, "room", json!({"point": [3000, 1500], "name": "Live"}))["id"].as_u64().unwrap();
    (walls, frozen, live)
}

fn room(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({"category": "room"}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap()
}

fn area(s: &mut Session, id: u64) -> f64 {
    room(s, id)["area_m2"].as_f64().unwrap()
}

#[test]
fn a_frozen_room_keeps_its_size_when_the_walls_move() {
    let mut s = Session::new();
    let (walls, frozen, live) = two_rooms(&mut s);
    assert_eq!((area(&mut s, frozen), area(&mut s, live)), (6.0, 6.0));
    // The west wall goes 1000 west and the east one 1000 east: the live room grows, the frozen one doesn't.
    run(&mut s, "move", json!({"ids": [walls[3]], "by": [-1000, 0]}));
    run(&mut s, "move", json!({"ids": [walls[1]], "by": [1000, 0]}));
    assert_eq!((area(&mut s, frozen), area(&mut s, live)), (6.0, 9.0));
    assert_eq!(room(&mut s, frozen)["frozen"], true);
    // Unfrozen, it is found from the walls again.
    run(&mut s, "modify", json!({"id": frozen, "frozen": false}));
    assert_eq!(area(&mut s, frozen), 9.0);
    assert!(room(&mut s, frozen).get("kept").is_none(), "an unfrozen room keeps no boundary");
    // Frozen again, it keeps the boundary it has now.
    run(&mut s, "modify", json!({"id": frozen, "frozen": true}));
    run(&mut s, "move", json!({"ids": [walls[3]], "by": [1000, 0]}));
    run(&mut s, "move", json!({"ids": [walls[1]], "by": [-1000, 0]}));
    assert_eq!((area(&mut s, frozen), area(&mut s, live)), (9.0, 6.0));
}

#[test]
fn a_frozen_room_stays_enclosed_when_its_walls_go() {
    let mut s = Session::new();
    let (walls, frozen, live) = two_rooms(&mut s);
    run(&mut s, "delete", json!({"ids": walls}));
    assert_eq!(room(&mut s, frozen)["enclosed"], true);
    assert_eq!(area(&mut s, frozen), 6.0);
    assert_eq!(room(&mut s, live)["enclosed"], false);
    let warnings = run(&mut s, "inspect", json!({}))["warnings"].to_string();
    assert!(warnings.contains("Live") && !warnings.contains("Frozen"), "{warnings}");
    // It keeps both measures: inside the finish faces as well as at the centre lines.
    run(&mut s, "room_area", json!({"at": "finish"}));
    let finish = area(&mut s, frozen);
    assert!(finish > 4.0 && finish < 6.0, "{finish}");
}

#[test]
fn a_frozen_room_moved_or_mirrored_takes_its_boundary_along() {
    let mut s = Session::new();
    let (walls, frozen, _) = two_rooms(&mut s);
    run(&mut s, "delete", json!({"ids": walls}));
    run(&mut s, "move", json!({"ids": [frozen], "by": [10000, 0]}));
    assert_eq!(area(&mut s, frozen), 6.0);
    let Some(Element::Room(r)) = s.model.elements.get(frozen) else { panic!("no room") };
    let outer = &r.kept.as_ref().and_then(|k| k.centre.as_ref()).unwrap().outer;
    assert!(outer.iter().all(|p| p.0 >= 10000.0 - 1e-6), "{outer:?}");
    run(&mut s, "mirror", json!({"ids": [frozen], "axis": [[0, 0], [0, 1000]], "copy": true}));
    let copy = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Room(_))).map(|(id, _)| *id).max().unwrap();
    assert_ne!(copy, frozen);
    assert_eq!(area(&mut s, copy), 6.0, "a mirror image is no smaller");
    assert_eq!(room(&mut s, copy)["frozen"], true);
}

#[test]
fn a_room_that_is_not_enclosed_cannot_be_frozen() {
    let mut s = Session::new();
    let (walls, _, live) = two_rooms(&mut s);
    run(&mut s, "delete", json!({"ids": walls}));
    let e = s.execute("modify", &json!({"id": live, "frozen": true})).unwrap_err();
    assert!(e.contains("not enclosed"), "{e}");
    assert_eq!(room(&mut s, live)["frozen"], Value::Null);
}

#[test]
fn a_frozen_room_is_saved_with_its_boundary_and_undone() {
    let mut s = Session::new();
    let (walls, frozen, _) = two_rooms(&mut s);
    run(&mut s, "delete", json!({"ids": walls}));
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-frozen-room.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(area(&mut t, frozen), 6.0);
    // Unfreezing a room whose walls are gone is undone to the boundary it kept.
    run(&mut t, "modify", json!({"id": frozen, "frozen": false}));
    assert_eq!(room(&mut t, frozen)["enclosed"], false);
    run(&mut t, "undo", json!({}));
    assert_eq!(area(&mut t, frozen), 6.0);
}

#[test]
fn a_kept_boundary_given_is_checked() {
    let mut s = Session::new();
    let (_, frozen, _) = two_rooms(&mut s);
    for kept in [
        json!({"centre": {"outer": [[0, 0], [1, 0]], "holes": []}}),
        json!({"centre": {"outer": [[0, 0], [1e300, 0], [0, 1]], "holes": []}}),
        json!({"centre": null, "finish": null}),
    ] {
        let e = s.execute("modify", &json!({"id": frozen, "kept": kept})).unwrap_err();
        assert!(e.contains("room"), "{e}");
    }
    assert_eq!(area(&mut s, frozen), 6.0);
}
