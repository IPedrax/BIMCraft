//! The lift from the 2D topology to the 3D/BIM graph: what the two files say about themselves, that every 2D space and wall
//! and opening can be followed from the building back to the plan, and that each rule on the lift fails on the fault it names.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::Built;
use super::graph::Topology;
use super::lift::Building;
use super::report::{self, Status};
use super::script::{self, Script};
use super::tests::{BRIEFS, archetype, built, count, doc, text};
use crate::engine::Session;
use crate::model::Element;

fn lifted(name: &str, seed: u64, area: f64) -> (Built, Topology, Building) {
    let b = built(archetype(name), seed, area);
    let topology = Topology::of(b.plan.as_ref().unwrap());
    let building = Building::lift(b.plan.as_ref().unwrap(), &topology);
    (b, topology, building)
}

fn ids(v: &Value, key: &str) -> BTreeSet<String> {
    v[key].as_array().map(|a| a.iter().filter_map(|x| x["id"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn lift_status(b: &Building, t: &Topology) -> BTreeMap<&'static str, Status> {
    report::lift_checks(b, t).into_iter().map(|c| (c.id, c.status)).collect()
}

#[test]
fn the_two_graphs_say_which_is_2d_and_which_is_3d() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        assert_eq!(doc(&b, "topology-graph.json")["graph_kind"], "2d_topology", "{name} {seed}");
        assert_eq!(doc(&b, "building-ir.json")["graph_kind"], "3d_bim", "{name} {seed}");
        assert_eq!(doc(&b, "program-graph.json").get("graph_kind"), None, "the program graph is neither");
    }
}

#[test]
fn every_2d_space_is_lifted_onto_a_level() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        let (top, ir) = (doc(&b, "topology-graph.json"), doc(&b, "building-ir.json"));
        let label = format!("{name} {seed} {area}");
        let levels = ids(&ir, "levels");
        assert!(!levels.is_empty(), "{label}");
        for l in ir["levels"].as_array().unwrap() {
            assert!(l["elevation_mm"].is_i64(), "{label}: a level has its elevation in whole millimetres");
            assert!(l["name"].is_string(), "{label}");
        }
        let nodes = ids(&top, "nodes");
        let lifted: Vec<String> =
            ir["spaces"].as_array().unwrap().iter().filter_map(|s| s["source_2d"].as_str().map(str::to_string)).collect();
        assert_eq!(lifted.len(), nodes.len(), "{label}: one 3D space for each 2D one");
        assert_eq!(lifted.iter().cloned().collect::<BTreeSet<_>>(), nodes, "{label}: the corridor is lifted too, and nothing else is");
        for s in ir["spaces"].as_array().unwrap() {
            assert!(levels.contains(s["level"].as_str().unwrap_or_default()), "{label}: {s}");
            assert!(s["base_mm"].as_i64() < s["top_mm"].as_i64() && s["base_mm"].is_i64(), "{label}: {s}");
        }
        assert_eq!(ir["source_2d"]["topology_hash"], b.topology_hash.as_str(), "{label}: the building names the plan it came from");
    }
}

#[test]
fn every_3d_wall_names_the_2d_it_came_from() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        let (top, ir) = (doc(&b, "topology-graph.json"), doc(&b, "building-ir.json"));
        let label = format!("{name} {seed} {area}");
        let (nodes, levels, walls) = (ids(&top, "nodes"), ids(&ir, "levels"), ids(&ir, "walls"));
        assert_eq!(walls.len(), ir["walls"].as_array().unwrap().len(), "{label}: wall ids are unique");
        assert!(walls.len() >= 4 + 2, "{label}: four outside walls and two for the corridor, at least");
        for w in ir["walls"].as_array().unwrap() {
            let refs = strings(&w["source_2d_refs"]);
            assert!(!refs.is_empty() && refs.iter().all(|r| nodes.contains(r)), "{label}: {w}");
            assert!(levels.contains(w["level"].as_str().unwrap_or_default()), "{label}: {w}");
            assert!(w["base_mm"].as_i64() < w["top_mm"].as_i64() && w["base_mm"].is_i64(), "{label}: {w}");
            assert!(w["thickness_mm"].as_i64().unwrap_or(0) > 0 && w["type"].is_string(), "{label}: {w}");
        }
        let by_id: BTreeMap<&str, &Value> = ir["walls"].as_array().unwrap().iter().filter_map(|w| Some((w["id"].as_str()?, w))).collect();
        for id in ["wall_ext_west", "wall_ext_north", "wall_ext_east", "wall_ext_south", "wall_corr_south", "wall_corr_north"] {
            assert!(by_id.contains_key(id), "{label}: {id}");
        }
        assert!(
            ["wall_corr_south", "wall_corr_north"].iter().any(|id| strings(&by_id[id]["source_2d_refs"]).len() >= 2),
            "{label}: at least one corridor wall bounds the corridor and the rooms along it"
        );
        // A partition stands between the two rooms it is named for.
        for (id, w) in &by_id {
            if let Some(room) = id.strip_prefix("wall_part_") {
                let refs = strings(&w["source_2d_refs"]);
                assert!(refs.iter().any(|r| r == room) && refs.len() == 2, "{label}: {id} {refs:?}");
            }
        }
    }
}

#[test]
fn wall_ids_come_from_where_a_wall_stands_and_not_from_the_seed() {
    let (_, _, a) = lifted("office", 1, 900.0);
    let (_, _, b) = lifted("office", 2, 900.0);
    let ids =
        |b: &Building| -> BTreeSet<String> { b.walls.iter().map(|w| w.id.clone()).filter(|i| !i.starts_with("wall_part_")).collect() };
    for got in [ids(&a), ids(&b)] {
        for id in ["wall_corr_south", "wall_corr_north", "wall_ext_west", "wall_ext_south"] {
            assert!(got.contains(id), "{got:?}: {id}");
        }
        assert!(got.iter().any(|id| id.starts_with("wall_ext")), "{got:?}: exterior walls are named from their segments");
        assert!(got.len() >= 6, "{got:?}: corridor and exterior walls are present");
    }
}

#[test]
fn every_opening_names_its_host_wall_and_where_it_came_from() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        let (top, ir) = (doc(&b, "topology-graph.json"), doc(&b, "building-ir.json"));
        let label = format!("{name} {seed} {area}");
        let (nodes, walls) = (ids(&top, "nodes"), ids(&ir, "walls"));
        let edges = ids(&top, "edges");
        let openings = ir["openings"].as_array().unwrap();
        assert!(!openings.is_empty(), "{label}");
        let mut seen = BTreeSet::new();
        for o in openings {
            assert!(seen.insert(o["id"].as_str().unwrap_or_default().to_string()), "{label}: ids are unique");
            assert!(walls.contains(o["host"].as_str().unwrap_or_default()), "{label}: {o}");
            assert!(nodes.contains(o["source_2d"].as_str().unwrap_or_default()), "{label}: {o}");
            match o["kind"].as_str() {
                // A room's door is the door edge of the topology it is the lift of.
                Some("door") if o["role"] == "room_access" => {
                    assert!(edges.contains(o["source_topology_edge"].as_str().unwrap_or_default()), "{label}: {o}")
                }
                // The way in, the exits and the windows are no door between two rooms.
                Some("door" | "window") => assert!(o["source_topology_edge"].is_null(), "{label}: {o}"),
                other => panic!("{label}: an opening of kind {other:?}"),
            }
        }
        let doors = openings.iter().filter(|o| o["kind"] == "door").count();
        let exits = openings.iter().filter(|o| matches!(o["role"].as_str(), Some("back_door" | "service_exit"))).count();
        assert!(exits >= 1, "{label}: a way out the back");
        assert_eq!(
            doors,
            top["nodes"].as_array().unwrap().len() + exits,
            "{label}: a door for every room but the corridor, the way in, and each exit to outside"
        );
    }
}

#[test]
fn the_slab_is_on_a_level_and_nothing_is_added_to_pad_the_graph() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        let ir = doc(&b, "building-ir.json");
        let (levels, slabs) = (ids(&ir, "levels"), ir["slabs"].as_array().unwrap());
        assert_eq!(slabs.len(), 1, "{name} {seed}: the one floor the script makes");
        assert!(levels.contains(slabs[0]["level"].as_str().unwrap_or_default()));
        assert_eq!(slabs[0]["kind"], "floor");
        for none in ["vertical_circulation", "components", "systems"] {
            assert_eq!(ir[none].as_array().map(Vec::len), Some(0), "{name} {seed}: no {none} is made, and the file says so");
        }
    }
}

#[test]
fn each_lift_rule_fails_on_a_graph_that_breaks_it_and_only_that_one() {
    let (_, topology, good) = lifted("office", 1, 900.0);
    let rules = ["LIFT-001", "LIFT-002", "LIFT-003", "LIFT-004"];
    let all = lift_status(&good, &topology);
    assert_eq!(all.keys().copied().collect::<Vec<_>>(), rules);
    assert!(all.values().all(|s| *s == Status::Pass), "{all:?}");

    let with = |change: &dyn Fn(&mut Building)| {
        let mut b = good.clone();
        change(&mut b);
        b
    };
    let other_wall = good.walls.iter().find(|w| w.id == "wall_ext_south").map(|w| w.id.clone()).unwrap();
    let a_door = good.openings.iter().position(|o| o.kind == "door" && !o.entry).unwrap();
    let cases: Vec<(&str, Building)> = vec![
        ("LIFT-001", with(&|b| drop(b.spaces.pop()))),
        ("LIFT-001", with(&|b| b.spaces[0].source_2d = "nowhere".into())),
        ("LIFT-001", with(&|b| b.spaces[0].level = "level_99".into())),
        ("LIFT-001", with(&|b| b.spaces[1].top_mm = b.spaces[1].base_mm)),
        ("LIFT-002", with(&|b| b.walls[0].source_2d_refs.clear())),
        ("LIFT-002", with(&|b| b.walls[0].source_2d_refs.push("nowhere".into()))),
        ("LIFT-002", with(&|b| b.walls[0].level = "level_99".into())),
        ("LIFT-002", with(&|b| b.walls[0].thickness_mm = 0)),
        ("LIFT-002", with(&|b| b.walls[0].top_mm = b.walls[0].base_mm)),
        ("LIFT-002", with(&|b| b.walls.last_mut().into_iter().for_each(|w| w.id = "wall_ext_west".into()))),
        ("LIFT-003", with(&|b| b.openings[a_door].host = "wall_nowhere".into())),
        ("LIFT-003", with(&|b| b.openings[a_door].host = other_wall.clone())),
        ("LIFT-003", with(&|b| b.openings[a_door].source_2d = "nowhere".into())),
        ("LIFT-003", with(&|b| b.openings[a_door].source_edge = Some("door:nowhere:nowhere".into()))),
        ("LIFT-003", with(&|b| b.openings[a_door].source_edge = None)),
        ("LIFT-003", with(&|b| b.openings.clear())),
        ("LIFT-004", with(&|b| b.slabs[0].level = "level_99".into())),
        ("LIFT-004", with(&|b| b.slabs.clear())),
    ];
    for (broken, building) in &cases {
        let got = lift_status(building, &topology);
        for id in rules {
            let want = if id == *broken { Status::Fail } else { Status::Pass };
            assert_eq!(got[id], want, "{broken} broken, rule {id}: {got:?}");
        }
    }
    // A building with fewer than four walls is no building, whatever else it holds.
    let few = with(&|b| b.walls.truncate(3));
    let c = report::lift_checks(&few, &topology).into_iter().find(|c| c.id == "LIFT-002").unwrap();
    assert!(c.status == Status::Fail && c.detail.contains("fewer than 4"), "{}", c.detail);
    // The way in has no door edge, and needs none.
    let entry = good.openings.iter().position(|o| o.entry).unwrap();
    assert_eq!(good.openings[entry].source_edge, None);
}

#[test]
fn a_failed_lift_rule_says_what_to_fix() {
    let (_, topology, good) = lifted("clinic", 3, 650.0);
    let mut b = good.clone();
    b.walls[2].source_2d_refs.clear();
    let c = report::lift_checks(&b, &topology).into_iter().find(|c| c.id == "LIFT-002").unwrap();
    assert_eq!(c.status, Status::Fail);
    assert!(c.detail.contains(&b.walls[2].id), "the wall is named: {}", c.detail);
}

/// A brief's building, its script as compiled from it, and the answers of a fresh project that ran it.
fn ran(name: &str, seed: u64, area: f64) -> (Built, Building, Script, Vec<Value>, Session) {
    let (b, _, building) = lifted(name, seed, area);
    let script = script::compile(b.plan.as_ref().unwrap(), &building);
    assert_eq!(script.text, text(&b, "script.bimcraft.txt"), "the file holds the script compiled from the building");
    let mut s = Session::new();
    let replies = s.script(&script.text).unwrap_or_else(|e| panic!("{name} {seed} {area}: {e}"));
    (b, building, script, replies, s)
}

fn run_status(b: &Built, building: &Building, script: &Script, replies: &[Value], s: &Session) -> BTreeMap<&'static str, Status> {
    report::run_checks(b.plan.as_ref().unwrap(), building, script, replies, &s.model).into_iter().map(|c| (c.id, c.status)).collect()
}

#[test]
fn every_opening_is_hosted_on_the_wall_bimcraft_chose() {
    for (name, seed, area) in BRIEFS {
        let (b, building, script, replies, s) = ran(name, seed, area);
        let label = format!("{name} {seed} {area}");
        // The wall elements, by the building's wall they were made for.
        let mut element: BTreeMap<&str, u64> = BTreeMap::new();
        for (l, r) in script.lines.iter().zip(&replies) {
            if let Some(w) = &l.wall_of {
                assert_eq!(r["ids"].as_array().map(Vec::len), Some(1), "{label}: a wall command makes one wall");
                element.insert(w, r["ids"][0].as_u64().unwrap());
            }
        }
        assert_eq!(element.len(), building.walls.len(), "{label}: a command for each wall of the building");
        let mut hosted = 0;
        for (l, r) in script.lines.iter().zip(&replies) {
            let Some(o) = &l.opening_of else { continue };
            let declared = &building.openings.iter().find(|x| &x.id == o).unwrap_or_else(|| panic!("{label}: {o}")).host;
            assert_eq!(
                r["host"].as_u64(),
                element.get(declared.as_str()).copied(),
                "{label}: {o} is not in {declared} as BIMCraft hosts it"
            );
            hosted += 1;
        }
        assert_eq!(hosted, building.openings.len(), "{label}: a command for each opening of the building");
        assert_eq!(
            count(&s, |e| matches!(e, Element::Wall(_))),
            building.walls.len(),
            "{label}: a wall element for each wall of the building"
        );
        let through = run_status(&b, &building, &script, &replies, &s);
        assert_eq!(through["LIFT-005"], Status::Pass, "{label}");
        assert_eq!(through["LIFT-006"], Status::Pass, "{label}");
    }
}

#[test]
fn the_script_makes_the_walls_of_the_plan_where_the_plan_put_them() {
    for (name, seed, area) in BRIEFS {
        let (b, building, _, _, s) = ran(name, seed, area);
        let made: Vec<([i64; 2], [i64; 2])> = s
            .model
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Wall(w) => Some(([w.start.0 as i64, w.start.1 as i64], [w.end.0 as i64, w.end.1 as i64])),
                _ => None,
            })
            .collect();
        // The plan's polylines, a segment to a wall, in the order the plan lists them.
        let planned: Vec<([i64; 2], [i64; 2])> = b
            .plan
            .as_ref()
            .unwrap()
            .walls
            .iter()
            .flat_map(|w| {
                let mut pts = w.points.clone();
                if w.closed {
                    pts.extend(w.points.first().copied());
                }
                pts.windows(2).map(|p| (p[0], p[1])).collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(made, planned, "{name} {seed} {area}");
        assert_eq!(building.walls.iter().map(|w| (w.start, w.end)).collect::<Vec<_>>(), planned);
    }
}

#[test]
fn each_run_time_lift_rule_fails_on_a_building_that_breaks_it_and_only_that_one() {
    let (b, good, script, replies, s) = ran("office", 1, 900.0);
    let rules = ["LIFT-005", "LIFT-006"];
    let now = run_status(&b, &good, &script, &replies, &s);
    assert!(rules.iter().all(|r| now[r] == Status::Pass), "{now:?}");

    let with = |change: &dyn Fn(&mut Building)| {
        let mut x = good.clone();
        change(&mut x);
        x
    };
    let door = good.openings.iter().position(|o| o.kind == "door" && !o.entry).unwrap();
    let window = good.openings.iter().position(|o| o.kind == "window").unwrap();
    let other = |host: &str| if host == "wall_ext_south" { "wall_ext_north" } else { "wall_ext_south" }.to_string();
    let part = good.walls.iter().position(|w| w.id.starts_with("wall_part_")).unwrap();
    let cases: Vec<(&str, Building)> = vec![
        ("LIFT-005", with(&|x| x.openings[door].host = other(&good.openings[door].host))),
        ("LIFT-005", with(&|x| x.openings[window].host = "wall_corr_south".into())),
        ("LIFT-005", with(&|x| x.openings[window].host = "wall_nowhere".into())),
        ("LIFT-005", with(&|x| drop(x.openings.pop()))),
        ("LIFT-006", with(&|x| x.walls[part].thickness_mm = 200)),
        ("LIFT-006", with(&|x| x.walls[part].top_mm = 3600)),
        ("LIFT-006", with(&|x| x.walls[part].base_mm = 100)),
        ("LIFT-006", with(&|x| x.walls[part].start[1] += 100)),
        ("LIFT-006", with(&|x| x.levels[1].elevation_mm = 3300)),
        ("LIFT-006", with(&|x| drop(x.walls.pop()))),
    ];
    for (broken, building) in &cases {
        let got = run_status(&b, building, &script, &replies, &s);
        for id in rules {
            let want = if id == *broken { Status::Fail } else { Status::Pass };
            assert_eq!(got[id], want, "{broken} broken, rule {id}: {got:?}");
        }
    }
}

/// The building graphs of two seeds of every archetype, as the files say them: between them the four symbol profiles.
fn symbol_spread() -> Vec<(String, Value, Value)> {
    let mut out = Vec::new();
    for (name, area, seeds) in [("office", 900.0, [0, 10]), ("clinic", 650.0, [1, 2]), ("hospital", 1800.0, [0, 1])] {
        for seed in seeds {
            let b = built(archetype(name), seed, area);
            out.push((format!("{name} {seed} {area}"), doc(&b, "building-ir.json"), doc(&b, "manifest.json")));
        }
    }
    out
}

fn symbols_of(ir: &Value, kind: &str) -> BTreeSet<String> {
    ir["openings"]
        .as_array()
        .map(|a| a.iter().filter(|o| o["kind"] == kind).filter_map(|o| o["symbol"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

#[test]
fn seeds_and_archetypes_export_many_door_and_window_symbols() {
    let spread = symbol_spread();
    let (mut profiles, mut doors, mut windows) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for (label, ir, manifest) in &spread {
        profiles.insert(manifest["opening_symbol_profile"].as_str().unwrap_or_default().to_string());
        assert_eq!(ir["opening_symbol_profile"], manifest["opening_symbol_profile"], "{label}");
        let (d, w) = (symbols_of(ir, "door"), symbols_of(ir, "window"));
        // Each building has its own variety: the way in, a way out the back, and multiple room-door symbols.
        assert!(d.contains("main_entry"), "{label}: {d:?}");
        assert!(d.contains("back_door") || d.contains("service_exit"), "{label}: {d:?}");
        assert!(d.len() >= 3, "{label}: {d:?}");
        assert!(!w.is_empty(), "{label}");
        doors.extend(d);
        windows.extend(w);
    }
    assert_eq!(profiles.len(), 4, "{profiles:?}");
    for symbol in ["main_entry", "back_door", "service_exit", "single_swing", "double_swing", "service", "secure"] {
        assert!(doors.contains(symbol), "no door has the symbol {symbol}: {doors:?}");
    }
    for symbol in ["punched", "ribbon", "storefront", "clerestory", "narrow_slot"] {
        assert!(windows.contains(symbol), "no window has the symbol {symbol}: {windows:?}");
    }
    // Different seeds of one archetype do not all draw alike.
    for name in ["office", "clinic", "hospital"] {
        let seen: BTreeSet<&str> =
            spread.iter().filter(|(l, ..)| l.starts_with(name)).filter_map(|(.., m)| m["opening_symbol_profile"].as_str()).collect();
        assert!(seen.len() >= 2, "{name}: {seen:?}");
    }
}

#[test]
fn each_opening_names_its_role_symbol_and_size_and_the_manifest_counts_them() {
    for (label, ir, manifest) in symbol_spread() {
        let openings = ir["openings"].as_array().unwrap();
        let mut types: BTreeMap<(String, String), (i64, i64, i64, String)> = BTreeMap::new();
        for o in openings {
            let (kind, symbol, role) = (o["kind"].as_str().unwrap(), o["symbol"].as_str().unwrap(), o["role"].as_str().unwrap());
            match kind {
                "door" => assert!(matches!(role, "main_entry" | "back_door" | "service_exit" | "room_access"), "{label}: {o}"),
                _ => assert_eq!(role, "window", "{label}: {o}"),
            }
            // The entries are labelled by what they are for; a room door by how it is drawn.
            if matches!(role, "main_entry" | "back_door" | "service_exit") {
                assert_eq!(symbol, role, "{label}: {o}");
                assert!(o["source_topology_edge"].is_null(), "{label}: {o}");
            } else if kind == "door" {
                assert!(matches!(symbol, "single_swing" | "double_swing" | "service" | "secure"), "{label}: {o}");
            }
            let (w, h, s) = (o["width_mm"].as_i64().unwrap(), o["height_mm"].as_i64().unwrap(), o["sill_mm"].as_i64().unwrap());
            assert!(w >= 600 && h >= 600 && s >= 0 && s + h <= 3000 + 100, "{label}: {o}");
            assert!(o["flip_hand"].is_boolean() && o["operation"].as_str().is_some_and(|op| !op.is_empty()), "{label}: {o}");
            // A type name stands for one size and one way of opening.
            let spec = (w, h, s, o["operation"].as_str().unwrap().to_string());
            let key = (kind.to_string(), o["type"].as_str().unwrap().to_string());
            assert_eq!(types.entry(key).or_insert_with(|| spec.clone()), &spec, "{label}: {o}");
        }
        let double = openings.iter().find(|o| o["symbol"] == "double_swing").map(|o| (o["width_mm"].as_i64(), o["operation"].clone()));
        if let Some((width, operation)) = double {
            assert!(width.unwrap_or(0) > 1200 && operation == "double-swing", "{label}: a double door is wide and has two leaves");
        }
        // The manifest counts what the building graph holds, door symbols and window symbols apart.
        let count = |kind: &str| {
            let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
            for o in openings.iter().filter(|o| o["kind"] == kind) {
                *counts.entry(o["symbol"].as_str().unwrap()).or_default() += 1;
            }
            serde_json::to_value(counts).unwrap()
        };
        assert_eq!(manifest["opening_symbols"]["doors"], count("door"), "{label}");
        assert_eq!(manifest["opening_symbols"]["windows"], count("window"), "{label}");
    }
}

#[test]
fn the_script_makes_a_type_for_each_symbol_before_the_doors_and_windows_that_use_it() {
    for (name, seed, area) in [("office", 1, 900.0), ("clinic", 3, 650.0), ("hospital", 10, 1800.0)] {
        let (b, building, script, replies, s) = ran(name, seed, area);
        let label = format!("{name} {seed} {area}");
        let first_opening = script.lines.iter().position(|l| l.opening_of.is_some()).unwrap();
        let types: Vec<&script::Line> = script.lines.iter().filter(|l| l.id == "type").collect();
        assert!(types.iter().all(|l| script.lines.iter().position(|x| std::ptr::eq(x, *l)).unwrap() < first_opening), "{label}");
        for o in &building.openings {
            let line = script.lines.iter().find(|l| l.opening_of.as_deref() == Some(o.id.as_str())).unwrap();
            assert_eq!(line.params["type"], o.type_name, "{label}: {}", o.id);
            assert!(
                types.iter().any(|t| t.params["name"] == o.type_name && t.params["category"] == o.kind && t.params["width"] == o.width_mm),
                "{label}: no type for {}",
                o.id
            );
        }
        // BIMCraft took them: the project holds a type of each symbol and the doors and windows name it.
        let used: BTreeSet<&str> = building.openings.iter().map(|o| o.type_name.as_str()).collect();
        let doors = s.model.types.door.keys().chain(s.model.types.window.keys()).filter(|k| used.contains(k.as_str())).count();
        assert_eq!(doors, used.len(), "{label}");
        assert_eq!(b.plan.as_ref().unwrap().openings.len(), building.openings.len(), "{label}");
        assert_eq!(replies.len(), script.lines.len(), "{label}");
    }
}

#[test]
fn a_symbol_the_script_did_not_make_fails_the_symbol_rule() {
    let (b, good, script, replies, s) = ran("office", 1, 900.0);
    let with = |change: &dyn Fn(&mut Building)| {
        let mut x = good.clone();
        change(&mut x);
        x
    };
    let door = good.openings.iter().position(|o| o.kind == "door" && o.role == "room_access").unwrap();
    let window = good.openings.iter().position(|o| o.kind == "window").unwrap();
    let cases = vec![
        with(&|x| x.openings[door].width_mm += 100),
        with(&|x| x.openings[door].operation = "sliding"),
        with(&|x| x.openings[door].flip_hand = !good.openings[door].flip_hand),
        with(&|x| x.openings[window].sill_mm += 100),
        with(&|x| x.openings[window].type_name = "Nowhere 1 x 1".into()),
    ];
    assert_eq!(run_status(&b, &good, &script, &replies, &s)["LIFT-007"], Status::Pass);
    for broken in &cases {
        assert_eq!(run_status(&b, broken, &script, &replies, &s)["LIFT-007"], Status::Fail);
    }
    // A symbol that no door has is refused before the script runs.
    let t = Topology::of(b.plan.as_ref().unwrap());
    let bad = with(&|x| x.openings[door].symbol = "revolving");
    assert_eq!(lift_status(&bad, &t)["LIFT-003"], Status::Fail);
    assert_eq!(lift_status(&good, &t)["LIFT-003"], Status::Pass);
}
