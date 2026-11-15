//! The medical-day generator through its output: the files it writes, what the topology extractor reads from the project those files
//! build, and the plan judged by the pack the way the done check judges it.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::super::*;
use super::LEAST_AREA;
use crate::graph_rules::{self, Graph as RuleGraph, Pack};

fn medical(seed: u64, area: f64) -> Result<Built, Refusal> {
    build(Archetype::MedicalDay, seed, area)
}

fn file<'a>(b: &'a Built, name: &str) -> &'a str {
    &b.files.iter().find(|(n, _)| *n == name).unwrap().1
}

fn doc(b: &Built, name: &str) -> Value {
    serde_json::from_str(file(b, name)).unwrap()
}

/// The done check's 12 briefs: seeds 1 to 12 over 500, 640, 780, 920, 1060 and 1200 m2.
fn brief(s: u64) -> (u64, f64) {
    (s, [500.0, 640.0, 780.0, 920.0, 1060.0, 1200.0][(s as usize - 1) % 6])
}

fn checks(g: &Value) -> Vec<(String, bool)> {
    let pack = Pack::embedded("medical-day").unwrap();
    graph_rules::check(pack, &RuleGraph::from_value(g).unwrap())
        .unwrap()
        .into_iter()
        .map(|c| (c.id, c.status == graph_rules::Status::Pass))
        .collect()
}

/// Two rounds of neighbourhood refinement on kind and link type, as the done check hashes a structure.
fn structure(g: &Value) -> String {
    let kind: BTreeMap<String, String> =
        g["nodes"].as_array().unwrap().iter().map(|n| (n["id"].as_str().unwrap().into(), n["kind"].as_str().unwrap().into())).collect();
    let mut near: BTreeMap<String, Vec<(String, String)>> = kind.keys().map(|k| (k.clone(), Vec::new())).collect();
    for e in g["edges"].as_array().unwrap() {
        let via = if e["kind"] == "open" { "open".to_string() } else { e["operation"].as_str().unwrap_or("door").to_string() };
        let (a, b) = (e["from"].as_str().unwrap().to_string(), e["to"].as_str().unwrap().to_string());
        near.get_mut(&a).unwrap().push((b.clone(), via.clone()));
        near.get_mut(&b).unwrap().push((a, via));
    }
    let mut lab = kind.clone();
    for _ in 0..2 {
        lab = kind
            .keys()
            .map(|n| {
                let mut ns: Vec<String> = near[n].iter().map(|(x, v)| format!("{v}>{}", lab[x])).collect();
                ns.sort();
                (n.clone(), format!("{}|{}", lab[n], ns.join(",")))
            })
            .collect();
    }
    let mut all: Vec<&String> = lab.values().collect();
    all.sort();
    all.into_iter().cloned().collect::<Vec<_>>().join("\n")
}

fn commands(script: &str) -> Vec<(String, Value)> {
    script
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (c, rest) = l.split_once(' ').unwrap_or((l, "{}"));
            (c.to_string(), serde_json::from_str(rest).unwrap_or(Value::Null))
        })
        .collect()
}

/// The seeds the default tier builds.
const QUICK: [u64; 4] = [1, 4, 6, 9];

/// The done check's seeds: a service block, an office band, and the spread of areas between them.
const DONE_SEEDS: [u64; 4] = [1, 2, 4, 6];

#[test]
fn the_done_checks_seeds_all_build() {
    for s in DONE_SEEDS {
        let (seed, area) = brief(s);
        let b = medical(seed, area).unwrap_or_else(|e| panic!("seed {seed} at {area} m2: {} {}", e.code, e.message));
        assert_eq!(b.files.len(), 6, "seed {seed}");
        assert_eq!(doc(&b, "manifest.json")["archetype"], "medical-day");
        assert_eq!(doc(&b, "topology-graph.json")["archetype"], "medical-day");
    }
}

#[test]
fn the_script_is_layout_only() {
    for seed in QUICK {
        let (seed, area) = brief(seed);
        let b = medical(seed, area).unwrap();
        let g = doc(&b, "topology-graph.json");
        let cmds = commands(file(&b, "script.bimcraft.txt"));
        let count = |c: &str| cmds.iter().filter(|(n, _)| n == c).count();
        for no in ["window", "grid", "dimension", "text", "tag", "component", "furniture", "symbol", "detail_line", "sheet", "place"] {
            assert_eq!(count(no), 0, "seed {seed}: {no}");
        }
        let props: Vec<_> = cmds.iter().filter(|(n, _)| n == "drawing_property").collect();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].1["name"], "building_type");
        assert_eq!(props[0].1["value"], "hospital");
        assert_eq!(count("room"), g["nodes"].as_array().unwrap().len());
        let doors = g["edges"].as_array().unwrap().iter().filter(|e| e["kind"] == "door").count();
        assert_eq!(count("door"), doors);
        let opens = g["edges"].as_array().unwrap().iter().filter(|e| e["kind"] == "open").count();
        assert_eq!(count("room_separator"), opens);
        assert!(g["entry"].is_null());
    }
}

#[test]
fn the_saved_model_extracts_back_to_the_generated_graph() {
    for seed in QUICK {
        let (seed, area) = brief(seed);
        let b = medical(seed, area).unwrap();
        let g = doc(&b, "topology-graph.json");
        let mut s = Session::new();
        s.script(file(&b, "script.bimcraft.txt")).unwrap();
        let ext = crate::topology::extract(&s.model);
        assert!(ext["entry"].is_null(), "seed {seed}");
        assert_eq!(ext["exterior_doors"].as_array().unwrap().len(), 0);
        assert_eq!(ext["diagnostics"].as_array().unwrap().len(), 0, "seed {seed}: {}", ext["diagnostics"]);
        let ids = |v: &Value| -> BTreeSet<String> {
            v["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap().to_string()).collect()
        };
        assert_eq!(ids(&g), ids(&ext), "seed {seed}");
        let links = |v: &Value| -> Vec<(String, String, String)> {
            let mut out: Vec<_> = v["edges"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "door" || e["kind"] == "open")
                .map(|e| {
                    let (a, b) = (e["from"].as_str().unwrap().to_string(), e["to"].as_str().unwrap().to_string());
                    let (a, b) = if a <= b { (a, b) } else { (b, a) };
                    (a, b, if e["kind"] == "open" { "open".into() } else { e["operation"].as_str().unwrap().to_string() })
                })
                .collect();
            out.sort();
            out
        };
        assert_eq!(links(&g), links(&ext), "seed {seed}");
    }
}

#[test]
fn generated_and_extracted_graphs_pass_md_001_to_011() {
    for seed in QUICK {
        let (seed, area) = brief(seed);
        let b = medical(seed, area).unwrap();
        let g = doc(&b, "topology-graph.json");
        let got = checks(&g);
        assert_eq!(got.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), (1..=11).map(|i| format!("MD-{i:03}")).collect::<Vec<_>>());
        assert!(got.iter().all(|c| c.1), "seed {seed}: {got:?}");
        let mut s = Session::new();
        s.script(file(&b, "script.bimcraft.txt")).unwrap();
        let mut ext = crate::topology::extract(&s.model);
        let kinds: BTreeMap<&str, &str> =
            g["nodes"].as_array().unwrap().iter().map(|n| (n["id"].as_str().unwrap(), n["kind"].as_str().unwrap())).collect();
        for n in ext["nodes"].as_array_mut().unwrap() {
            n["kind"] = serde_json::json!(kinds[n["id"].as_str().unwrap()]);
        }
        assert!(checks(&ext).iter().all(|c| c.1), "seed {seed}");
    }
}

#[test]
fn every_plan_keeps_the_required_kinds_and_two_corridors() {
    for s in DONE_SEEDS {
        let (seed, area) = brief(s);
        let b = medical(seed, area).unwrap();
        let g = doc(&b, "topology-graph.json");
        let kinds: Vec<&str> = g["nodes"].as_array().unwrap().iter().map(|n| n["kind"].as_str().unwrap()).collect();
        for k in [
            "treatment_bay",
            "treatment_room",
            "isolation",
            "anteroom",
            "washroom",
            "medication",
            "clean_utility",
            "soiled_utility",
            "staff_work",
            "corridor",
        ] {
            assert!(kinds.contains(&k), "seed {seed}: no {k}");
        }
        assert!(kinds.iter().filter(|k| **k == "corridor").count() >= 2, "seed {seed}");
    }
}

#[test]
fn the_done_checks_seeds_vary() {
    let (mut service, mut office, mut shapes, mut hashes) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for s in DONE_SEEDS {
        let (seed, area) = brief(s);
        let b = medical(seed, area).unwrap();
        let g = doc(&b, "topology-graph.json");
        let has = |k: &str| g["nodes"].as_array().unwrap().iter().any(|n| n["kind"] == k);
        service.insert(has("plant"));
        office.insert(has("office"));
        shapes.insert(structure(&g));
        hashes.insert(b.topology_hash.clone());
    }
    assert_eq!(service.len(), 2, "plans with and without a service block");
    assert_eq!(office.len(), 2, "plans with and without an office band");
    assert_eq!(shapes.len(), DONE_SEEDS.len(), "{} structures", shapes.len());
    assert_eq!(hashes.len(), DONE_SEEDS.len());
}

#[test]
fn the_area_follows_the_request() {
    for seed in QUICK {
        let (seed, area) = brief(seed);
        let b = medical(seed, area).unwrap();
        assert!((b.gross_m2 - area).abs() / area <= 0.05, "{} for {area}", b.gross_m2);
    }
}

#[test]
fn the_same_seed_gives_the_same_bytes() {
    let (seed, area) = brief(1);
    let (a, b) = (medical(seed, area).unwrap(), medical(seed, area).unwrap());
    assert_eq!(file(&a, "script.bimcraft.txt"), file(&b, "script.bimcraft.txt"));
    assert_eq!(a.topology_hash, b.topology_hash);
}

#[test]
fn a_tiny_area_is_refused_with_a_typed_error() {
    let e = medical(1, 40.0).err().unwrap();
    assert_eq!(e.code, "unsatisfiable_area");
    assert!(medical(1, LEAST_AREA - 1.0).is_err());
}
