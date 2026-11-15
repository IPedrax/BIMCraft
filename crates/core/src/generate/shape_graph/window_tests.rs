//! Windows and room contents (the spec's Slice 7), through the engine: the family file's fields, what they put in the script, and
//! that they change nothing else.

use serde_json::{Value, json};

use super::family::Family;
use super::register_tests::house_family;
use super::{Interior, Request, build, build_family};
use crate::generate::Built;

fn request(seed: u64, rooms: i64) -> Request {
    Request {
        archetype: "house".into(),
        seed,
        rooms,
        tolerance: 0,
        shape: None,
        gridlines: false,
        interior: Interior::Aligned,
        bumps: false,
    }
}

fn file<'a>(b: &'a Built, name: &str) -> &'a str {
    b.files.iter().find(|(n, _)| *n == name).map(|(_, t)| t.as_str()).unwrap_or_default()
}

/// The script's lines of one command.
fn lines<'a>(b: &'a Built, id: &str) -> Vec<&'a str> {
    file(b, "script.bimcraft.txt").lines().filter(|l| l.split(' ').next() == Some(id)).collect()
}

fn param(line: &str) -> Value {
    serde_json::from_str(line.split_once(' ').map_or("null", |(_, p)| p)).unwrap_or(Value::Null)
}

/// The key of the house room type whose contents hold `family`.
fn holder(family: &str) -> String {
    let v: Value = serde_json::from_str(house_family()).expect("the house family is JSON");
    let rooms = v["rooms"].as_object().expect("rooms");
    rooms
        .iter()
        .find(|(_, r)| r["contents"].as_array().is_some_and(|c| c.iter().any(|e| e["family"] == family)))
        .map(|(k, _)| k.clone())
        .expect("a room type")
}

/// The house family without `window` and `contents`, as the copy a checker saves with sorted keys.
fn stripped() -> String {
    let mut v: Value = serde_json::from_str(house_family()).expect("the house family is JSON");
    for r in v["rooms"].as_object_mut().into_iter().flat_map(|m| m.values_mut()) {
        if let Some(o) = r.as_object_mut() {
            o.remove("window");
            o.remove("contents");
        }
    }
    v.to_string()
}

#[test]
fn a_house_has_windows_and_contents_in_its_script() {
    let b = build(&request(2, 20), false).expect("a plan");
    let (windows, components) = (lines(&b, "window"), lines(&b, "component"));
    assert!(windows.len() >= 10, "{} windows", windows.len());
    assert!(components.len() >= 10, "{} components", components.len());
    let marks: Vec<String> = windows.iter().map(|l| param(l)["mark"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(marks[0], "W1");
    let building: Value = serde_json::from_str(file(&b, "building-ir.json")).expect("JSON");
    let of = |k: &str| building["openings"].as_array().into_iter().flatten().filter(|o| o["kind"] == k).count();
    assert_eq!(of("window"), windows.len());
    assert_eq!(building["components"].as_array().map(Vec::len), Some(components.len()));
    let report: Value = serde_json::from_str(file(&b, "report.json")).expect("JSON");
    assert!(report["checks"].as_array().into_iter().flatten().any(|c| c["id"] == "SG-004" && c["status"] == "PASS"), "{report}");
}

#[test]
fn windows_and_contents_change_nothing_else() {
    let fam = Family::parse(&stripped()).expect("a family without windows or contents");
    for (seed, rooms) in [(2, 20), (1, 22), (6, 14)] {
        let with = build(&request(seed, rooms), false).expect("a plan");
        let without = build_family(&fam, &request(seed, rooms), false).expect("a plan");
        assert!(lines(&without, "window").is_empty() && lines(&without, "component").is_empty());
        assert!(!lines(&with, "window").is_empty() && !lines(&with, "component").is_empty());
        assert_eq!(with.topology_hash, without.topology_hash, "{seed}/{rooms}");
        for id in ["wall", "door", "room_separator", "room"] {
            assert_eq!(lines(&with, id), lines(&without, id), "{seed}/{rooms}: {id}");
        }
        for name in ["program-graph.json", "topology-graph.json"] {
            assert_eq!(file(&with, name), file(&without, name), "{seed}/{rooms}: {name}");
        }
    }
}

/// A file saved with its keys in sorted order (what `serde_json::Value` does without `preserve_order`) is the same family: the
/// room types are in the file's order, and the file's order is the alphabetical one.
#[test]
fn the_house_file_is_the_same_family_saved_with_sorted_keys() {
    let v: Value = serde_json::from_str(house_family()).expect("JSON");
    let sorted = Family::parse(&serde_json::to_string(&v).expect("JSON")).expect("parses");
    for (seed, rooms) in [(2, 20), (1, 22), (9, 12)] {
        let a = build(&request(seed, rooms), false).expect("a plan");
        let b = build_family(&sorted, &request(seed, rooms), false).expect("a plan");
        assert_eq!(a.files, b.files, "{seed}/{rooms}");
    }
}

#[test]
fn most_bedrooms_hold_a_bed_and_most_baths_a_wc() {
    let (mut bedrooms, mut beds, mut baths, mut wcs) = (0, 0, 0, 0);
    for seed in 1..=10 {
        let b = build(&request(seed, 16), false).expect("a plan");
        let graph: Value = serde_json::from_str(file(&b, "topology-graph.json")).expect("JSON");
        let n = |ty: &str| graph["nodes"].as_array().into_iter().flatten().filter(|x| x["room_type"] == ty).count();
        let held = |f: &str| lines(&b, "component").iter().filter(|l| param(l)["family"] == f).count();
        (bedrooms, beds, baths, wcs) = (bedrooms + n(&holder("Bed")), beds + held("Bed"), baths + n(&holder("WC")), wcs + held("WC"));
    }
    assert!(bedrooms >= 10 && baths >= 10, "{bedrooms} bedrooms, {baths} baths");
    assert!(beds * 10 >= bedrooms * 9, "{beds} beds in {bedrooms} bedrooms");
    assert!(wcs * 10 >= baths * 9, "{wcs} WCs in {baths} baths");
}

#[test]
fn the_same_brief_gives_the_same_files() {
    let a = build(&request(2, 20), false).expect("a plan");
    let b = build(&request(2, 20), false).expect("a plan");
    assert_eq!(a.files, b.files);
}

#[test]
fn the_script_of_a_house_runs_and_holds_each_window_and_component() {
    use crate::model::Element;
    let b = build(&request(2, 20), false).expect("a plan");
    let mut s = crate::engine::Session::new();
    s.script(file(&b, "script.bimcraft.txt")).expect("the script runs");
    let windows = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Window(_))).count();
    let components = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Component(_))).count();
    assert_eq!((windows, components), (lines(&b, "window").len(), lines(&b, "component").len()));
}

#[test]
fn a_family_file_with_windows_and_contents_is_checked() {
    let room = holder("Bed");
    let with = |change: &dyn Fn(&mut Value)| -> Result<Family, String> {
        let mut v: Value = serde_json::from_str(house_family()).expect("JSON");
        change(&mut v["rooms"][&room]);
        Family::parse(&v.to_string())
    };
    // The spec's example is accepted.
    assert!(
        with(&|r| {
            r["window"] = json!(true);
            r["contents"] = json!([
                {"family": "Bed", "types": ["1600 x 2000", "900 x 2000"], "count": [1, 1], "against": "wall"},
                {"family": "Table", "count": [0, 1], "against": "centre"}
            ]);
        })
        .is_ok()
    );
    let entry = |e: Value| move |r: &mut Value| r["contents"] = json!([e.clone()]);
    for (what, e, says) in [
        ("an unknown family", json!({"family": "Piano", "count": [1, 1], "against": "wall"}), "Piano"),
        ("a place that is not one", json!({"family": "Bed", "count": [1, 1], "against": "ceiling"}), "ceiling"),
        ("an unknown key", json!({"family": "Bed", "count": [1, 1], "against": "wall", "colour": "red"}), "colour"),
        ("a count the wrong way round", json!({"family": "Bed", "count": [2, 1], "against": "wall"}), "count"),
        ("a negative count", json!({"family": "Bed", "count": [-1, 1], "against": "wall"}), "count"),
        ("a count past the cap", json!({"family": "Bed", "count": [0, 99], "against": "wall"}), "count"),
        ("no types", json!({"family": "Bed", "types": [], "count": [1, 1], "against": "wall"}), "types"),
        ("a type the family lacks", json!({"family": "Bed", "types": ["2400 x 2400"], "count": [1, 1], "against": "wall"}), "2400"),
        ("a door where a piece belongs", json!({"family": "Single Door", "count": [1, 1], "against": "wall"}), "Single Door"),
    ] {
        let err = with(&entry(e)).expect_err(what);
        assert!(err.contains(says) || err.contains(&room), "{what}: {err}");
    }
    assert!(with(&|r| r["window"] = json!("yes")).is_err(), "window is true or false");
    let err = with(&|r| r["contents"] = json!([{"family": "Piano", "count": [1, 1], "against": "wall"}])).expect_err("refused");
    assert!(err.contains(&room) && err.contains("entry 0"), "names the room type and the entry: {err}");
}

#[test]
fn a_refused_family_file_writes_nothing() {
    let dir = std::env::temp_dir().join("shape-graph-contents-refused");
    let _ = std::fs::remove_dir_all(&dir);
    let mut v: Value = serde_json::from_str(house_family()).expect("JSON");
    v["rooms"][holder("Bed")]["contents"] = json!([{"family": "Piano", "count": [1, 1], "against": "wall"}]);
    let fam = std::env::temp_dir().join("shape-graph-contents-refused.json");
    std::fs::write(&fam, v.to_string()).expect("a scratch file");
    let p = json!({"path": dir.display().to_string(), "family_file": fam.display().to_string(), "seed": 1, "rooms": 12});
    let err = crate::generate::run(&p).expect_err("refused");
    assert_eq!(err.code, "invalid_family");
    assert!(!dir.exists());
    let _ = std::fs::remove_file(&fam);
}
