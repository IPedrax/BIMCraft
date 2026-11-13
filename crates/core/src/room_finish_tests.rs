//! A room's own floor and ceiling: made by its type, kept along its boundary at the walls' faces, let go and made again.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Finish, Model};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

/// A room type `Finished` with the starter floor and ceiling types, the floor's top 50 mm and the ceiling 2400 mm over the level.
fn finished(s: &mut Session) {
    run(
        s,
        "type",
        json!({"category": "room", "name": "Finished", "floor": "Generic - 200mm", "floor_offset": 50, "ceiling": "Generic - 20mm", "ceiling_offset": 2400}),
    );
}

/// A 4000 x 3000 box of 200 mm walls on their centre lines, and a room of type `Finished` in it: the walls and the room.
fn boxed(s: &mut Session) -> (Vec<u64>, u64) {
    finished(s);
    let walls = run(s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
    let walls = walls["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    let room = id(&run(s, "room", json!({"point": [2000, 1500], "type": "Finished", "name": "Hall"})));
    (walls, room)
}

/// The room's floor and ceiling, while it has them connected.
fn finishes(m: &Model, room: u64) -> (Option<u64>, Option<u64>) {
    let Some(Element::Room(r)) = m.elements.get(room) else { panic!("no room {room}") };
    (r.floor.connected().and_then(|g| m.elements.id(g)), r.ceiling.connected().and_then(|g| m.elements.id(g)))
}

fn element(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap_or_else(|| panic!("no element {id}"))
}

fn area(s: &mut Session, id: u64) -> f64 {
    element(s, id)["area_m2"].as_f64().unwrap()
}

fn count(m: &Model, category: &str) -> usize {
    m.elements.values().filter(|e| e.category() == category).count()
}

#[test]
fn a_new_project_has_one_room_type_with_no_floor_or_ceiling() {
    let mut s = Session::new();
    assert_eq!(s.model.type_names("room"), ["Standard"]);
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
    let room = id(&run(&mut s, "room", json!({"point": [2000, 1500]})));
    assert_eq!(element(&mut s, room)["type"], "Standard");
    assert_eq!(finishes(&s.model, room), (None, None));
    assert_eq!((count(&s.model, "floor"), count(&s.model, "ceiling")), (0, 0));
}

#[test]
fn a_room_of_a_type_with_finishes_gets_them_at_the_wall_faces_whatever_the_measure() {
    for at in ["centre", "finish"] {
        let mut s = Session::new();
        run(&mut s, "room_area", json!({"at": at}));
        let (_, room) = boxed(&mut s);
        let (Some(floor), Some(ceiling)) = finishes(&s.model, room) else { panic!("no finishes at {at}") };
        // Inside the 200 mm walls: 3800 x 2800.
        assert_eq!((area(&mut s, floor), area(&mut s, ceiling)), (10.64, 10.64), "{at}");
        let (f, c) = (element(&mut s, floor), element(&mut s, ceiling));
        assert_eq!((f["type"].as_str(), f["offset"].as_f64(), f["room"].as_u64()), (Some("Generic - 200mm"), Some(50.0), Some(room)));
        assert_eq!((c["type"].as_str(), c["offset"].as_f64(), c["room"].as_u64()), (Some("Generic - 20mm"), Some(2400.0), Some(room)));
        assert_eq!(f["boundary"], json!([[100.0, 100.0], [3900.0, 100.0], [3900.0, 2900.0], [100.0, 2900.0]]));
        assert_eq!(f["boundary"], c["boundary"]);
        // One step makes the room and its finishes, and takes them back.
        run(&mut s, "undo", json!({}));
        assert_eq!((count(&s.model, "room"), count(&s.model, "floor"), count(&s.model, "ceiling")), (0, 0, 0));
    }
}

#[test]
fn the_finishes_follow_a_wall_that_moves_in_the_same_step() {
    let mut s = Session::new();
    let (walls, room) = boxed(&mut s);
    let (Some(floor), Some(ceiling)) = finishes(&s.model, room) else { panic!() };
    run(&mut s, "move", json!({"ids": [walls[1]], "by": [1000, 0]}));
    assert_eq!((area(&mut s, floor), area(&mut s, ceiling)), (13.44, 13.44));
    assert_eq!(finishes(&s.model, room), (Some(floor), Some(ceiling)), "they are the same elements");
    run(&mut s, "undo", json!({}));
    assert_eq!((area(&mut s, floor), area(&mut s, ceiling)), (10.64, 10.64));
}

#[test]
fn what_changes_elsewhere_leaves_the_finishes_untouched() {
    let mut s = Session::new();
    let (_, room) = boxed(&mut s);
    let (Some(floor), Some(ceiling)) = finishes(&s.model, room) else { panic!() };
    let stamps = s.model.elements.revision_snapshot();
    // A wall that bounds nothing, a column outside, a floor of its own, a material: none of them is the room's.
    run(&mut s, "wall", json!({"points": [[10000, 0], [12000, 0]]}));
    run(&mut s, "floor", json!({"boundary": [[20000, 0], [22000, 0], [22000, 2000], [20000, 2000]]}));
    run(&mut s, "material", json!({"name": "Oak", "color": "#aa8855"}));
    let changed = s.model.elements.revision_snapshot().changed_since(&stamps);
    assert!(!changed.contains(&floor) && !changed.contains(&ceiling) && !changed.contains(&room), "{changed:?}");
}

#[test]
fn an_unbounded_room_keeps_its_finishes_until_it_is_bounded_again() {
    let mut s = Session::new();
    let (walls, room) = boxed(&mut s);
    let (Some(floor), Some(ceiling)) = finishes(&s.model, room) else { panic!() };
    run(&mut s, "delete", json!({"ids": [walls[1]]}));
    assert_eq!(element(&mut s, room)["enclosed"], false);
    assert_eq!((area(&mut s, floor), area(&mut s, ceiling)), (10.64, 10.64), "open, nothing is worked out again");
    // Closed further east, they are found again.
    run(&mut s, "wall", json!({"points": [[5000, 0], [5000, 3000]]}));
    run(&mut s, "modify", json!({"id": walls[0], "end": [5000, 0]}));
    run(&mut s, "modify", json!({"id": walls[2], "start": [5000, 3000]}));
    assert_eq!(element(&mut s, room)["enclosed"], true);
    assert_eq!((area(&mut s, floor), area(&mut s, ceiling)), (13.44, 13.44));
}

#[test]
fn a_room_placed_unbounded_gets_its_finishes_when_walls_close_round_it() {
    let mut s = Session::new();
    finished(&mut s);
    let room = id(&run(&mut s, "room", json!({"point": [2000, 1500], "type": "Finished"})));
    assert_eq!(finishes(&s.model, room), (None, None));
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
    let (Some(floor), Some(_)) = finishes(&s.model, room) else { panic!("no finishes once enclosed") };
    assert_eq!(area(&mut s, floor), 10.64);
}

#[test]
fn a_frozen_room_keeps_its_finishes_as_it_was_frozen() {
    let mut s = Session::new();
    let (walls, room) = boxed(&mut s);
    let (Some(floor), _) = finishes(&s.model, room) else { panic!() };
    run(&mut s, "modify", json!({"id": room, "frozen": true}));
    run(&mut s, "move", json!({"ids": [walls[1]], "by": [1000, 0]}));
    assert_eq!(area(&mut s, floor), 10.64);
    run(&mut s, "modify", json!({"id": room, "frozen": false}));
    assert_eq!(area(&mut s, floor), 13.44);
}

#[test]
fn a_disconnected_finish_is_an_ordinary_one_and_regenerate_makes_another() {
    let mut s = Session::new();
    let (walls, room) = boxed(&mut s);
    let (Some(floor), Some(ceiling)) = finishes(&s.model, room) else { panic!() };
    // Connected, its outline, offset, type and level are the room's.
    for p in [json!({"offset": 100}), json!({"type": "Generic - 200mm"}), json!({"boundary": [[0, 0], [1000, 0], [1000, 1000]]})] {
        let mut p = p;
        p["id"] = json!(floor);
        let e = s.execute("modify", &p).unwrap_err();
        assert!(e.contains("connected to room") && e.contains("room_finish"), "{e}");
    }
    let square = json!([[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]], [[1000, 1000], [0, 1000]], [[0, 1000], [0, 0]]]);
    let e = s.execute("edit_boundary", &json!({"id": ceiling, "sketch": square})).unwrap_err();
    assert!(e.contains("connected to room"), "{e}");
    // Disconnected by the floor itself: it stays as it is, and no longer follows.
    let said = run(&mut s, "room_finish", json!({"id": floor, "action": "disconnect"}));
    assert_eq!((said["room"].as_u64(), said["part"].as_str()), (Some(room), Some("floor")));
    assert_eq!(finishes(&s.model, room), (None, Some(ceiling)));
    assert!(element(&mut s, floor).get("room").is_none());
    run(&mut s, "move", json!({"ids": [walls[1]], "by": [1000, 0]}));
    assert_eq!((area(&mut s, floor), area(&mut s, ceiling)), (10.64, 13.44));
    run(&mut s, "modify", json!({"id": floor, "offset": 100}));
    run(&mut s, "edit_boundary", json!({"id": floor, "sketch": square}));
    // Nothing connects it again, and the room makes no other until told.
    assert!(s.execute("room_finish", &json!({"id": floor, "action": "regenerate"})).is_err());
    assert!(s.execute("modify", &json!({"id": room, "floor": {"connected": floor}})).is_err());
    assert_eq!(count(&s.model, "floor"), 1);
    // Regenerate: a new one, along the room as it is now; the old one stays.
    run(&mut s, "room_finish", json!({"id": room, "part": "floor", "action": "regenerate"}));
    let (Some(new), _) = finishes(&s.model, room) else { panic!("no floor made again") };
    assert_ne!(new, floor);
    assert_eq!((area(&mut s, new), count(&s.model, "floor")), (13.44, 2));
}

#[test]
fn a_connected_finish_deleted_is_let_go_and_can_be_made_again() {
    let mut s = Session::new();
    let (_, room) = boxed(&mut s);
    let (Some(floor), Some(ceiling)) = finishes(&s.model, room) else { panic!() };
    run(&mut s, "delete", json!({"ids": [floor]}));
    let Some(Element::Room(r)) = s.model.elements.get(room) else { panic!() };
    assert_eq!((r.floor, finishes(&s.model, room).1), (Finish::Disconnected, Some(ceiling)));
    run(&mut s, "room_finish", json!({"id": room, "part": "floor", "action": "regenerate"}));
    assert!(finishes(&s.model, room).0.is_some());
}

#[test]
fn a_room_deleted_takes_its_finishes_unless_they_are_kept() {
    let mut s = Session::new();
    let (_, room) = boxed(&mut s);
    let (Some(floor), Some(ceiling)) = finishes(&s.model, room) else { panic!() };
    run(&mut s, "delete", json!({"ids": [room]}));
    assert_eq!((count(&s.model, "floor"), count(&s.model, "ceiling")), (0, 0));
    run(&mut s, "undo", json!({}));
    assert_eq!(finishes(&s.model, room), (Some(floor), Some(ceiling)));
    run(&mut s, "delete", json!({"ids": [room], "keep_finishes": true}));
    assert_eq!((count(&s.model, "room"), count(&s.model, "floor"), count(&s.model, "ceiling")), (0, 1, 1));
    // Ordinary now: it can be edited.
    run(&mut s, "modify", json!({"id": floor, "offset": 0}));
}

#[test]
fn a_rooms_type_and_its_types_settings_reach_its_finishes() {
    let mut s = Session::new();
    let (_, room) = boxed(&mut s);
    let (Some(floor), Some(ceiling)) = finishes(&s.model, room) else { panic!() };
    // The type's ceiling is another type, lower; its floor goes.
    run(
        &mut s,
        "type",
        json!({"category": "room", "name": "Finished", "floor": null, "ceiling": "Tile 600 x 600", "ceiling_offset": 2200}),
    );
    assert_eq!(finishes(&s.model, room), (None, Some(ceiling)));
    assert!(s.model.elements.get(floor).is_none(), "the floor its type no longer has is taken out");
    let c = element(&mut s, ceiling);
    assert_eq!((c["type"].as_str(), c["offset"].as_f64()), (Some("Tile 600 x 600"), Some(2200.0)));
    // On the starter type, it has neither; back on the finished one, both again.
    run(&mut s, "modify", json!({"id": room, "type": "Standard"}));
    assert_eq!((finishes(&s.model, room), count(&s.model, "ceiling")), ((None, None), 0));
    run(&mut s, "type", json!({"category": "room", "name": "Finished", "floor": "Generic - 200mm"}));
    run(&mut s, "modify", json!({"id": room, "type": "Finished"}));
    assert!(matches!(finishes(&s.model, room), (Some(_), Some(_))));
    // A type's floor is a floor type of the project.
    let e = s.execute("type", &json!({"category": "room", "name": "Finished", "floor": "No such"})).unwrap_err();
    assert!(e.contains("floor type"), "{e}");
    let e = s.execute("room", &json!({"point": [50000, 0], "type": "No such"})).unwrap_err();
    assert!(e.contains("no room type"), "{e}");
}

#[test]
fn a_copied_room_has_finishes_of_its_own_and_a_moved_finish_is_put_back() {
    let mut s = Session::new();
    let (walls, room) = boxed(&mut s);
    let (Some(floor), Some(ceiling)) = finishes(&s.model, room) else { panic!() };
    // The room with its walls, 10 m east: the copy is bounded there, with a floor and a ceiling of its own.
    let mut ids = walls.clone();
    ids.push(room);
    let copies = run(&mut s, "copy", json!({"ids": ids, "by": [10000, 0]}));
    let copy = copies["ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_u64)
        .find(|i| matches!(s.model.elements.get(*i), Some(Element::Room(_))))
        .unwrap();
    let (Some(f2), Some(c2)) = finishes(&s.model, copy) else { panic!("the copy has no finishes") };
    assert!(f2 != floor && c2 != ceiling);
    assert_eq!(finishes(&s.model, room), (Some(floor), Some(ceiling)));
    assert_eq!(element(&mut s, f2)["boundary"][0], json!([10100.0, 100.0]));
    // A copy of the floor alone is an ordinary floor.
    let loose = run(&mut s, "copy", json!({"ids": [floor], "by": [0, 10000]}))["ids"][0].as_u64().unwrap();
    assert!(element(&mut s, loose).get("room").is_none());
    run(&mut s, "modify", json!({"id": loose, "offset": 10}));
    // Moved on its own, a connected floor is put back where its room is.
    run(&mut s, "move", json!({"ids": [floor], "by": [500, 0]}));
    assert_eq!(element(&mut s, floor)["boundary"][0], json!([100.0, 100.0]));
}

#[test]
fn a_file_from_before_room_types_opens_with_its_rooms_on_the_starter_type() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
    let room = id(&run(&mut s, "room", json!({"point": [2000, 1500]})));
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(19);
    file["types"].as_object_mut().unwrap().remove("room");
    let library = file["library"].as_object_mut().unwrap();
    let entries = library.len();
    library.retain(|_, e| !(e["name"] == "Standard" && e["kind"].to_string().contains("room")));
    assert_eq!(library.len(), entries - 1, "{library:?}");
    for e in file["elements"].as_object_mut().unwrap().values_mut() {
        if e["category"] == "room" {
            e.as_object_mut().unwrap().remove("type");
        }
    }
    let (m, left_out) = Model::read(&file.to_string()).unwrap();
    assert_eq!((m.format, left_out.len()), (crate::model::FORMAT, 0), "{left_out:?}");
    assert_eq!(m.type_names("room"), ["Standard"]);
    assert!(matches!(m.elements.get(room), Some(Element::Room(r)) if r.type_name == "Standard"));
    assert_eq!(m.check(), Ok(()));
}

#[test]
fn finishes_survive_a_save_and_a_rename_of_their_types() {
    let mut s = Session::new();
    let (walls, room) = boxed(&mut s);
    let (Some(floor), Some(ceiling)) = finishes(&s.model, room) else { panic!() };
    run(&mut s, "rename_type", json!({"category": "floor", "type": "Generic - 200mm", "name": "Screed"}));
    assert_eq!(s.model.types.room["Finished"].floor.as_deref(), Some("Screed"));
    assert_eq!(element(&mut s, floor)["type"], "Screed");
    // A floor type a room type names is in use: Purge Unused leaves it.
    let purged = run(&mut s, "purge", json!({"dry_run": true}))["removed"].to_string();
    assert!(!purged.contains("Screed") && !purged.contains("Finished"), "{purged}");
    let (m, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(left_out.len(), 0, "{left_out:?}");
    assert_eq!(finishes(&m, room), (Some(floor), Some(ceiling)));
    s.reset(m, None);
    run(&mut s, "move", json!({"ids": [walls[1]], "by": [1000, 0]}));
    assert_eq!(area(&mut s, floor), 13.44);
}

#[test]
fn a_rooms_own_colour_from_a_file_before_room_types_becomes_a_type_of_that_colour() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
    run(&mut s, "wall", json!({"points": [[2000, 0], [2000, 3000]]}));
    let rooms = [500, 1500, 3000].map(|x| id(&run(&mut s, "room", json!({"point": [x, 1500]}))));
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(19);
    file["types"].as_object_mut().unwrap().remove("room");
    file["library"].as_object_mut().unwrap().retain(|_, e| !(e["name"] == "Standard" && e["kind"].to_string().contains("room")));
    // As format 19 wrote them: no type, and two of the three a colour of their own, the same one.
    for (i, room) in rooms.iter().enumerate() {
        let e = file["elements"][room.to_string()].as_object_mut().unwrap();
        e.remove("type");
        if i < 2 {
            e.insert("colour".into(), json!([214, 229, 210]));
        }
    }
    let (m, left_out) = Model::read(&file.to_string()).unwrap();
    assert_eq!(left_out.len(), 0, "{left_out:?}");
    assert_eq!(m.type_names("room"), ["Standard", "Standard #D6E5D2"]);
    let of = |room: u64| match m.elements.get(room) {
        Some(Element::Room(r)) => (r.type_name.clone(), r.colour, m.room_colour(r)),
        _ => panic!(),
    };
    assert_eq!(of(rooms[0]), ("Standard #D6E5D2".to_string(), None, Some([214, 229, 210])));
    assert_eq!(of(rooms[1]), of(rooms[0]));
    assert_eq!(of(rooms[2]), ("Standard".to_string(), None, None));
    assert_eq!(m.check(), Ok(()));
    // Saved, the colour is the type's alone.
    let saved = m.to_json().unwrap();
    let again: Value = serde_json::from_str(&saved).unwrap();
    assert!(again["elements"][rooms[0].to_string()].get("colour").is_none());
    assert_eq!(again["types"]["room"]["Standard #D6E5D2"]["colour"], json!([214, 229, 210]));
}
