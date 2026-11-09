//! The room a component stands in: what `inspect` says of it, what a schedule shows as its Room, and what `load_points` counts
//! from the model.

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap_or_default()
}

/// Two rooms side by side, 4000 x 3000 each, in 200 mm walls on their centre lines: `Sala` (101) west, `Cozinha` (102) east.
fn two_rooms(s: &mut Session) {
    run(s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 3000], [0, 3000]], "closed": true, "type": "Generic - 200mm"}));
    run(s, "wall", json!({"points": [[4000, 0], [4000, 3000]], "type": "Generic - 200mm"}));
    run(s, "room", json!({"point": [2000, 1500], "name": "Sala", "number": "101"}));
    run(s, "room", json!({"point": [6000, 1500], "name": "Cozinha", "number": "102"}));
}

/// Element `id` as `elements` gives it.
fn inspect(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({"category": "component"}));
    all["elements"].as_array().and_then(|a| a.iter().find(|e| e["id"] == id).cloned()).unwrap_or_default()
}

#[test]
fn a_component_knows_the_room_it_stands_in() {
    let mut s = Session::new();
    two_rooms(&mut s);
    let sala = run(&mut s, "elements", json!({"category": "room"}))["elements"][0]["guid"].clone();
    let west = id(&run(&mut s, "component", json!({"family": "Outlet", "at": [1000, 1000]})));
    let east = id(&run(&mut s, "component", json!({"family": "Outlet", "at": [7000, 1000]})));
    let outside = id(&run(&mut s, "component", json!({"family": "Outlet", "at": [9000, 1000]})));
    let w = inspect(&mut s, west);
    assert_eq!((&w["room"], &w["room_name"], &w["room_number"]), (&sala, &json!("Sala"), &json!("101")), "{w}");
    assert_eq!(inspect(&mut s, east)["room_name"], "Cozinha");
    let o = inspect(&mut s, outside);
    assert!(o.get("room").is_none() && o.get("room_name").is_none(), "outside every room: {o}");
}

#[test]
fn an_outlet_on_a_walls_face_is_in_the_room_on_that_side() {
    let mut s = Session::new();
    two_rooms(&mut s);
    // The bottom wall runs from y -100 to 100 and the middle one from x 3900 to 4100: on their faces, inside each room, each
    // facing its room as one placed on the face does. An outlet's room calculation point stands 150 mm in front of it (its
    // front is toward -y), so the one on the bottom wall's north face is turned to face north.
    let bottom = id(&run(&mut s, "component", json!({"family": "Outlet", "at": [2000, 100], "angle": 180})));
    let middle = id(&run(&mut s, "component", json!({"family": "Outlet", "at": [4100, 1500], "angle": 90})));
    assert_eq!(inspect(&mut s, bottom)["room_name"], "Sala");
    assert_eq!(inspect(&mut s, middle)["room_name"], "Cozinha");
    // The same, with the rooms measured at the finish faces.
    run(&mut s, "room_area", json!({"at": "finish"}));
    assert_eq!(inspect(&mut s, bottom)["room_name"], "Sala");
    assert_eq!(inspect(&mut s, middle)["room_name"], "Cozinha");
}

#[test]
fn a_component_on_another_level_is_in_none_of_these_rooms() {
    let mut s = Session::new();
    two_rooms(&mut s);
    let up = id(&run(&mut s, "component", json!({"family": "Outlet", "at": [1000, 1000], "level": "Level 2"})));
    assert!(inspect(&mut s, up).get("room").is_none());
}

#[test]
fn a_schedule_of_components_shows_their_room() {
    let mut s = Session::new();
    two_rooms(&mut s);
    run(&mut s, "component", json!({"family": "Outlet", "at": [1000, 1000]}));
    run(&mut s, "component", json!({"family": "Outlet", "at": [9000, 1000]}));
    let t = run(&mut s, "schedule", json!({"name": "Outlets", "category": "components", "fields": ["Family", "Room Number", "Room Name"]}));
    let rows = t["rows"].as_array().cloned().unwrap_or_default();
    assert!(rows.iter().any(|r| r == &json!(["Outlet", "101", "Sala"])), "{t}");
    let none = |v: &Value| v.is_null() || v == "";
    assert!(rows.iter().any(|r| r[0] == "Outlet" && none(&r[1]) && none(&r[2])), "the one outside a room has none: {t}");
}

#[test]
fn load_points_counts_the_outlets_and_lights_drawn_in_each_room() {
    let mut s = Session::new();
    two_rooms(&mut s);
    for at in [[1000, 1000], [3000, 1000]] {
        run(&mut s, "component", json!({"family": "Outlet", "at": at}));
    }
    run(&mut s, "component", json!({"family": "Ceiling Luminaire", "at": [2000, 1500]}));
    // Outside, and on another level: counted in no room.
    run(&mut s, "component", json!({"family": "Outlet", "at": [9000, 1000]}));
    run(&mut s, "component", json!({"family": "Outlet", "at": [6000, 1000], "level": "Level 2"}));
    let r = run(&mut s, "load_points", json!({}));
    let by = |name: &str| r["rooms"].as_array().and_then(|a| a.iter().find(|x| x["name"] == name).cloned()).unwrap_or_default();
    let (sala, cozinha) = (by("Sala"), by("Cozinha"));
    assert_eq!((&sala["tug_points"], &sala["lighting_points"]), (&json!(2), &json!(1)), "{sala}");
    assert_eq!(sala["ok"], false, "a room of about 13 m of perimeter needs 3: {sala}");
    assert_eq!((&cozinha["tug_points"], &cozinha["lighting_points"], &cozinha["ok"]), (&json!(0), &json!(0), &json!(false)));
    assert_eq!(r["counted"], json!({"outlets": true, "lighting": true}));
    assert_eq!(r["ok"], false);
}

#[test]
fn load_points_takes_the_least_where_the_model_draws_none() {
    let mut s = Session::new();
    two_rooms(&mut s);
    let r = run(&mut s, "load_points", json!({}));
    assert_eq!(r["counted"], json!({"outlets": false, "lighting": false}));
    assert_eq!(r["rooms"][0]["tug_points"], r["rooms"][0]["tug_minimum"]);
    assert_eq!(r["ok"], true);
}

#[test]
fn a_demolished_outlet_is_not_counted() {
    let mut s = Session::new();
    two_rooms(&mut s);
    let gone = id(&run(&mut s, "component", json!({"family": "Outlet", "at": [1000, 1000]})));
    run(&mut s, "component", json!({"family": "Outlet", "at": [3000, 1000]}));
    let phases = run(&mut s, "phase", json!({}))["phases"].clone();
    let last = phases.as_array().and_then(|p| p.last()).cloned().unwrap_or_default();
    run(&mut s, "modify", json!({"id": gone, "params": {"Phase Demolished": last}}));
    let r = run(&mut s, "load_points", json!({}));
    assert_eq!(r["rooms"][0]["tug_points"], 1, "{r}");
}
