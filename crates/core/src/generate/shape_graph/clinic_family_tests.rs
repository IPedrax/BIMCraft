//! The built-in clinic family is data under its pack: it names the pack, every graph gets a passing check for each of the pack's
//! rules, and its windows and contents are the ones the spec lists and change nothing else. No kind is written here: the kinds
//! come from the pack, and which room type holds what from a file beside the toy families.

use std::collections::BTreeSet;

use serde_json::Value;

use super::family::{self, Family};
use super::{Request, build, build_family};
use crate::generate::Built;
use crate::graph_rules::Pack;

const NAME: &str = "clinic";

/// What the spec says each room type holds, by room type key.
const EXPECTED: &str = include_str!("../testdata/shape-graph/clinic-windows-contents.json");

fn text() -> &'static str {
    family::builtin(NAME).expect("built in")
}

fn request(seed: u64, rooms: i64) -> Request {
    Request::new(NAME.into(), seed, rooms, 0, None, false)
}

fn file<'a>(b: &'a Built, name: &str) -> &'a str {
    b.files.iter().find(|(n, _)| *n == name).map(|(_, t)| t.as_str()).unwrap_or_default()
}

fn json(b: &Built, name: &str) -> Value {
    serde_json::from_str(file(b, name)).expect("JSON")
}

/// The script's lines of one command.
fn lines<'a>(b: &'a Built, id: &str) -> Vec<&'a str> {
    file(b, "script.bimcraft.txt").lines().filter(|l| l.split(' ').next() == Some(id)).collect()
}

fn param(line: &str) -> Value {
    serde_json::from_str(line.split_once(' ').map_or("null", |(_, p)| p)).unwrap_or(Value::Null)
}

/// The family without `window` and `contents`, its rooms left in the file's order. Each of the two is one line of the file, before
/// the room type's `source`.
fn stripped() -> Family {
    let keep = |line: &&str| {
        let t = line.trim_start();
        !t.starts_with("\"window\":") && !t.starts_with("\"contents\":")
    };
    let out: Vec<&str> = text().lines().filter(keep).collect();
    assert!(out.len() < text().lines().count(), "the file has windows or contents to take out");
    let fam = Family::parse(&out.join("\n")).expect("the copy parses");
    assert!(fam.types.iter().all(|t| !t.window && t.contents.is_empty()));
    fam
}

/// The key of the room type whose contents hold `family` at least once in every room.
fn always_holds(fam: &Family, family: &str) -> String {
    let t = fam.types.iter().find(|t| t.contents.iter().any(|c| c.family == family && c.count.0 >= 1));
    t.map(|t| t.key.clone()).unwrap_or_else(|| panic!("no room type always holds a {family}"))
}

#[test]
fn the_clinic_names_its_pack_and_uses_only_the_kinds_its_program_rule_names() {
    let fam = Family::parse(text()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(fam.pack_id.as_deref(), Some(NAME));
    assert_eq!(fam.review.status, "reviewed");
    let pack = Pack::embedded(NAME).expect("its pack");
    let named: BTreeSet<&str> = pack
        .rules
        .iter()
        .filter_map(|r| r.check.get("groups")?.as_array())
        .flatten()
        .flat_map(|g| g["kinds"].as_array().into_iter().flatten().filter_map(Value::as_str))
        .collect();
    assert!(named.len() >= 7, "{named:?}");
    for t in &fam.types {
        assert!(named.contains(t.kind.as_str()), "the room type `{}` has a kind the pack's program rule does not name", t.key);
    }
}

#[test]
fn its_windows_and_contents_are_the_ones_the_spec_lists() {
    let fam = Family::parse(text()).unwrap_or_else(|e| panic!("{e}"));
    let want: Value = serde_json::from_str(EXPECTED).expect("JSON");
    let windows: BTreeSet<&str> = want["window"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    assert_eq!(windows.len(), 4);
    let contents = want["contents"].as_object().expect("contents");
    for key in windows.iter().copied().chain(contents.keys().map(String::as_str)) {
        assert!(fam.types.iter().any(|t| t.key == key), "the family has no room type `{key}`");
    }
    for t in &fam.types {
        assert_eq!(t.window, windows.contains(t.key.as_str()), "`window` of `{}`", t.key);
        let have: Vec<(String, i64, i64)> = t.contents.iter().map(|c| (c.family.clone(), c.count.0, c.count.1)).collect();
        let listed: Vec<(String, i64, i64)> = contents
            .get(&t.key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|c| {
                (
                    c["family"].as_str().unwrap_or_default().to_string(),
                    c["count"][0].as_i64().unwrap_or(-1),
                    c["count"][1].as_i64().unwrap_or(-1),
                )
            })
            .collect();
        assert_eq!(have, listed, "`contents` of `{}`", t.key);
    }
}

#[test]
fn a_clinic_plan_has_a_passing_check_for_each_rule_of_its_pack() {
    let pack = Pack::embedded(NAME).expect("its pack");
    assert_eq!(pack.rules.len(), 7);
    for (seed, rooms) in [(1, 8), (2, 16), (3, 25)] {
        let b = build(&request(seed, rooms), false).unwrap_or_else(|r| panic!("{seed}/{rooms}: {r}"));
        let report = json(&b, "report.json");
        let checks = report["checks"].as_array().expect("checks");
        for rule in &pack.rules {
            let found: Vec<&Value> = checks.iter().filter(|c| c["id"] == rule.id.as_str()).collect();
            assert_eq!(found.len(), 1, "{seed}/{rooms}: one check for {}", rule.id);
            assert_eq!(found[0]["status"], "PASS", "{seed}/{rooms} {}: {}", rule.id, found[0]["detail"]);
        }
        assert_eq!(report["summary"]["fail"], 0, "{seed}/{rooms}");
        assert_eq!(json(&b, "manifest.json")["family_review"], "reviewed");
    }
}

#[test]
fn windows_and_contents_change_nothing_else() {
    let bare = stripped();
    let mut windows = 0;
    for (seed, rooms) in [(1, 20), (2, 14), (5, 25)] {
        let with = build(&request(seed, rooms), false).unwrap_or_else(|r| panic!("{seed}/{rooms}: {r}"));
        let without = build_family(&bare, &request(seed, rooms), false).unwrap_or_else(|r| panic!("{seed}/{rooms}: {r}"));
        assert!(lines(&without, "window").is_empty() && lines(&without, "component").is_empty());
        assert!(!lines(&with, "component").is_empty(), "{seed}/{rooms}: no component");
        windows += lines(&with, "window").len();
        assert_eq!(with.topology_hash, without.topology_hash, "{seed}/{rooms}");
        for id in ["wall", "door", "room_separator", "room"] {
            assert_eq!(lines(&with, id), lines(&without, id), "{seed}/{rooms}: {id}");
        }
        for name in ["program-graph.json", "topology-graph.json"] {
            assert_eq!(file(&with, name), file(&without, name), "{seed}/{rooms}: {name}");
        }
    }
    assert!(windows > 0, "no plan has a window");
}

#[test]
fn most_rooms_that_always_hold_a_wc_hold_one_and_only_the_listed_families_are_placed() {
    let fam = Family::parse(text()).unwrap_or_else(|e| panic!("{e}"));
    let holder = always_holds(&fam, "WC");
    let listed: BTreeSet<&str> = fam.types.iter().flat_map(|t| t.contents.iter().map(|c| c.family.as_str())).collect();
    let (mut rooms, mut wcs) = (0, 0);
    for seed in 1..=4 {
        let b = build(&request(seed, 20), false).unwrap_or_else(|r| panic!("{seed}: {r}"));
        let graph = json(&b, "topology-graph.json");
        rooms += graph["nodes"].as_array().into_iter().flatten().filter(|n| n["room_type"] == holder.as_str()).count();
        for line in lines(&b, "component") {
            let family = param(line)["family"].as_str().unwrap_or_default().to_string();
            assert!(listed.contains(family.as_str()), "{seed}: a {family} is placed, which no room type lists");
            wcs += usize::from(family == "WC");
        }
    }
    assert!(rooms >= 4, "{rooms} rooms");
    assert!(wcs * 10 >= rooms * 9, "{wcs} WCs in {rooms} rooms");
}
