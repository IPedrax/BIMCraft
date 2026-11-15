//! The studio-office generator through its output: the graphs it samples, the files it writes, and what the topology extractor
//! reads from the project those files build.

use std::collections::BTreeSet;

use serde_json::Value;

use super::super::*;
use super::Means;
use super::sample::sample;
use crate::graph_rules::{self, Graph as RuleGraph, Pack};

fn studio(seed: u64, area: f64) -> Result<Built, Refusal> {
    build(Archetype::StudioOffice, seed, area)
}

fn file<'a>(b: &'a Built, name: &str) -> &'a str {
    &b.files.iter().find(|(n, _)| *n == name).unwrap().1
}

fn doc(b: &Built, name: &str) -> Value {
    serde_json::from_str(file(b, name)).unwrap()
}

#[test]
fn the_archetype_is_its_own_and_has_the_standard_files() {
    assert_eq!(Archetype::parse("studio-office"), Some(Archetype::StudioOffice));
    assert!(Archetype::names().contains("studio-office"));
    let b = studio(1, 240.0).unwrap();
    let names: Vec<&str> = b.files.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        ["program-graph.json", "topology-graph.json", "building-ir.json", "script.bimcraft.txt", "report.json", "manifest.json"]
    );
    assert_eq!(doc(&b, "manifest.json")["archetype"], "studio-office");
    assert_eq!(doc(&b, "topology-graph.json")["archetype"], "studio-office");
    assert!(b.checks.iter().all(|c| c.status == report::Status::Pass), "{:?}", b.checks);
}

#[test]
fn seeds_at_three_areas_build_and_pass_the_pack() {
    let pack = Pack::embedded("studio-office").unwrap();
    for area in [180.0, 360.0, 540.0] {
        for seed in 1..=2 {
            let b = studio(seed, area).unwrap_or_else(|e| panic!("seed {seed} at {area}: {e}"));
            let g = RuleGraph::from_value(&doc(&b, "topology-graph.json")).unwrap();
            let checks = graph_rules::check(pack, &g).unwrap();
            assert!(checks.iter().all(|c| c.status == graph_rules::Status::Pass), "seed {seed} at {area}: {checks:?}");
            let gross = b.gross_m2;
            assert!((gross - area).abs() / area < 0.08, "seed {seed}: {gross} m2 for {area}");
        }
    }
}

#[test]
fn the_same_brief_gives_the_same_bytes() {
    let (a, b) = (studio(7, 300.0).unwrap(), studio(7, 300.0).unwrap());
    assert_eq!(a.files, b.files);
    assert_ne!(studio(8, 300.0).unwrap().topology_hash, a.topology_hash);
}

#[test]
fn seeds_vary_in_structure_shell_and_program() {
    const AREAS: [f64; 7] = [180.0, 240.0, 300.0, 360.0, 420.0, 480.0, 540.0];
    let (mut hashes, mut shells, mut baths, mut more) = (BTreeSet::new(), BTreeSet::new(), 0, 0);
    let mut by_area: std::collections::BTreeMap<u64, Vec<f64>> = Default::default();
    // At 180, 300 and 540 m2: three shells, and seed 3 has more than three WCs and two meeting rooms.
    for seed in [1_u64, 3, 7] {
        let area = AREAS[(seed as usize - 1) % AREAS.len()];
        let b = studio(seed, area).unwrap_or_else(|e| panic!("seed {seed} at {area}: {e}"));
        hashes.insert(b.topology_hash.clone());
        let g = doc(&b, "topology-graph.json");
        let nodes = g["nodes"].as_array().unwrap();
        let count = |k: &str| nodes.iter().filter(|n| n["kind"] == k).count();
        baths += usize::from(count("bath") > 3);
        more += usize::from(count("meeting") > 1 || count("storage") > 1);
        shells.insert(doc(&b, "building-ir.json")["footprint"]["boundary"].as_array().unwrap().len());
        let largest = nodes.iter().max_by(|a, b| a["area_m2"].as_f64().unwrap().total_cmp(&b["area_m2"].as_f64().unwrap())).unwrap();
        assert_eq!(largest["id"], "studio", "seed {seed}");
        let floor: f64 = nodes.iter().map(|n| n["area_m2"].as_f64().unwrap()).sum();
        by_area.entry(area as u64).or_default().push(floor);
    }
    assert_eq!(hashes.len(), 3, "{} topologies", hashes.len());
    assert!(shells.len() >= 3, "{shells:?} outline sizes");
    assert!(baths > 0, "no plan with more than 3 WCs");
    assert!(more > 0, "no plan with more than one meeting room or store");
    let mean = |a: u64| by_area[&a].iter().sum::<f64>() / by_area[&a].len() as f64;
    assert!(mean(540) >= 2.0 * mean(180), "{} against {}", mean(540), mean(180));
}

#[test]
fn a_brief_below_the_least_area_is_refused_and_says_what_it_is() {
    let e = studio(1, 100.0).err().unwrap();
    assert_eq!(e.code, "unsatisfiable_area");
    assert!(e.message.contains("150"), "{}", e.message);
    assert_eq!(build(Archetype::StudioOffice, 1, f64::NAN).err().unwrap().code, "invalid_area");
}

#[test]
fn the_sampled_graph_has_the_trace_s_shape() {
    for seed in 0..30 {
        let g = sample(seed, 300.0);
        assert_eq!(g.rooms[g.entry].id, "reception");
        let open: Vec<_> = g.links.iter().filter(|l| l.means == Means::Open).collect();
        assert_eq!(open.len(), 1);
        assert_eq!((open[0].a, open[0].b), (g.passage, g.studio));
        // The reception's door to the corridor is double-swing exactly when the corridor is wide enough for one.
        let rc = g.links.iter().find(|l| l.a == g.reception && l.b == g.corridor).unwrap();
        assert_eq!(rc.means == Means::DoubleSwing, g.corridor_width >= 2000);
    }
}

#[test]
fn the_script_draws_each_door_and_window_once_in_the_dxf() {
    let b = studio(3, 300.0).unwrap();
    let script = file(&b, "script.bimcraft.txt");
    let doors = script.lines().filter(|l| l.starts_with("door ")).count();
    let windows = script.lines().filter(|l| l.starts_with("window ")).count();
    assert!(doors >= 10 && windows >= 1, "{doors} doors, {windows} windows");
    let dir = std::env::temp_dir().join(format!("bimcraft-studio-dxf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plan.dxf");
    let mut s = Session::new();
    s.script(script).unwrap();
    s.execute("save", &serde_json::json!({"path": path})).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let inserts = |layer: &str| {
        lines
            .windows(2)
            .enumerate()
            .filter(|(i, w)| w[0] == "0" && w[1] == "INSERT" && lines[i + 2..i + 40].windows(2).any(|p| p[0] == "8" && p[1] == layer))
            .count()
    };
    assert_eq!(inserts("A-DOOR"), doors);
    assert_eq!(inserts("A-GLAZ"), windows);
    let _ = std::fs::remove_dir_all(&dir);
}
