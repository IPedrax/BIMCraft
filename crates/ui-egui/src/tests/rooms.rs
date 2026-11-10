//! Rooms in the window: the Room Separator tool beside Room, a room frozen from its Properties, its height there, and the
//! reference cross Visibility/Graphics shows.

use super::*;
use egui_kittest::kittest::NodeT;

fn separators(h: &Harness<BimApp>) -> Vec<(P2, P2)> {
    h.state()
        .session
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::RoomSeparator(rs) => Some((rs.start, rs.end)),
            _ => None,
        })
        .collect()
}

/// A fold arrow in Visibility/Graphics, scrolled to in its table first, as a user would.
fn unfold(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    press(h, label);
}

#[test]
fn the_room_separator_tool_beside_room_lays_a_line_each_click_until_escape() {
    let mut h = app(BOX);
    tool(&mut h, "Room Separator");
    assert_eq!(h.state().tool, Tool::RoomSeparator(None));
    click(&mut h, P2(4000.0, 0.0));
    assert!(separators(&h).is_empty(), "the first click is the line's start");
    click(&mut h, P2(4000.0, 3000.0));
    click(&mut h, P2(4000.0, 6000.0));
    assert_eq!(separators(&h), [(P2(4000.0, 0.0), P2(4000.0, 3000.0)), (P2(4000.0, 3000.0), P2(4000.0, 6000.0))]);
    // Esc ends the chain; the next click starts another.
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::RoomSeparator(None));
    click(&mut h, P2(0.0, 3000.0));
    assert_eq!(separators(&h).len(), 2);
    // The rooms on each side of it are two.
    key(&mut h, Key::Escape);
    tool(&mut h, "Room");
    click(&mut h, P2(2000.0, 3000.0));
    let room = find(&h, |e| matches!(e, Element::Room(_)));
    assert_eq!(field(&h, room, "area_m2"), json!(24.0));
}

#[test]
fn a_room_is_frozen_from_its_properties_and_keeps_its_size() {
    let mut h = app(&format!("{BOX}\nroom {{\"point\": [2000, 3000], \"name\": \"Hall\"}}"));
    let room = find(&h, |e| matches!(e, Element::Room(_)));
    click(&mut h, P2(2000.0, 3000.0));
    assert_eq!(h.state().selection, [room]);
    for label in ["Upper Limit", "Limit Offset", "Unbounded Height", "Volume (m³)"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert_eq!(field(&h, room, "volume_m3"), json!(129.6), "48 m² by 2700 mm, up to 300 mm under Level 2");
    // At the foot of the palette, past its fold.
    h.get_by_role_and_label(Role::CheckBox, "Frozen").scroll_to_me();
    h.run();
    pick(&mut h, Role::CheckBox, "Frozen");
    assert_eq!(field(&h, room, "frozen"), json!(true));
    // A wall moved: the frozen room keeps its 48 m².
    let east = find(&h, |e| matches!(e, Element::Wall(w) if w.start.0 == 8000.0 && w.end.0 == 8000.0));
    let moved = h.state_mut().run("move", json!({"ids": [east], "by": [2000, 0]}));
    assert!(moved.is_some());
    assert_eq!(field(&h, room, "area_m2"), json!(48.0));
    // Its limit offset typed: up to Level 2 itself, taller, more volume.
    type_role(&mut h, Role::TextInput, "Limit Offset", "0");
    assert_eq!(field(&h, room, "volume_m3"), json!(144.0));
}

#[test]
fn visibility_graphics_shows_the_rooms_reference_points() {
    let mut h = app(&format!("{BOX}\nroom {{\"point\": [2000, 3000]}}"));
    let shown = |h: &Harness<BimApp>| !h.state().graphics_shown().hides("roomreference");
    assert!(!shown(&h), "off unless shown");
    shortcut(&mut h, "VV");
    unfold(&mut h, "Rooms: Unfold");
    pick(&mut h, Role::CheckBox, "Reference");
    press(&mut h, "OK");
    assert!(shown(&h));
    h.state_mut().run("undo", json!({}));
    assert!(!shown(&h));
}

#[test]
fn visibility_graphics_folds_a_categorys_subcategories_under_it_indented() {
    let mut h = app(BOX);
    shortcut(&mut h, "VV");
    assert!(h.query_by_label("Reference").is_none(), "folded at first");
    unfold(&mut h, "Rooms: Unfold");
    let left = |h: &Harness<BimApp>, label: &str| h.get_by_role_and_label(Role::CheckBox, label).rect().left();
    assert!(left(&h, "Reference") - left(&h, "Rooms") >= 24.0, "a subcategory sits well in from its category");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Room Separation Lines").is_some());
    unfold(&mut h, "Rooms: Fold");
    assert!(h.query_by_label("Reference").is_none());
    // A search finds a subcategory, its category unfolded round it.
    type_into(&mut h, "Category name search:", "refer");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Reference").is_some());
}

#[test]
fn a_walls_room_bounding_is_switched_off_in_its_properties() {
    let mut h = app(&format!("{BOX}\nwall {{\"points\": [[4000,0],[4000,6000]]}}\nroom {{\"point\": [2000, 3000]}}"));
    let room = find(&h, |e| matches!(e, Element::Room(_)));
    assert_eq!(field(&h, room, "area_m2"), json!(24.0));
    click(&mut h, P2(4000.0, 4500.0));
    let wall = h.state().selection[0];
    assert_eq!(field(&h, wall, "room_bounding"), json!(true));
    h.get_by_role_and_label(Role::CheckBox, "Room Bounding").scroll_to_me();
    h.run();
    pick(&mut h, Role::CheckBox, "Room Bounding");
    assert_eq!(field(&h, wall, "room_bounding"), json!(false));
    assert_eq!(field(&h, room, "area_m2"), json!(48.0), "the room runs through it");
}

#[test]
fn a_rooms_upper_limit_is_a_level_or_not_bounded_keeping_its_top() {
    let mut h = app(&format!("{BOX}\nroom {{\"point\": [2000, 3000]}}"));
    let room = find(&h, |e| matches!(e, Element::Room(_)));
    click(&mut h, P2(2000.0, 3000.0));
    // 300 mm under Level 2, at 3000.
    assert_eq!(field(&h, room, "unbounded_height"), json!(2700.0));
    choose(&mut h, "Upper Limit", "Not Bounded");
    assert_eq!((field(&h, room, "top_level"), field(&h, room, "limit_offset")), (Value::Null, json!(2700.0)));
    assert_eq!(field(&h, room, "unbounded_height"), json!(2700.0), "its top stays");
}

#[test]
fn a_3d_views_rooms_are_unticked_until_ticked_in_visibility_graphics() {
    let mut h = app(&format!("{BOX}\nroom {{\"point\": [2000, 3000]}}"));
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().sheet, Sheet::View3D);
    let shown = |h: &Harness<BimApp>| h.state().graphics_shown().shows_in_3d("room");
    assert!(!shown(&h));
    shortcut(&mut h, "VV");
    let ticked = |h: &Harness<BimApp>| {
        use egui_kittest::kittest::NodeT;
        h.get_by_role_and_label(Role::CheckBox, "Rooms").accesskit_node().toggled()
    };
    assert_eq!(ticked(&h), Some(egui::accesskit::Toggled::False), "unticked in a new 3D view");
    pick(&mut h, Role::CheckBox, "Rooms");
    press(&mut h, "OK");
    assert!(shown(&h));
}

/// The palette reads its elements' JSON every frame, and a component's says the room it stands in, which costs its level's room
/// boundaries: it is found once a model, for the whole selection together, and found again after a change.
#[test]
fn the_palette_finds_the_room_its_components_stand_in_once_a_model() {
    let script = format!(
        "{BOX}\nwall {{\"points\": [[4000, 0], [4000, 6000]]}}\nroom {{\"point\": [2000, 3000], \"name\": \"West\"}}\n\
         room {{\"point\": [6000, 3000], \"name\": \"East\"}}\ncomponent {{\"family\": \"Chair\", \"at\": [1000, 1000]}}\n\
         component {{\"family\": \"Chair\", \"at\": [3000, 1000]}}"
    );
    let mut h = app(&script);
    let chairs = ids(&h, "component");
    h.state_mut().selection = chairs.clone();
    h.run();
    // What the palette keeps of the model the window holds now.
    let kept = |h: &Harness<BimApp>| -> Vec<Arc<Value>> {
        let app = h.state();
        let kept = app.palette_json.borrow();
        match kept.as_ref() {
            Some((m, json)) if m.ptr_eq(&Arc::downgrade(&app.session.model)) => {
                chairs.iter().filter_map(|id| json.get(id).cloned()).collect()
            }
            _ => Vec::new(),
        }
    };
    let first = kept(&h);
    assert_eq!(first.len(), 2, "both found in the frame that showed them");
    assert!(first.iter().all(|j| j["room_name"] == "West"), "{first:?}");
    h.run();
    let again = kept(&h);
    assert!(again.len() == 2 && first.iter().zip(&again).all(|(a, b)| Arc::ptr_eq(a, b)), "a frame that changes nothing finds none again");
    // Moved into the east room: found again, in it.
    h.state_mut().session.execute("move", &json!({"ids": chairs, "by": [4000, 0]})).unwrap();
    h.run();
    let moved = kept(&h);
    assert!(moved.len() == 2 && moved.iter().all(|j| j["room_name"] == "East"), "{moved:?}");
/// A room of a type with a floor and a ceiling, each made for it at the walls' faces.
const FINISHED: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
room {"point": [4000, 3000]}
type {"category": "room", "name": "Standard", "floor": "Generic - 200mm", "ceiling": "Generic - 20mm"}"#;

fn finish_of_room(h: &Harness<BimApp>, room: Id, part: bimcraft_core::model::Part) -> bimcraft_core::model::Finish {
    match h.state().session.model.elements.get(room) {
        Some(Element::Room(r)) => r.finish(part),
        _ => panic!("no room {room}"),
    }
}

#[test]
fn a_rooms_floor_and_ceiling_head_its_properties_with_disconnect_then_regenerate_in_both_profiles() {
    use bimcraft_core::model::{Finish, Part};
    for rev in [false, true] {
        let mut h = if rev { app(FINISHED) } else { bimcraft(FINISHED) };
        let room = ids(&h, "room")[0];
        let (floor, ceiling) = (ids(&h, "floor")[0], ids(&h, "ceiling")[0]);
        h.state_mut().selection = vec![room];
        h.run();
        // Each with its type and its id, a link that selects it.
        assert!(h.query_by_label("Generic - 200mm").is_some() && h.query_by_label("Generic - 20mm").is_some(), "rev {rev}");
        assert!(h.query_by_role_and_label(Role::Link, &format!("Floor {floor}")).is_some(), "rev {rev}");
        assert!(h.query_by_role_and_label(Role::Link, &format!("Ceiling {ceiling}")).is_some(), "rev {rev}");
        press(&mut h, "Disconnect Floor");
        assert_eq!(finish_of_room(&h, room, Part::Floor), Finish::Disconnected, "rev {rev}");
        assert!(matches!(finish_of_room(&h, room, Part::Ceiling), Finish::Connected(_)), "rev {rev}");
        assert_eq!(h.state().session.model.finish_of(floor), None, "rev {rev}: an ordinary floor now");
        // No way back to it: the button makes another.
        assert!(h.query_by_label("Disconnect Floor").is_none(), "rev {rev}");
        press(&mut h, "Regenerate Floor");
        assert!(matches!(finish_of_room(&h, room, Part::Floor), Finish::Connected(_)), "rev {rev}");
        assert_eq!(ids(&h, "floor").len(), 2, "rev {rev}: the one let go, and the room's new one");
        // The link selects the ceiling, which names its room.
        h.get_by_role_and_label(Role::Link, &format!("Ceiling {ceiling}")).click();
        h.run();
        assert_eq!(h.state().selection, [ceiling], "rev {rev}");
        assert!(h.query_by_label("Connected to").is_some(), "rev {rev}");
    }
}

#[test]
fn a_room_without_a_floor_or_a_ceiling_of_its_own_has_no_such_rows() {
    let mut h = app(&format!("{BOX}\nroom {{\"point\": [4000, 3000]}}\nfloor {{}}"));
    for category in ["room", "floor"] {
        h.state_mut().selection = vec![ids(&h, category)[0]];
        h.run();
        for label in ["Connected Floor and Ceiling", "Connected Room", "Connected to", "Disconnect", "Disconnect Floor"] {
            assert!(h.query_by_label(label).is_none(), "{category}: {label}");
        }
    }
}

#[test]
fn a_floor_that_follows_a_room_names_it_and_is_its_rooms_until_disconnected() {
    let mut h = app(FINISHED);
    let (room, floor) = (ids(&h, "room")[0], ids(&h, "floor")[0]);
    h.state_mut().selection = vec![floor];
    h.run();
    assert!(h.query_by_label("Connected to").is_some());
    assert!(h.query_by_role_and_label(Role::Link, &format!("Room {room}")).is_some());
    // Its outline, its type and its height are the room's.
    press(&mut h, "Modify | Floors");
    assert!(h.get_by_label("Edit Boundary").accesskit_node().is_disabled());
    assert!(h.get_by_label("Type Selector").accesskit_node().is_disabled());
    assert!(h.query_by_role_and_label(Role::TextInput, "Height Offset From Level").is_none());
    press(&mut h, "Disconnect");
    assert_eq!(h.state().session.model.finish_of(floor), None);
    assert_eq!(finish_of_room(&h, room, bimcraft_core::model::Part::Floor), bimcraft_core::model::Finish::Disconnected);
    assert!(h.query_by_label("Connected to").is_none());
    assert!(!h.get_by_label("Edit Boundary").accesskit_node().is_disabled());
    assert!(!h.get_by_label("Type Selector").accesskit_node().is_disabled());
}

#[test]
fn deleting_a_room_asks_whether_its_floor_and_ceiling_go_with_it() {
    for (answer, rooms, finishes) in [("Cancel", 1, 2), ("Keep Them", 0, 2), ("Delete Them Too", 0, 0)] {
        let mut h = app(FINISHED);
        h.state_mut().selection = ids(&h, "room");
        h.run();
        key(&mut h, Key::Delete);
        assert_eq!(ids(&h, "room").len(), 1, "{answer}: asked first");
        press(&mut h, answer);
        assert!(h.state().deleting_rooms.is_none(), "{answer}");
        assert_eq!(ids(&h, "room").len(), rooms, "{answer}");
        assert_eq!(ids(&h, "floor").len() + ids(&h, "ceiling").len(), finishes, "{answer}");
        let m = &h.state().session.model;
        assert!(ids(&h, "floor").iter().all(|id| m.finish_of(*id).is_none() || rooms == 1), "{answer}: kept as ordinary ones");
    }
    // A room with none, and a room whose floor and ceiling are selected with it, go without a question.
    let mut h = app(&format!("{BOX}\nroom {{\"point\": [4000, 3000]}}"));
    h.state_mut().selection = ids(&h, "room");
    h.run();
    key(&mut h, Key::Delete);
    assert!(ids(&h, "room").is_empty());
    let mut h = app(FINISHED);
    h.state_mut().selection = [ids(&h, "room"), ids(&h, "floor"), ids(&h, "ceiling")].concat();
    h.run();
    key(&mut h, Key::Delete);
    assert!(h.state().deleting_rooms.is_none() && ids(&h, "room").is_empty() && ids(&h, "floor").is_empty());
}

#[test]
fn the_room_tool_places_rooms_of_the_type_its_selector_shows_with_their_floor() {
    let script = format!("{BOX}\ntype {{\"category\": \"room\", \"name\": \"Wet\", \"floor\": \"Generic - 200mm\"}}");
    let mut h = app(&script);
    tool(&mut h, "Room");
    h.state_mut().types.insert("room", "Wet".into());
    h.run();
    click(&mut h, P2(4000.0, 3000.0));
    let room = ids(&h, "room")[0];
    assert_eq!(field(&h, room, "type"), json!("Wet"));
    assert_eq!((ids(&h, "floor").len(), ids(&h, "ceiling").len()), (1, 0));
}
