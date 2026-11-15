//! `building` in the shape graph's manifest: a hash of the layout, not of how the script was written.

use serde_json::{Value, json};

use super::{Request, build, building_id};

fn request(seed: u64) -> Request {
    Request {
        archetype: "house".into(),
        seed,
        rooms: 9,
        tolerance: 1,
        shape: None,
        gridlines: false,
        interior: super::Interior::Aligned,
        bumps: false,
    }
}

fn manifest(seed: u64) -> Value {
    let built = build(&request(seed), false).unwrap();
    let text = built.files.iter().find(|(n, _)| *n == "manifest.json").map(|(_, t)| t.clone()).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn building_is_a_hash_of_the_layout() {
    let m = manifest(3);
    let b = m["building"].as_str().unwrap();
    assert!(b.starts_with("house-") && b.len() == 18, "{b}");
    assert!(b[6..].bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)), "{b}");
    assert_eq!(manifest(3)["building"], m["building"]);
    assert_ne!(manifest(4)["building"], m["building"]);
    assert_eq!(m["generator_version"], "shape-graph/1");
}

#[test]
fn building_ignores_the_title_door_marks_and_order_but_not_a_moved_wall() {
    let lines = vec![
        ("new", json!({"name": "a", "seed": 1})),
        ("wall", json!({"points": [[0, 0], [1000, 0]], "type": "x"})),
        ("door", json!({"at": [500, 0], "mark": "D1"})),
        ("door", json!({"at": [700, 0], "mark": "D2"})),
        ("cad_profile", json!({"name": "delivery"})),
    ];
    let base = building_id("house", &lines);
    let mut shuffled = lines.clone();
    shuffled[0] = ("new", json!({"name": "b", "seed": 2}));
    shuffled[2] = ("door", json!({"at": [700, 0], "mark": "D1"}));
    shuffled[3] = ("door", json!({"at": [500, 0], "mark": "D2"}));
    assert_eq!(building_id("house", &shuffled), base);
    let mut moved = lines.clone();
    moved[1] = ("wall", json!({"points": [[0, 0], [1010, 0]], "type": "x"}));
    assert_ne!(building_id("house", &moved), base);
}
