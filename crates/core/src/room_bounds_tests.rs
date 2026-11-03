//! What bounds a room: walls and columns with Room Bounding on, where the level's Computation Height cuts them, and room
//! separation lines; two rooms never sharing a space; an unbounded room's square; and its reference cross, a subcategory.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::render::{View, draw};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

fn room(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({"category": "room"}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap()
}

fn area(s: &mut Session, id: u64) -> f64 {
    room(s, id)["area_m2"].as_f64().unwrap()
}

/// A 6000 x 4000 box of walls on their centre lines, on Level 1.
fn box_of_walls(s: &mut Session) {
    run(s, "wall", json!({"points": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "closed": true}));
}

#[test]
fn a_wall_with_room_bounding_off_is_passed_through() {
    let mut s = Session::new();
    box_of_walls(&mut s);
    let part = run(&mut s, "wall", json!({"points": [[3000, 0], [3000, 4000]]}))["ids"][0].as_u64().unwrap();
    let r = id(&run(&mut s, "room", json!({"point": [1500, 2000]})));
    assert_eq!(area(&mut s, r), 12.0);
    assert_eq!(run(&mut s, "elements", json!({"category": "wall"}))["elements"][0]["room_bounding"], true, "on unless set");
    run(&mut s, "modify", json!({"id": part, "room_bounding": false}));
    assert_eq!(area(&mut s, r), 24.0, "the whole box");
    run(&mut s, "room_area", json!({"at": "finish"}));
    assert!(area(&mut s, r) > 20.0, "at the faces too");
}

#[test]
fn a_column_bounds_rooms_unless_its_room_bounding_is_off() {
    let mut s = Session::new();
    box_of_walls(&mut s);
    let r = id(&run(&mut s, "room", json!({"point": [1000, 1000]})));
    let c = id(&run(&mut s, "column", json!({"at": [3000, 2000], "type": "Rectangular 400 x 400"})));
    assert_eq!(area(&mut s, r), 23.84, "less the column standing free in it");
    run(&mut s, "modify", json!({"id": c, "room_bounding": false}));
    assert_eq!(area(&mut s, r), 24.0);
}

#[test]
fn walls_bound_rooms_where_the_levels_computation_height_cuts_them() {
    let mut s = Session::new();
    box_of_walls(&mut s);
    // A low wall, 1000 mm tall: under the 1200 mm the level finds rooms at.
    run(&mut s, "wall", json!({"points": [[3000, 0], [3000, 4000]], "height": 1000, "top_level": null}));
    let r = id(&run(&mut s, "room", json!({"point": [1500, 2000]})));
    assert_eq!(area(&mut s, r), 24.0);
    let level = run(&mut s, "elements", json!({"category": "level"}))["elements"][0].clone();
    assert_eq!(level["computation_height"], 1200.0);
    run(&mut s, "modify", json!({"id": level["id"], "computation_height": 500}));
    assert_eq!(area(&mut s, r), 12.0);
    // A room separation line bounds whatever the height: it has none.
    run(&mut s, "room_separator", json!({"points": [[0, 3000], [3000, 3000]]}));
    assert_eq!(area(&mut s, r), 9.0, "the separator splits the half");
}

#[test]
fn two_rooms_cannot_share_a_space_the_later_is_left_unbounded() {
    let mut s = Session::new();
    box_of_walls(&mut s);
    let first = id(&run(&mut s, "room", json!({"point": [1000, 1000], "name": "First"})));
    let made = run(&mut s, "room", json!({"point": [5000, 3000], "name": "Second"}));
    let second = id(&made);
    assert!(made["warning"].as_str().is_some_and(|w| w.contains("already in this space")), "{made}");
    assert_eq!((area(&mut s, first), area(&mut s, second)), (24.0, 0.0));
    assert_eq!(room(&mut s, second)["shares_space_with"], first);
    let warnings = run(&mut s, "inspect", json!({}))["warnings"].to_string();
    assert!(warnings.contains("Second") && warnings.contains("can't share"), "{warnings}");
    // Parted by a separation line, each has its own; the first gone, the second has it all.
    run(&mut s, "room_separator", json!({"points": [[3000, 0], [3000, 4000]]}));
    assert_eq!((area(&mut s, first), area(&mut s, second)), (12.0, 12.0));
    run(&mut s, "undo", json!({}));
    run(&mut s, "delete", json!({"ids": [first]}));
    assert_eq!(area(&mut s, second), 24.0);
}

#[test]
fn an_unbounded_room_covers_a_square_round_its_point() {
    let mut s = Session::new();
    let made = run(&mut s, "room", json!({"point": [500, 500]}));
    assert!(made["warning"].as_str().is_some_and(|w| w.contains("isn't enclosed")), "{made}");
    let Some(crate::model::Element::Room(r)) = s.model.elements.get(id(&made)) else { panic!("no room") };
    let (outline, bounded) = s.model.room_outline(r);
    assert!(!bounded);
    assert_eq!(outline, [P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 1000.0), P2(0.0, 1000.0)]);
    let e = s.execute("modify", &json!({"id": id(&made), "frozen": true})).unwrap_err();
    assert!(e.contains("unbounded"), "{e}");
}

#[test]
fn rooms_reference_and_separation_lines_are_subcategories_of_rooms() {
    let mut s = Session::new();
    box_of_walls(&mut s);
    let g = |s: &Session| crate::graphics::graphics_of(&s.model, 100000).cloned().unwrap_or_default();
    assert!(g(&s).hides("roomreference") && !g(&s).hides("roomseparator"), "the reference off unless shown");
    run(&mut s, "graphics", json!({"view": 100000, "show": ["roomreference"]}));
    assert!(!g(&s).hides("roomreference"));
    run(&mut s, "graphics", json!({"view": 100000, "hide": ["rooms"]}));
    assert!(g(&s).hides("roomreference") && g(&s).hides("roomseparator"), "hidden with their category");
    let e = s.execute("graphics", &json!({"view": 100000, "hide": ["nonsense"]})).unwrap_err();
    assert!(e.contains("roomreference"), "named among what there is: {e}");
}

#[test]
fn the_reference_runs_from_the_rooms_point_to_its_boundary_and_a_click_on_it_picks_the_room() {
    let mut s = Session::new();
    box_of_walls(&mut s);
    let r = id(&run(&mut s, "room", json!({"point": [1000, 2000]})));
    run(&mut s, "graphics", json!({"view": 100000, "show": ["roomreference"]}));
    let view = View { scale: 0.1, cx: 3000.0, cy: 2000.0, width: 800, height: 600 };
    let pick = |s: &Session, p: P2| draw(&s.model, 100000, view).pick(view.px(p), 2.0);
    // Up and right from the point at 45°: on the cross well away from the point and the label.
    assert_eq!(pick(&s, P2(2500.0, 3500.0)), Some(r), "on the cross");
    // Down and left it meets the west wall's line at x = 0, y = 1000: past there, no cross.
    assert_eq!(pick(&s, P2(500.0, 1500.0)), Some(r));
    assert_ne!(pick(&s, P2(-300.0, 700.0)), Some(r), "it stops at the boundary");
    run(&mut s, "graphics", json!({"view": 100000, "hide": ["roomreference"]}));
    assert_ne!(pick(&s, P2(2500.0, 3500.0)), Some(r), "hidden, no cross to pick");
}

#[test]
fn rooms_are_off_in_every_new_3d_view_until_shown() {
    let mut s = Session::new();
    box_of_walls(&mut s);
    run(&mut s, "room", json!({"point": [1000, 1000]}));
    let g3 = |s: &Session| s.model.default_3d().and_then(|v| crate::graphics::graphics_of(&s.model, v)).cloned().unwrap_or_default();
    assert!(!g3(&s).shows_in_3d("room"), "off unless shown");
    let plan = crate::graphics::graphics_of(&s.model, 100000).cloned().unwrap_or_default();
    assert!(!plan.hides("room"), "a plan draws them");
    run(&mut s, "graphics", json!({"view": "{3D}", "show": ["rooms"]}));
    assert!(g3(&s).shows_in_3d("room"));
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["rooms"]}));
    assert!(!g3(&s).shows_in_3d("room"));
}
