//! Room Area Computation: rooms measured at the walls' centre lines or at their finish faces, as a project setting.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Model;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// A 4000 x 3000 box of 300 mm walls on their centre lines, with a room in it.
fn box_with_room(s: &mut Session) -> u64 {
    run(s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true, "type": "Exterior - 300mm"}));
    run(s, "room", json!({"point": [2000, 1500], "name": "TEST"}))["id"].as_u64().unwrap_or_default()
}

fn area(s: &mut Session) -> f64 {
    run(s, "elements", json!({"category": "room"}))["elements"][0]["area_m2"].as_f64().unwrap_or(f64::NAN)
}

#[test]
fn a_room_is_measured_at_the_centre_lines_or_inside_the_finish_faces() {
    let mut s = Session::new();
    box_with_room(&mut s);
    assert_eq!(run(&mut s, "room_area", json!({})), json!({"at": "centre"}), "centre unless set");
    assert_eq!(area(&mut s), 12.0);
    assert_eq!(run(&mut s, "room_area", json!({"at": "finish"})), json!({"at": "finish"}));
    assert_eq!(area(&mut s), 9.99, "(4000 - 300) x (3000 - 300)");
    assert_eq!(run(&mut s, "inspect", json!({}))["room_area"], "finish");
    // Asking without `at` changes nothing.
    assert_eq!(run(&mut s, "room_area", json!({})), json!({"at": "finish"}));
    run(&mut s, "undo", json!({}));
    assert_eq!(area(&mut s), 12.0);
    assert_eq!(run(&mut s, "inspect", json!({}))["room_area"], "centre");
}

#[test]
fn a_room_separation_line_bounds_a_room_at_the_finish_faces_too() {
    let mut s = Session::new();
    box_with_room(&mut s);
    run(&mut s, "room_separator", json!({"points": [[2000, 0], [2000, 3000]]}));
    run(&mut s, "room", json!({"point": [3000, 1500]}));
    run(&mut s, "room_area", json!({"at": "finish"}));
    let rooms = run(&mut s, "elements", json!({"category": "room"}));
    let mut areas: Vec<f64> = rooms["elements"].as_array().unwrap().iter().filter_map(|e| e["area_m2"].as_f64()).collect();
    areas.sort_by(f64::total_cmp);
    // Half the box each: 1850 x 2700 between the finish faces and the separation line, which has no thickness.
    assert_eq!(areas.len(), 2);
    assert!(areas.iter().all(|a| (a - 4.995).abs() <= 0.006), "{areas:?}");
}

#[test]
fn the_setting_is_saved_and_a_file_without_it_reads_as_centre() {
    let mut s = Session::new();
    box_with_room(&mut s);
    run(&mut s, "room_area", json!({"at": "finish"}));
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-room-area.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(run(&mut t, "inspect", json!({}))["room_area"], "finish");
    assert_eq!(area(&mut t), 9.99);
    let _ = std::fs::remove_file(path);
    // A file from before the setting has no such field, and is read with nothing left out.
    let mut file = serde_json::to_value(&*s.model).unwrap();
    assert_eq!(file["room_area"], "finish");
    file.as_object_mut().unwrap().remove("room_area");
    let (m, left_out) = Model::read(&file.to_string()).unwrap();
    assert_eq!((m.room_area, left_out.len()), (crate::model::RoomArea::Centre, 0));
    // And a project on the default doesn't write it.
    assert!(serde_json::to_value(Model::new("t")).unwrap().get("room_area").is_none());
}

#[test]
fn the_command_names_what_there_is_when_given_what_there_isnt() {
    let mut s = Session::new();
    for bad in [json!({"at": "core"}), json!({"at": 5}), json!({"at": [0, 0]})] {
        let e = s.execute("room_area", &bad).unwrap_err();
        assert!(e.contains("centre") && e.contains("finish"), "{bad}: {e}");
    }
    assert_eq!(s.model.room_area, crate::model::RoomArea::Centre);
}

/// Every room's area, smallest first.
fn areas(s: &mut Session) -> Vec<f64> {
    let rooms = run(s, "elements", json!({"category": "room"}));
    let mut list: Vec<f64> = rooms["elements"].as_array().unwrap().iter().filter_map(|e| e["area_m2"].as_f64()).collect();
    list.sort_by(f64::total_cmp);
    list
}

fn wall(s: &mut Session, points: Value, closed: bool) {
    run(s, "wall", json!({"points": points, "closed": closed, "type": "Exterior - 300mm"}));
}

fn near(got: f64, want: f64) {
    assert!((got - want).abs() <= 0.006, "{got} isn't {want}");
}

#[test]
fn a_room_cannot_lie_inside_a_wall() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "Exterior - 300mm"}));
    for at in ["centre", "finish"] {
        run(&mut s, "room_area", json!({"at": at}));
        for point in [[2000, 0], [2000, 100]] {
            let e = s.execute("room", &json!({"point": point})).unwrap_err();
            if at == "finish" {
                assert!(e.contains("inside wall"), "{at} {point:?}: {e}");
            }
            assert!(areas(&mut s).is_empty(), "{at} {point:?}: no room was made");
        }
    }
}

#[test]
fn a_room_in_the_clear_next_to_a_wall_is_still_a_room_at_the_finish_faces() {
    let mut s = Session::new();
    box_with_room(&mut s);
    run(&mut s, "room_area", json!({"at": "finish"}));
    // In the box, 20 mm from the inner face of the south wall.
    let made = run(&mut s, "room", json!({"point": [2000, 170]}));
    near(made["area_m2"].as_f64().unwrap_or_default(), 9.99);
    // In the south wall's body, 10 mm from that face.
    assert!(s.execute("room", &json!({"point": [2000, 140]})).unwrap_err().contains("inside wall"));
}

#[test]
fn an_l_shaped_room_is_measured_inside_its_walls() {
    let mut s = Session::new();
    wall(&mut s, json!([[0, 0], [6000, 0], [6000, 2000], [2000, 2000], [2000, 5000], [0, 5000]]), true);
    run(&mut s, "room", json!({"point": [1000, 1000]}));
    near(areas(&mut s)[0], 18.0); // 6000 x 2000 + 2000 x 3000 on the centre lines
    run(&mut s, "room_area", json!({"at": "finish"}));
    // 5700 x 1700 + 1700 x 3000
    near(areas(&mut s)[0], 14.79);
}

#[test]
fn an_island_of_walls_is_taken_from_the_room_at_the_finish_faces() {
    let mut s = Session::new();
    wall(&mut s, json!([[0, 0], [6000, 0], [6000, 5000], [0, 5000]]), true);
    wall(&mut s, json!([[2500, 2000], [3500, 2000], [3500, 3000], [2500, 3000]]), true);
    run(&mut s, "room", json!({"point": [1000, 1000]}));
    near(areas(&mut s)[0], 29.0); // 6000 x 5000 less 1000 x 1000, both on centre lines
    run(&mut s, "room_area", json!({"at": "finish"}));
    // 5700 x 4700 less the island's outer faces, 1300 x 1300
    near(areas(&mut s)[0], 25.1);
    // The island is a closed loop of walls, so its clear inside (700 x 700) is a room of its own.
    assert!(
        s.execute("room", &json!({"point": [3000, 2500]})).is_ok_and(|v| v["area_m2"].as_f64().is_some_and(|a| (a - 0.49).abs() < 0.006))
    );
}

#[test]
fn four_rooms_round_a_wall_crossing_are_each_measured_at_the_finish_faces() {
    let mut s = Session::new();
    wall(&mut s, json!([[0, 0], [6000, 0], [6000, 4000], [0, 4000]]), true);
    wall(&mut s, json!([[3000, 0], [3000, 4000]]), false);
    wall(&mut s, json!([[0, 2000], [6000, 2000]]), false);
    for point in [[1500, 1000], [4500, 1000], [1500, 3000], [4500, 3000]] {
        run(&mut s, "room", json!({"point": point}));
    }
    assert_eq!(areas(&mut s), vec![6.0; 4], "3000 x 2000 on the centre lines");
    run(&mut s, "room_area", json!({"at": "finish"}));
    let got = areas(&mut s);
    assert_eq!(got.len(), 4);
    for a in got {
        near(a, 4.59); // (3000 - 300) x (2000 - 300)
    }
    // The crossing itself is wall, not a room.
    assert!(s.execute("room", &json!({"point": [3000, 2000]})).unwrap_err().contains("inside wall"));
}

#[test]
fn the_area_at_the_finish_faces_is_the_same_wherever_the_wall_is_located() {
    let finish = |location: &str, points: Value| {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": points, "closed": true, "type": "Exterior - 300mm", "location": location}));
        run(&mut s, "room_area", json!({"at": "finish"}));
        run(&mut s, "room", json!({"point": [2000, 1500]}));
        areas(&mut s)[0]
    };
    let centre = finish("centre", json!([[0, 0], [4000, 0], [4000, 3000], [0, 3000]]));
    near(centre, 9.99);
    // The same walls, drawn clockwise (their exterior side is to the left) from their outer face: the points are 150 mm further out.
    let outer = finish("exterior-finish", json!([[-150, -150], [-150, 3150], [4150, 3150], [4150, -150]]));
    near(outer, centre);
    // And from their inner face, 150 mm further in.
    let inner = finish("interior-finish", json!([[150, 150], [150, 2850], [3850, 2850], [3850, 150]]));
    near(inner, centre);
}

/// A 4000 x 3000 box of 300 mm walls on their centre lines, with no room in it.
fn box_of_walls(s: &mut Session) {
    wall(s, json!([[0, 0], [4000, 0], [4000, 3000], [0, 3000]]), true);
}

/// A room placed at the centre lines with its point in a wall's inner half is, measured at the faces, inside the wall: its
/// faces close round the point, but the wall is no room. Everything that measures it says so, the same way.
#[test]
fn a_room_left_inside_a_wall_by_measuring_at_the_faces_is_not_enclosed_everywhere() {
    let mut s = Session::new();
    box_of_walls(&mut s);
    let room = run(&mut s, "room", json!({"point": [2000, 100], "name": "TEST", "number": "1"}))["id"].as_u64().unwrap();
    assert_eq!(areas(&mut s), vec![12.0], "at the centre lines it is the box");
    run(&mut s, "room_area", json!({"at": "finish"}));
    let e = &run(&mut s, "elements", json!({"category": "room"}))["elements"][0];
    assert_eq!((e["enclosed"].clone(), e["area_m2"].clone()), (json!(false), json!(0.0)), "not the wall's body: {e}");
    // Inspect says why, not that the walls stopped closing.
    let warnings = run(&mut s, "inspect", json!({}))["warnings"].to_string();
    assert!(warnings.contains(&format!("room {room} (1 TEST) lies inside wall")), "{warnings}");
    // The plan's tag says NOT ENCLOSED under its name and number, rather than the wall's area.
    let level = s.model.levels()[0].0;
    let d = crate::render::plan(&s.model, level, 800, 600);
    let tag = d.owners.iter().filter(|o| **o == Some(room)).count();
    let strokes = |t: &str| crate::font::strokes(t, 0.0, 0.0, 12.0).len();
    assert_eq!(tag, strokes("TEST") + strokes("1") + strokes("NOT ENCLOSED"));
    // IFC gives the space no shape, as it does a room whose walls don't close.
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains(",$,'TEST',.ELEMENT.,.SPACE.,$)"), "{ifc}");
}

/// The same room in a project measured at the centre lines is the box, so IFC gives its gross quantities; at the faces it
/// is no room, so there are no net ones.
#[test]
fn ifc_keeps_the_gross_quantities_of_a_room_with_no_net_ones() {
    let mut s = Session::new();
    box_of_walls(&mut s);
    run(&mut s, "room", json!({"point": [2000, 100]}));
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCQUANTITYAREA('GrossFloorArea',$,$,12."), "{ifc}");
    assert!(ifc.contains("'GrossPerimeter'"), "{ifc}");
    assert!(!ifc.contains("'NetFloorArea'") && !ifc.contains("'NetPerimeter'"), "not the wall's body: {ifc}");
}
