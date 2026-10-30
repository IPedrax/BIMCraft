//! Rooms in the window: the Room Separator tool beside Room, a room frozen from its Properties, its height there, and the
//! reference cross Visibility/Graphics shows.

use super::*;

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
