//! A room's height, as Revit's: its Upper Limit and Limit Offset, its Base Offset, its Unbounded Height and its volume; and
//! its reference point, which a plan draws as a cross when its graphics show Rooms > Reference.

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// A 4000 x 3000 box of walls on Level 1 (at 0), and a room in it; Level 2 at 3000.
fn box_with_room(s: &mut Session, room: Value) -> u64 {
    run(s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
    let mut p = json!({"point": [2000, 1500]});
    if let (Some(p), Some(r)) = (p.as_object_mut(), room.as_object()) {
        p.extend(r.clone());
    }
    run(s, "room", p)["id"].as_u64().unwrap()
}

fn room(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({"category": "room"}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap()
}

#[test]
fn a_room_reaches_300_mm_under_the_level_above_unless_told_and_its_volume_is_its_area_times_that() {
    let mut s = Session::new();
    let id = box_with_room(&mut s, json!({}));
    let r = room(&mut s, id);
    // Level 2 is at 3000.
    assert_eq!((r["unbounded_height"].as_f64(), r["volume_m3"].as_f64()), (Some(2700.0), Some(32.4)));
    assert_eq!(r["limit_offset"], -300.0);
    assert!(r["top_level"].is_string());
    // With no level above, not bounded and 2500 mm tall; the default isn't written.
    let top = run(&mut s, "level", json!({"elevation": 9000, "name": "Roof"}))["id"].clone();
    let up = run(&mut s, "room", json!({"point": [2000, 1500], "level": top}))["id"].as_u64().unwrap();
    let r = room(&mut s, up);
    assert_eq!((r["unbounded_height"].as_f64(), r.get("limit_offset"), r.get("top_level")), (Some(2500.0), None, None));
}

#[test]
fn a_room_reaches_its_upper_limit_plus_its_limit_offset_from_its_base_offset() {
    let mut s = Session::new();
    let l2 = run(&mut s, "inspect", json!({}))["levels"].as_array().unwrap().iter().find(|l| l["name"] == "Level 2").cloned();
    let l2 = l2.map_or_else(|| run(&mut s, "level", json!({"elevation": 3000, "name": "Level 2"}))["id"].clone(), |l| l["id"].clone());
    let id = box_with_room(&mut s, json!({"top_level": "Level 2", "limit_offset": -200, "base_offset": 100}));
    let r = room(&mut s, id);
    let top = run(&mut s, "inspect", json!({}))["levels"].as_array().unwrap().iter().find(|l| l["id"] == l2).unwrap()["elevation"]
        .as_f64()
        .unwrap();
    assert!(r["top_level"].is_string(), "a level, by its GUID: {r}");
    assert_eq!(r["unbounded_height"].as_f64(), Some(top - 200.0 - 100.0));
    // Level 2 raised, the room grows with it.
    run(&mut s, "modify", json!({"id": l2, "elevation": top + 1000.0}));
    assert_eq!(room(&mut s, id)["unbounded_height"].as_f64(), Some(top + 1000.0 - 300.0));
    // Back to its own level: the limit offset alone.
    run(&mut s, "modify", json!({"id": id, "top_level": null, "limit_offset": 2700}));
    let r = room(&mut s, id);
    assert_eq!((r["unbounded_height"].as_f64(), r["volume_m3"].as_f64()), (Some(2600.0), Some(31.2)));
}

#[test]
fn a_room_whose_top_is_not_over_its_bottom_is_refused() {
    let mut s = Session::new();
    let id = box_with_room(&mut s, json!({}));
    let e = s.execute("modify", &json!({"id": id, "limit_offset": -3000})).unwrap_err();
    assert!(e.contains("must be above its bottom"), "{e}");
    let e = s.execute("modify", &json!({"id": id, "base_offset": 3000})).unwrap_err();
    assert!(e.contains("must be above its bottom"), "{e}");
    let e = s.execute("room", &json!({"point": [1000, 1000], "limit_offset": 0})).unwrap_err();
    assert!(e.contains("must be above its bottom"), "{e}");
    assert!(s.execute("modify", &json!({"id": id, "limit_offset": f64::MAX})).is_err());
    assert_eq!(room(&mut s, id)["unbounded_height"].as_f64(), Some(2700.0));
}

/// The dark pixels of level 1's plan in a box 14 px wide round the room's point, at 1 px to 10 mm.
fn ink_at_point(s: &Session) -> usize {
    use crate::render::{View, draw, png};
    let view = View { scale: 0.1, cx: 2000.0, cy: 1500.0, width: 600, height: 500 };
    let img = tiny_skia::Pixmap::decode_png(&png(&draw(&s.model, 100000, view)).unwrap()).unwrap();
    let (x, y) = view.px(crate::geom::P2(2000.0, 1500.0));
    let (x, y) = (x as i64, y as i64);
    (x - 7..=x + 7)
        .flat_map(|i| (y - 7..=y + 7).map(move |j| (i, j)))
        .filter_map(|(i, j)| img.pixel(u32::try_from(i).ok()?, u32::try_from(j).ok()?))
        .filter(|c| c.red() < 128)
        .count()
}

#[test]
fn a_plan_draws_a_rooms_reference_point_as_a_cross_when_its_graphics_show_it() {
    let mut s = Session::new();
    box_with_room(&mut s, json!({"name": "R"}));
    let without = ink_at_point(&s);
    let r = run(&mut s, "graphics", json!({"view": 100000, "show": ["roomreference"]}));
    assert_eq!(r["categories"]["roomreference"]["shown"], true);
    let with = ink_at_point(&s);
    assert!(with > without + 8, "the cross adds ink round the point: {without} then {with}");
    // A template that controls the categories controls it too.
    run(&mut s, "template", json!({"name": "T", "from": 100000}));
    run(&mut s, "graphics", json!({"view": 100000, "hide": ["roomreference"]}));
    assert_eq!(ink_at_point(&s), without);
    let e = s.execute("graphics", &json!({"view": 100000, "template": "T", "hide": ["roomreference"]})).unwrap_err();
    assert!(e.contains("follow view template"), "{e}");
    run(&mut s, "graphics", json!({"view": 100000, "template": "T"}));
    assert_eq!(ink_at_point(&s), with);
}
