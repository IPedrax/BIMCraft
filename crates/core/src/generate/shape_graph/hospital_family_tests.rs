//! The hospital family (the spec's Slice 9): it names its pack, whose rules then judge every graph, and its rooms have windows and
//! contents that change nothing else. No kind is written here: they are read from the family file and from the pack's PR-001.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::family::{self, Family};
use super::{Request, build, build_family};
use crate::generate::Built;
use crate::graph_rules::Pack;

const NAME: &str = "hospital";

fn text() -> &'static str {
    family::builtin(NAME).expect("a built-in family")
}

fn json_file() -> Value {
    serde_json::from_str(text()).expect("the family is JSON")
}

fn request(seed: u64, rooms: i64) -> Request {
    Request::new(NAME.into(), seed, rooms, 0, None, false)
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

/// The room types whose contents hold `family`, with that entry.
fn holders(family: &str) -> Vec<(String, Value, Value)> {
    let v = json_file();
    let mut out = Vec::new();
    for (key, room) in v["rooms"].as_object().expect("rooms") {
        if let Some(e) = room["contents"].as_array().into_iter().flatten().find(|e| e["family"] == family) {
            out.push((key.clone(), room.clone(), e.clone()));
        }
    }
    out
}

/// The kinds the pack's minimum program names.
fn program_kinds() -> BTreeSet<String> {
    let pack = Pack::embedded(NAME).expect("the pack");
    let rule = pack.rules.iter().find(|r| r.id == "PR-001").expect("the minimum program");
    let groups = rule.check.get("groups").and_then(Value::as_array).expect("groups");
    groups.iter().flat_map(|g| g["kinds"].as_array().into_iter().flatten()).filter_map(Value::as_str).map(String::from).collect()
}

#[test]
fn the_family_names_its_pack_and_uses_the_kinds_of_its_minimum_program() {
    assert_eq!(json_file()["pack_id"], NAME);
    let fam = Family::parse(text()).expect("the family is read under its pack");
    assert_eq!(fam.pack_id.as_deref(), Some(NAME));
    let kinds = program_kinds();
    assert!(kinds.len() > 10, "{kinds:?}");
    for t in &fam.types {
        assert!(kinds.contains(&t.kind), "the room type {} has a kind PR-001 does not name", t.key);
    }
}

#[test]
fn the_review_says_what_is_true_of_the_file() {
    let v = json_file();
    assert_eq!(v["review"]["status"], "reviewed");
    let note = v["review"]["note"].as_str().expect("a note");
    assert!(note.contains("pack_id") && note.contains("graph-rules/hospital.json"), "it names its pack: {note}");
    assert!(note.contains("window") && note.contains("contents") && note.contains("not been reviewed by a person"), "{note}");
    assert!(!note.contains("remains null"), "{note}");
}

#[test]
fn the_rooms_have_the_windows_and_contents_of_the_slice() {
    let one = |family: &str| {
        let mut found = holders(family);
        assert_eq!(found.len(), 1, "one room type holds a {family}");
        found.remove(0)
    };
    let (_, bed_type, bed) = one("Bed");
    assert_eq!(bed["count"], json!([1, 1]));
    assert_eq!(bed_type["contents"].as_array().map(Vec::len), Some(1));
    let (wc_room, wc_type, wc) = one("WC");
    let (basin_room, _, basin) = one("Washbasin");
    assert_eq!(wc_room, basin_room);
    assert_eq!((&wc["count"], &basin["count"]), (&json!([1, 1]), &json!([1, 1])));
    assert_eq!(wc_type["contents"].as_array().map(Vec::len), Some(2));
    let (_, sofa_type, sofa) = one("Sofa");
    assert_eq!(sofa["count"], json!([1, 3]));
    assert_eq!(sofa_type["contents"].as_array().map(Vec::len), Some(1));
    let tables = holders("Table");
    assert_eq!(tables.len(), 2);
    for (key, room, table) in &tables {
        assert_eq!(table["count"], json!([0, 1]), "{key}");
        assert_eq!(room["contents"].as_array().map(Vec::len), Some(1), "{key}");
        assert_eq!(room["window"], true, "{key}");
    }
    assert!(holders("Kitchen Sink").is_empty());
    let v = json_file();
    let rooms = v["rooms"].as_object().expect("rooms");
    assert_eq!(rooms.values().filter(|r| r.get("contents").is_some()).count(), 5);
    assert_eq!(rooms.values().filter(|r| r["window"] == true).count(), 6);
    assert!(rooms.values().all(|r| r.get("window").is_none_or(|w| w == true)), "a room type without windows leaves the key out");
    assert_eq!((&bed_type["window"], &sofa_type["window"]), (&json!(true), &json!(true)));
    assert!(wc_type.get("window").is_none(), "{wc_room} has no window");
}

#[test]
fn every_rule_of_the_pack_is_a_passing_check_in_the_report() {
    let b = build(&request(1, 30), false).expect("a plan");
    let report: Value = serde_json::from_str(file(&b, "report.json")).expect("JSON");
    let checks = report["checks"].as_array().expect("checks");
    let pack = Pack::embedded(NAME).expect("the pack");
    assert_eq!(pack.rules.len(), 7);
    for rule in &pack.rules {
        let found: Vec<&Value> = checks.iter().filter(|c| c["id"] == rule.id.as_str()).collect();
        assert_eq!(found.len(), 1, "one check for {}", rule.id);
        assert_eq!(found[0]["status"], "PASS", "{}: {}", rule.id, found[0]["detail"]);
    }
    let manifest: Value = serde_json::from_str(file(&b, "manifest.json")).expect("JSON");
    assert_eq!((&manifest["family"], &manifest["family_review"]), (&json!(NAME), &json!("reviewed")));
}

#[test]
fn most_patient_rooms_hold_a_bed_and_most_restrooms_a_wc() {
    let (bed_room, wc_room) = (holders("Bed").remove(0).0, holders("WC").remove(0).0);
    let (mut bed_rooms, mut beds, mut wc_rooms, mut wcs, mut windows) = (0, 0, 0, 0, 0);
    for seed in 1..=4 {
        let b = build(&request(seed, 30), false).expect("a plan");
        let graph: Value = serde_json::from_str(file(&b, "topology-graph.json")).expect("JSON");
        let n = |ty: &str| graph["nodes"].as_array().into_iter().flatten().filter(|x| x["room_type"] == ty).count();
        let held = |f: &str| lines(&b, "component").iter().filter(|l| param(l)["family"] == f).count();
        (bed_rooms, beds, wc_rooms, wcs) = (bed_rooms + n(&bed_room), beds + held("Bed"), wc_rooms + n(&wc_room), wcs + held("WC"));
        windows += lines(&b, "window").len();
    }
    assert!(bed_rooms >= 4 && wc_rooms >= 4, "{bed_rooms} rooms for a bed, {wc_rooms} for a WC");
    assert!(beds * 10 >= bed_rooms * 9 && beds <= bed_rooms, "{beds} beds in {bed_rooms} rooms");
    assert!(wcs * 10 >= wc_rooms * 9 && wcs <= wc_rooms, "{wcs} WCs in {wc_rooms} rooms");
    assert!(windows > 0);
}

#[test]
fn windows_and_contents_change_nothing_else() {
    let mut v = json_file();
    for r in v["rooms"].as_object_mut().into_iter().flat_map(|m| m.values_mut()) {
        if let Some(o) = r.as_object_mut() {
            o.remove("window");
            o.remove("contents");
        }
    }
    // The rooms keep the file's order only in the file's own text, so the two keys are cut out of that, and `v` shows the cut
    // took nothing else.
    let mut stripped = String::new();
    let mut skip = false;
    for line in text().lines() {
        let key = line.trim_start();
        if key.starts_with("\"window\"") {
            continue;
        }
        if key.starts_with("\"contents\"") {
            skip = true;
        }
        if !skip {
            stripped.push_str(line);
            stripped.push('\n');
        } else if line.trim_end().ends_with("],") && line.starts_with("      ]") {
            skip = false;
        }
    }
    let without: Value = serde_json::from_str(&stripped).expect("the cut file is JSON");
    assert_eq!(without["rooms"], v["rooms"], "only `window` and `contents` were cut");
    let fam = Family::parse(&stripped).expect("a family without windows or contents");
    let with = build(&request(2, 30), false).expect("a plan");
    let without = build_family(&fam, &request(2, 30), false).expect("a plan");
    assert!(lines(&without, "window").is_empty() && lines(&without, "component").is_empty());
    assert!(!lines(&with, "window").is_empty() && !lines(&with, "component").is_empty());
    assert_eq!(with.topology_hash, without.topology_hash);
    for id in ["wall", "door", "room_separator", "room"] {
        assert_eq!(lines(&with, id), lines(&without, id), "{id}");
    }
    for name in ["program-graph.json", "topology-graph.json"] {
        assert_eq!(file(&with, name), file(&without, name), "{name}");
    }
}
