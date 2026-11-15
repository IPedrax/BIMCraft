//! The topology extractor, driven through commands as a front end drives it.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::topology::extract;

/// An 8 m x 4 m box with a partition at x = 4000, rooms "A" (west) and "B" (east), then `extra`.
fn house(extra: &str) -> Session {
    let mut s = Session::new();
    let script = format!(
        "family {{\"name\": \"Sliding Door\", \"types\": {{\"2750 x 2100\": {{\"Width\": 2750}}, \"900 x 2100\": {{\"Width\": 900}}}}}}\n\
         wall {{\"points\": [[0, 0], [0, 4000], [8000, 4000], [8000, 0]], \"closed\": true, \"type\": \"Exterior - 300mm\"}}\n\
         wall {{\"points\": [[4000, 0], [4000, 4000]], \"type\": \"Interior - 100mm\"}}\n\
         room {{\"point\": [2000, 2000], \"name\": \"West\", \"number\": \"A\"}}\n\
         room {{\"point\": [6000, 2000], \"name\": \"East\", \"number\": \"B\"}}\n{extra}\n"
    );
    s.script(&script).unwrap();
    s
}

fn edges<'a>(g: &'a Value, kind: &str) -> Vec<&'a Value> {
    g["edges"].as_array().unwrap().iter().filter(|e| e["kind"] == kind).collect()
}

fn codes(g: &Value) -> Vec<String> {
    g["diagnostics"].as_array().unwrap().iter().map(|d| d["code"].as_str().unwrap().to_string()).collect()
}

#[test]
fn door_between_two_rooms_is_a_door_edge_with_family_operation_width() {
    let s = house(
        "door {\"at\": [4000, 3500], \"family\": \"Single Door\", \"type\": \"900 x 2100\"}\n\
         door {\"at\": [4000, 1500], \"family\": \"Sliding Door\", \"type\": \"2750 x 2100\"}",
    );
    let g = extract(&s.model);
    assert_eq!(g["nodes"].as_array().unwrap().len(), 2);
    let doors = edges(&g, "door");
    assert_eq!(doors.len(), 2, "{g}");
    let swing = doors.iter().find(|e| e["family"] == "Single Door").unwrap();
    assert_eq!((swing["from"].as_str(), swing["to"].as_str(), swing["operation"].as_str()), (Some("A"), Some("B"), Some("swing")));
    let sliding = doors.iter().find(|e| e["family"] == "Sliding Door").unwrap();
    assert_eq!(sliding["operation"], "sliding");
    assert_eq!(sliding["width_mm"], 2750.0);
    assert_eq!(sliding["type"], "2750 x 2100");
    assert!(g["diagnostics"].as_array().unwrap().is_empty(), "{g}");
}

#[test]
fn a_typed_door_names_its_type_as_family() {
    let s = house("door {\"at\": [4000, 2000], \"type\": \"Single 800 x 2100\"}");
    let g = extract(&s.model);
    let d = edges(&g, "door")[0];
    assert_eq!(
        (d["family"].as_str(), d["operation"].as_str(), d["width_mm"].as_f64()),
        (Some("Single 800 x 2100"), Some("swing"), Some(800.0))
    );
}

#[test]
fn exterior_door_marks_the_entry() {
    let s = house("door {\"at\": [0, 2000], \"family\": \"Single Door\", \"type\": \"900 x 2100\"}");
    let g = extract(&s.model);
    let d = edges(&g, "door");
    assert_eq!(d.len(), 1);
    assert_eq!((d[0]["from"].as_str(), d[0]["to"].as_str(), d[0]["exterior"].as_bool()), (Some("A"), Some("exterior"), Some(true)));
    assert_eq!(g["entry"]["room"], "A");
    assert_eq!(g["exterior_doors"].as_array().unwrap().len(), 1);
}

#[test]
fn a_double_swing_door_is_a_swing_door_for_the_entry() {
    // A wide double-swing main entry into A beats a narrower single swing service exit into B.
    let s = house(
        "type {\"category\": \"door\", \"name\": \"Main 1400\", \"width\": 1400, \"height\": 2100, \"operation\": \"double-swing\"}\n\
         door {\"at\": [0, 2000], \"type\": \"Main 1400\"}\n\
         door {\"at\": [8000, 2000], \"type\": \"Single 900 x 2100\"}",
    );
    let g = extract(&s.model);
    assert_eq!(g["entry"]["room"], "A", "{g}");
}

#[test]
fn a_door_named_an_entry_beats_a_wider_service_exit() {
    let s = house(
        "type {\"category\": \"door\", \"name\": \"Main Entry 1000\", \"width\": 1000, \"height\": 2100, \"operation\": \"swing\"}\n\
         type {\"category\": \"door\", \"name\": \"Service Exit 1200\", \"width\": 1200, \"height\": 2100, \"operation\": \"swing\"}\n\
         door {\"at\": [0, 2000], \"type\": \"Main Entry 1000\"}\n\
         door {\"at\": [8000, 2000], \"type\": \"Service Exit 1200\"}",
    );
    let g = extract(&s.model);
    assert_eq!(g["entry"]["room"], "A", "{g}");
}

#[test]
fn several_exterior_doors_pick_a_swing_door_then_the_widest_then_the_lowest_id() {
    // A wide sliding door into B, a narrow swing door into A: swing wins.
    let s = house(
        "door {\"at\": [8000, 2000], \"family\": \"Sliding Door\", \"type\": \"2750 x 2100\"}\n\
         door {\"at\": [0, 2000], \"family\": \"Single Door\", \"type\": \"900 x 2100\"}",
    );
    let g = extract(&s.model);
    assert_eq!(g["entry"]["room"], "A", "{g}");
    assert_eq!(g["exterior_doors"].as_array().unwrap().len(), 2);
    // Two swing doors: the wider one, whatever its id.
    let s = house(
        "door {\"at\": [0, 2000], \"type\": \"Single 800 x 2100\"}\n\
         door {\"at\": [8000, 1000], \"type\": \"Single 900 x 2100\"}\n\
         door {\"at\": [8000, 3000], \"type\": \"Single 900 x 2100\"}",
    );
    let g = extract(&s.model);
    let wide = g["exterior_doors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["width_mm"] == 900.0)
        .map(|d| d["door_id"].as_u64().unwrap())
        .min()
        .unwrap();
    assert_eq!(g["entry"]["door_id"], wide, "the widest, then the lowest id: {g}");
    assert_eq!(g["entry"]["room"], "B");
}

/// The box without its partition, split by room separation lines.
fn open_house(separators: &str) -> Session {
    let mut s = Session::new();
    s.script(&format!(
        "wall {{\"points\": [[0, 0], [0, 4000], [8000, 4000], [8000, 0]], \"closed\": true, \"type\": \"Exterior - 300mm\"}}\n\
         {separators}\n\
         room {{\"point\": [2000, 2000], \"name\": \"West\", \"number\": \"A\"}}\n\
         room {{\"point\": [6000, 2000], \"name\": \"East\", \"number\": \"B\"}}\n"
    ))
    .unwrap();
    s
}

#[test]
fn separator_gives_open_edge_and_short_touch_does_not() {
    let g = extract(&open_house("room_separator {\"points\": [[4000, 0], [4000, 4000]]}").model);
    let open = edges(&g, "open");
    assert_eq!(open.len(), 1, "{g}");
    assert_eq!((open[0]["from"].as_str(), open[0]["to"].as_str()), (Some("A"), Some("B")));
    assert!(edges(&g, "adjacent").is_empty() && edges(&g, "door").is_empty());
}

#[test]
fn a_short_separator_touch_makes_no_link_and_one_against_outside_gives_nothing() {
    // The separator stops 45 mm into the second room: A and B are one space with a stub, so only 45 mm is shared.
    let g = extract(&house("room_separator {\"points\": [[4000, 1000], [4000, 1045]]}").model);
    assert!(edges(&g, "open").is_empty(), "{g}");
    // A separator with a room on one side only (a porch's open side).
    let g = extract(
        &open_house("room_separator {\"points\": [[4000, 0], [4000, 4000]]}\nroom_separator {\"points\": [[0, 0], [-3000, 0]]}").model,
    );
    assert_eq!(edges(&g, "open").len(), 1, "{g}");
}

#[test]
fn wall_without_door_is_adjacent_and_never_also_access() {
    let g = extract(&house("").model);
    let adj = edges(&g, "adjacent");
    assert_eq!(adj.len(), 1, "{g}");
    assert_eq!((adj[0]["from"].as_str(), adj[0]["to"].as_str()), (Some("A"), Some("B")));
    assert!(adj[0]["length_mm"].as_f64().unwrap() >= 3000.0);
    let g = extract(&house("door {\"at\": [4000, 2000], \"family\": \"Single Door\", \"type\": \"900 x 2100\"}").model);
    assert!(edges(&g, "adjacent").is_empty(), "{g}");
}

#[test]
fn removing_a_door_turns_its_edge_adjacent() {
    let mut s = house("door {\"at\": [4000, 2000], \"family\": \"Single Door\", \"type\": \"900 x 2100\"}");
    let id = s.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Door(_))).map(|(id, _)| *id).unwrap();
    s.execute("delete", &json!({"ids": [id]})).unwrap();
    let g = extract(&s.model);
    assert!(edges(&g, "door").is_empty() && edges(&g, "adjacent").len() == 1, "{g}");
}

#[test]
fn unenclosed_room_is_a_typed_diagnostic() {
    let mut s = house("");
    s.execute("room", &json!({"point": [-5000, -5000], "name": "Outside", "number": "X"})).unwrap();
    let dir = std::env::temp_dir().join(format!("bimcraft-topology-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.json");
    s.execute("topology", &json!({"path": path})).unwrap();
    let g: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(codes(&g), ["room_not_enclosed"], "{g}");
    assert_eq!(g["diagnostics"][0]["room"], "X");
    let ids: Vec<&str> = g["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["A", "B"]);
}

#[test]
fn a_door_with_no_room_beside_it_is_a_diagnostic() {
    let mut s = Session::new();
    s.script("wall {\"points\": [[0, 0], [4000, 0]], \"type\": \"Exterior - 300mm\"}\ndoor {\"at\": [2000, 0], \"family\": \"Single Door\", \"type\": \"900 x 2100\"}")
        .unwrap();
    let g = extract(&s.model);
    assert_eq!(codes(&g), ["door_without_room"], "{g}");
}

#[test]
fn stepped_shell_footprint_is_not_a_rectangle() {
    // An L: 8 x 6 m with a 4 x 3 m corner cut away, and a wall stub dangling below the bottom edge.
    let mut s = Session::new();
    s.script(
        "wall {\"points\": [[0, 0], [0, 6000], [4000, 6000], [4000, 3000], [8000, 3000], [8000, 0]], \"closed\": true, \"type\": \"Exterior - 300mm\"}\n\
         wall {\"points\": [[0, 0], [0, -500]], \"type\": \"Exterior - 300mm\"}\n\
         room {\"point\": [2000, 2000], \"name\": \"L\", \"number\": \"L\"}",
    )
    .unwrap();
    let g = extract(&s.model);
    let f = &g["footprint"];
    let outline = f["outline"].as_array().unwrap();
    assert_eq!(outline.len(), 6, "{f}");
    assert!((f["area_m2"].as_f64().unwrap() - 36.0).abs() < 0.01, "{f}");
    let ratio = f["area_over_bbox"].as_f64().unwrap();
    assert!((ratio - 0.75).abs() < 1e-3 && ratio < 0.97, "{f}");
    let ys: BTreeSet<i64> = outline.iter().map(|p| p[1].as_f64().unwrap() as i64).collect();
    assert!(ys.iter().all(|y| *y >= 0), "a dangling stub leaves no spike: {f}");
}

#[test]
fn same_answer_at_centre_and_finish() {
    let door = "door {\"at\": [4000, 1000], \"family\": \"Single Door\", \"type\": \"900 x 2100\"}\ndoor {\"at\": [0, 2000], \"type\": \"Single 900 x 2100\"}";
    let mut s = house(door);
    let strip = |g: &Value| {
        g["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| format!("{}|{}|{}|{}", e["from"], e["to"], e["kind"], e["door_id"]))
            .collect::<Vec<_>>()
    };
    let centre = strip(&extract(&s.model));
    s.execute("room_area", &json!({"at": "finish"})).unwrap();
    let g = extract(&s.model);
    assert_eq!(strip(&g), centre, "{g}");
    assert_eq!(g["nodes"].as_array().unwrap().len(), 2);
}

#[test]
fn the_same_model_gives_the_same_bytes() {
    let s = house("door {\"at\": [4000, 1000], \"family\": \"Single Door\", \"type\": \"900 x 2100\"}");
    assert_eq!(extract(&s.model).to_string(), extract(&s.model).to_string());
}

#[test]
fn topology_refuses_what_it_cannot_write() {
    let mut s = house("");
    for p in [json!({"path": ""}), json!({"path": 5}), json!({"other": 1}), json!({"path": "/nonexistent-dir-xyz/t.json"})] {
        assert!(s.execute("topology", &p).is_err(), "{p}");
    }
    assert!(s.execute("topology", &json!({})).unwrap()["nodes"].is_array());
}
