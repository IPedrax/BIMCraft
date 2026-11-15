//! `generate --delivery on`: the same layout drawn to the delivery rules, and what it writes.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::program::Archetype;
use super::tests::{built_once, doc, text};
use super::*;
use crate::engine::Session;

const ARCHETYPES: [(&str, f64); 7] = [
    ("house", 150.0),
    ("office", 900.0),
    ("clinic", 650.0),
    ("hospital", 1800.0),
    ("studio-office", 200.0),
    ("office-pattern", 400.0),
    ("medical-day", 900.0),
];

fn on(name: &str, seed: u64, area: f64) -> &'static Built {
    built_once(Archetype::parse(name).unwrap(), seed, AreaRequest::m2(area), true)
}

fn off(name: &str, seed: u64, area: f64) -> &'static Built {
    built_once(Archetype::parse(name).unwrap(), seed, AreaRequest::m2(area), false)
}

fn dir(test: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("bimcraft-delivery-{}-{test}", std::process::id()))
}

fn is_hex12(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[test]
fn delivery_other_than_on_or_off_is_refused_and_writes_nothing() {
    let d = dir("maybe");
    let _ = std::fs::remove_dir_all(&d);
    let err = run(&json!({"path": d, "archetype": "house", "seed": 1, "area_m2": 150.0, "delivery": "maybe"})).unwrap_err();
    assert_eq!(err.code, "invalid_params");
    assert!(!d.exists());
}

#[test]
fn delivery_with_rooms_is_refused_and_writes_nothing() {
    let d = dir("rooms");
    let _ = std::fs::remove_dir_all(&d);
    for value in ["on", "off"] {
        let err = run(&json!({"path": d, "archetype": "house", "seed": 1, "rooms": 12, "delivery": value})).unwrap_err();
        assert_eq!(err.code, "invalid_params", "{value}");
    }
    assert!(!d.exists());
}

#[test]
fn off_writes_the_files_of_no_option() {
    for (name, area) in ARCHETYPES {
        let (a, b) = (dir(&format!("a-{name}")), dir(&format!("b-{name}")));
        for d in [&a, &b] {
            let _ = std::fs::remove_dir_all(d);
        }
        run(&json!({"path": a, "archetype": name, "seed": 1, "area_m2": area})).unwrap();
        run(&json!({"path": b, "archetype": name, "seed": 1, "area_m2": area, "delivery": "off"})).unwrap();
        let mut names: Vec<_> = std::fs::read_dir(&a).unwrap().map(|e| e.unwrap().file_name()).collect();
        names.sort();
        assert_eq!(names.len(), 6);
        for n in names {
            assert_eq!(std::fs::read(a.join(&n)).unwrap(), std::fs::read(b.join(&n)).unwrap(), "{name} {n:?}");
        }
        for d in [&a, &b] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

fn script_lines(b: &Built) -> Vec<(String, Value)> {
    text(b, "script.bimcraft.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (id, p) = l.split_once(' ').unwrap_or((l, "{}"));
            (id.to_string(), serde_json::from_str(p).unwrap())
        })
        .collect()
}

#[test]
fn the_delivery_script_numbers_rooms_and_names_the_profile() {
    for (name, area) in ARCHETYPES {
        for seed in [1, 2] {
            let b = on(name, seed, area);
            let lines = script_lines(b);
            let ids: Vec<&str> = lines.iter().map(|(i, _)| i.as_str()).collect();
            assert_eq!(ids.iter().filter(|i| **i == "cad_profile").count(), 1, "{name}");
            let at = ids.iter().position(|i| *i == "drawing_property").unwrap();
            assert_eq!(ids[at + 1], "cad_profile", "{name}");
            assert_eq!(lines[at + 1].1, json!({"name": "delivery"}));
            assert!(!ids.contains(&"dimension"), "{name}");
            // The seed is the project's, so one script saves the same project every time.
            assert_eq!(lines[0].1["seed"], json!(seed), "{name}");
            // Every plan carries the note that says its gross area and seed, as the corridor generators' do.
            let notes: Vec<&str> = lines
                .iter()
                .filter(|(i, _)| i == "text")
                .filter_map(|(_, p)| p["text"].as_str())
                .filter(|t| t.contains(" m2 gross, seed "))
                .collect();
            assert_eq!(notes.len(), 1, "{name}: {notes:?}");
            let topology = doc(b, "topology-graph.json");
            let nodes = topology["nodes"].as_array().unwrap();
            let numbers: Vec<&str> = lines.iter().filter(|(i, _)| i == "room").map(|(_, p)| p["number"].as_str().unwrap()).collect();
            assert_eq!(numbers.len(), nodes.len(), "{name}");
            let mut by_number: BTreeMap<&str, ()> = BTreeMap::new();
            for (i, n) in nodes.iter().enumerate() {
                assert_eq!(n["number"], json!(format!("{}", 101 + i)), "{name} node {i}");
                assert!(numbers.contains(&n["number"].as_str().unwrap()), "{name}");
                by_number.insert(n["number"].as_str().unwrap(), ());
            }
            assert_eq!(by_number.len(), nodes.len());
        }
    }
}

#[test]
fn the_delivery_graph_has_the_nodes_and_edges_of_the_plain_one() {
    for (name, area) in ARCHETYPES {
        let (d, p) = (on(name, 1, area), off(name, 1, area));
        let (dt, pt) = (doc(d, "topology-graph.json"), doc(p, "topology-graph.json"));
        let ids = |t: &Value| t["nodes"].as_array().unwrap().iter().map(|n| n["id"].clone()).collect::<Vec<_>>();
        assert_eq!(ids(&dt), ids(&pt), "{name}");
        assert_eq!(dt["edges"], pt["edges"], "{name}");
    }
}

#[test]
fn the_delivery_manifest_names_the_building_and_matches_its_files() {
    for (name, area) in ARCHETYPES {
        let b = on(name, 1, area);
        let m = doc(b, "manifest.json");
        assert_eq!(m["delivery"], json!(true), "{name}");
        let building = m["building"].as_str().unwrap();
        let rest = building.strip_prefix(&format!("{name}-")).unwrap_or_else(|| panic!("{name}: {building}"));
        assert!(is_hex12(rest), "{building}");
        for f in m["files"].as_array().unwrap() {
            assert_eq!(f["hash"].as_str().unwrap(), fnv1a64(&text(b, f["path"].as_str().unwrap())), "{name} {f}");
        }
        assert_eq!(m["topology_hash"].as_str().unwrap(), fnv1a64(&text(b, "topology-graph.json")));
        assert_eq!(doc(b, "building-ir.json")["source_2d"]["topology_hash"], m["topology_hash"]);
        assert_eq!(b.topology_hash, m["topology_hash"].as_str().unwrap());
    }
}

/// The building is named for its layout: a second build gives the same name, and another layout another. One archetype a test.
fn the_building_is_the_layout(name: &str) {
    let area = ARCHETYPES.iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("{name}")).1;
    let a = doc(on(name, 1, area), "manifest.json")["building"].clone();
    // A second build of the same brief, not the first one again.
    let again = build_for_delivery(Archetype::parse(name).unwrap(), 1, AreaRequest::m2(area)).unwrap();
    assert_eq!(a, doc(&again, "manifest.json")["building"], "{name}");
    let other = doc(on(name, 3, area), "manifest.json")["building"].clone();
    let (one, three) = (off(name, 1, area), off(name, 3, area));
    let same_layout = |l: &Built, r: &Built| {
        let walls = |b: &Built| script_lines(b).into_iter().filter(|(i, _)| i == "wall").collect::<Vec<_>>();
        walls(l) == walls(r)
    };
    if !same_layout(one, three) {
        assert_ne!(a, other, "{name}");
    }
}

#[test]
fn the_building_is_the_layout_house() {
    the_building_is_the_layout("house");
}

#[test]
fn the_building_is_the_layout_office() {
    the_building_is_the_layout("office");
}

#[test]
fn the_building_is_the_layout_clinic() {
    the_building_is_the_layout("clinic");
}

#[test]
fn the_building_is_the_layout_hospital() {
    the_building_is_the_layout("hospital");
}

#[test]
fn the_building_is_the_layout_studio_office() {
    the_building_is_the_layout("studio-office");
}

#[test]
fn the_building_is_the_layout_office_pattern() {
    the_building_is_the_layout("office-pattern");
}

#[test]
fn the_building_is_the_layout_medical_day() {
    the_building_is_the_layout("medical-day");
}

// Every archetype in the table has its test above: one more in the table needs one more here.
const _: () = assert!(ARCHETYPES.len() == 7);

#[test]
fn the_delivery_script_runs_and_extracts_to_the_generated_graph() {
    for (name, area) in ARCHETYPES {
        let b = on(name, 1, area);
        let mut session = Session::new();
        session.script(&text(b, "script.bimcraft.txt")).unwrap_or_else(|e| panic!("{name}: {e}"));
        let got = crate::topology::extract(&session.model);
        assert_eq!(got["diagnostics"].as_array().map_or(0, Vec::len), 0, "{name}: {}", got["diagnostics"]);
        let want = doc(b, "topology-graph.json");
        let number_of: BTreeMap<String, String> = want["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| (n["id"].as_str().unwrap().to_string(), n["number"].as_str().unwrap().to_string()))
            .collect();
        let mut want_ids: Vec<String> = number_of.values().cloned().collect();
        let mut got_ids: Vec<String> = got["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap().to_string()).collect();
        want_ids.sort();
        got_ids.sort();
        assert_eq!(got_ids, want_ids, "{name}");
        let links = |edges: &Value, map: Option<&BTreeMap<String, String>>| {
            let mut v: Vec<String> = edges
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| {
                    ["door", "open"].contains(&e["kind"].as_str().unwrap_or("")) && e["to"] != "exterior" && e["from"] != "exterior"
                })
                .map(|e| {
                    let name = |k: &str| {
                        let s = e[k].as_str().unwrap_or("");
                        map.map_or(s.to_string(), |m| m.get(s).cloned().unwrap_or_else(|| s.to_string()))
                    };
                    let (a, b) = (name("from"), name("to"));
                    let (a, b) = if a <= b { (a, b) } else { (b, a) };
                    format!("{} {a} {b}", e["kind"])
                })
                .collect();
            v.sort();
            v
        };
        assert_eq!(links(&got["edges"], None), links(&want["edges"], Some(&number_of)), "{name}");
        let entry = want["entry"]["room"].as_str().or_else(|| want["entry"].as_str()).map(|e| number_of[e].clone());
        assert_eq!(
            got["entry"]["room"].as_str().map(str::to_string).or_else(|| got["entry"].as_str().map(str::to_string)),
            entry,
            "{name}"
        );
        // The entry is where the main door opens, never a service exit that a tie-break happened to pick.
        let door = got["entry"]["door_id"].clone();
        if let Some(d) = got["exterior_doors"].as_array().and_then(|ds| ds.iter().find(|d| d["door_id"] == door)) {
            let ty = d["type"].as_str().unwrap_or_default();
            assert!(!ty.contains("Service"), "{name}: the entry door is {ty}");
        }
    }
}
