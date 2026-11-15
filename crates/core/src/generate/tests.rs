//! The generator through its command and its files: what a brief writes, what it refuses, and that what it writes runs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{Value, json};

use super::graph::{Edge, Node, Topology};
use super::program::{Archetype, Role};
use super::report::{self, Status};
use super::*;
use crate::engine::Session;
use crate::model::Element;

pub(super) fn built(archetype: Archetype, seed: u64, area: f64) -> Built {
    build(archetype, seed, area).unwrap_or_else(|e| panic!("{} {seed} {area}: {e}", archetype.name()))
}

pub(super) fn text(b: &Built, name: &str) -> String {
    b.files.iter().find(|(n, _)| *n == name).map(|(_, t)| t.clone()).unwrap_or_else(|| panic!("no {name}"))
}

pub(super) fn doc(b: &Built, name: &str) -> Value {
    serde_json::from_str(&text(b, name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn dir(test: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bimcraft-generate-{}-{test}", std::process::id()))
}

fn brief(path: &std::path::Path, archetype: &str, seed: u64, area: f64) -> Value {
    json!({"path": path, "archetype": archetype, "seed": seed, "area_m2": area})
}

/// One brief per archetype, and the smallest and largest office.
pub(super) const BRIEFS: [(&str, u64, f64); 4] =
    [("office", 3, 300.0), ("office", 11, 5000.0), ("clinic", 3, 650.0), ("hospital", 10, 1800.0)];

pub(super) fn archetype(name: &str) -> Archetype {
    Archetype::parse(name).unwrap_or_else(|| panic!("{name}"))
}

/// The topology as a file reader sees it: its nodes and its door and open edges.
type Nodes = BTreeMap<String, (String, String)>;

fn read_topology(t: &Value) -> (Nodes, Vec<(String, String)>, String) {
    let nodes = t["nodes"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|n| {
                    (
                        n["id"].as_str().unwrap_or_default().to_string(),
                        (n["kind"].as_str().unwrap_or_default().to_string(), n["role"].as_str().unwrap_or_default().to_string()),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let access = t["edges"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|e| matches!(e["kind"].as_str(), Some("door" | "open")))
                .map(|e| (e["from"].as_str().unwrap_or_default().to_string(), e["to"].as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default();
    (nodes, access, t["entry"].as_str().unwrap_or_default().to_string())
}

fn reach(access: &[(String, String)], from: BTreeSet<String>) -> BTreeSet<String> {
    let mut seen = from;
    loop {
        let more: Vec<String> = access
            .iter()
            .flat_map(|(a, b)| [(a, b), (b, a)])
            .filter(|(a, b)| seen.contains(*a) && !seen.contains(*b))
            .map(|(_, b)| b.clone())
            .collect();
        if more.is_empty() {
            return seen;
        }
        seen.extend(more);
    }
}

#[test]
fn an_area_too_small_for_the_program_is_refused_with_a_typed_diagnostic_and_nothing_is_written() {
    let e = build(Archetype::Office, 1, 40.0).err().unwrap_or_else(|| panic!("40 m2 office was built"));
    assert_eq!(e.code, "unsatisfiable_area");
    assert!(e.message.contains("too small") && e.to_string().starts_with("unsatisfiable_area: "), "{e}");
    assert!(e.message.contains("about"), "the message says what area would do: {e}");

    let out = dir("too-small");
    let mut s = Session::new();
    let said = s.execute("generate", &brief(&out, "office", 1, 40.0)).unwrap_err();
    assert!(said.starts_with("unsatisfiable_area: ") && said.contains("unsatisfiable"), "{said}");
    assert!(!out.exists(), "an impossible brief makes no directory");

    // In a directory that is already there, no script is left either.
    std::fs::create_dir_all(&out).unwrap();
    s.execute("generate", &brief(&out, "clinic", 1, 40.0)).unwrap_err();
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn the_least_area_a_brief_is_met_at_works() {
    for name in ["office", "clinic", "hospital"] {
        let a = archetype(name);
        let least = least_area(a, 1).unwrap_or_else(|| panic!("{name} has a least area"));
        assert!(build(a, 1, least).is_ok(), "{name} at {least}");
        assert!(build(a, 1, least - 10.0).is_err(), "{name} just under {least}");
    }
}

#[test]
fn hostile_briefs_are_refused_before_any_file_is_touched() {
    let out = dir("hostile");
    let ok = |o: &std::path::Path| brief(o, "office", 1, 900.0);
    let mut cases: Vec<(Value, &str)> = vec![
        (json!(null), "invalid_params"),
        (json!([]), "invalid_params"),
        (json!("x"), "invalid_params"),
        (json!({}), "invalid_params"),
        (json!({"path": out, "archetype": "office", "seed": 1}), "invalid_params"),
        (json!({"path": out, "archetype": "office", "seed": 1, "area_m2": 900, "colour": "red"}), "invalid_params"),
        (json!({"path": out, "archetype": "office", "seed": -1, "area_m2": 900}), "invalid_params"),
        (json!({"path": out, "archetype": "office", "seed": 1.5, "area_m2": 900}), "invalid_params"),
        (json!({"path": out, "archetype": "office", "seed": "1", "area_m2": 900}), "invalid_params"),
        (json!({"path": out, "archetype": "office", "seed": 1, "area_m2": "big"}), "invalid_params"),
        (json!({"path": 5, "archetype": "office", "seed": 1, "area_m2": 900}), "invalid_params"),
        (json!({"path": "", "archetype": "office", "seed": 1, "area_m2": 900}), "invalid_params"),
        (json!({"path": "  ", "archetype": "office", "seed": 1, "area_m2": 900}), "invalid_params"),
        (json!({"path": "a\0b", "archetype": "office", "seed": 1, "area_m2": 900}), "invalid_params"),
        (json!({"path": "x".repeat(5000), "archetype": "office", "seed": 1, "area_m2": 900}), "invalid_params"),
        (json!({"path": out, "archetype": "", "seed": 1, "area_m2": 900}), "unknown_archetype"),
        (json!({"path": out, "archetype": 5, "seed": 1, "area_m2": 900}), "invalid_params"),
        (json!({"path": out, "archetype": "datacenter", "seed": 1, "area_m2": 900}), "unknown_archetype"),
        (json!({"path": out, "archetype": "Office", "seed": 1, "area_m2": 900}), "unknown_archetype"),
        (json!({"path": out, "archetype": "x".repeat(100_000), "seed": 1, "area_m2": 900}), "unknown_archetype"),
    ];
    // JSON has no NaN or infinity: they come as null.
    for area in [f64::NAN, f64::INFINITY] {
        cases.push((json!({"path": out, "archetype": "office", "seed": 1, "area_m2": area}), "invalid_params"));
    }
    assert!(build(Archetype::Office, 1, f64::NAN).is_err() && build(Archetype::Office, 1, f64::INFINITY).is_err());
    for area in [0.0, -1.0, 1e300, f64::MAX, 5000.5, 1e9] {
        cases.push((json!({"path": out, "archetype": "office", "seed": 1, "area_m2": area}), "invalid_area"));
    }
    for area in [5e-324, 0.5, 1.0, 40.0, 99.0] {
        cases.push((json!({"path": out, "archetype": "clinic", "seed": u64::MAX, "area_m2": area}), "unsatisfiable_area"));
    }
    let mut s = Session::new();
    let before = std::sync::Arc::clone(&s.model);
    for (p, code) in &cases {
        let e = s.execute("generate", p).unwrap_err();
        assert!(e.starts_with(&format!("{code}: ")), "{p}: {e}");
        assert!(e.chars().count() < 1000, "a refusal stays short: {} characters", e.chars().count());
    }
    assert!(!out.exists(), "nothing was made");
    // A request that is fine is the control: it is the table that refuses, not the command.
    s.execute("generate", &ok(&out)).unwrap();
    assert!(out.join("manifest.json").is_file());
    std::fs::remove_dir_all(&out).unwrap();
    assert!(s.model.check().is_ok());
    assert!(std::sync::Arc::ptr_eq(&s.model, &before), "the command does not touch the session's project");
}

#[test]
fn a_directory_that_cannot_be_written_is_refused_and_leaves_nothing_behind() {
    let base = dir("blocked");
    std::fs::create_dir_all(&base).unwrap();
    let file = base.join("in-the-way");
    std::fs::write(&file, "x").unwrap();
    let mut s = Session::new();
    for path in [file.clone(), file.join("run")] {
        let e = s.execute("generate", &brief(&path, "office", 1, 900.0)).unwrap_err();
        assert!(e.starts_with("output_unwritable: "), "{e}");
    }
    // One of the files can't be made: a directory stands where it goes, and the files before it are taken away again.
    let run = base.join("run");
    std::fs::create_dir_all(run.join("report.json")).unwrap();
    let e = s.execute("generate", &brief(&run, "office", 1, 900.0)).unwrap_err();
    assert!(e.starts_with("output_unwritable: "), "{e}");
    let left: Vec<String> = std::fs::read_dir(&run).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(left, ["report.json"]);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_brief_writes_the_six_files_and_the_manifest_names_them_with_their_hashes() {
    let out = dir("files");
    let mut s = Session::new();
    let r = s.execute("generate", &brief(&out, "office", 1, 900.0)).unwrap();
    assert_eq!(r["archetype"], "office");
    assert_eq!(r["checks"]["fail"], 0);
    let names = ["program-graph.json", "topology-graph.json", "building-ir.json", "script.bimcraft.txt", "report.json", "manifest.json"];
    for n in names {
        assert!(std::fs::metadata(out.join(n)).unwrap().len() > 0, "{n}");
    }
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        (manifest["archetype"].as_str(), manifest["seed"].as_u64(), manifest["requested_area_m2"].as_f64()),
        (Some("office"), Some(1), Some(900.0))
    );
    assert_eq!(manifest["requested_area"], json!({"unit": "m2", "value": 900.0}));
    assert_eq!(manifest["layout_family"].as_str(), Some("corridor_bar"));
    assert_eq!(manifest["layout_strategy"].as_str(), Some("single_loaded_corridor"));
    assert_eq!(manifest["boundary_shape"].as_str(), Some("l_shape"));
    assert_eq!(manifest["generator_version"], GENERATOR_VERSION);
    assert_eq!(manifest["hash_algorithm"], "fnv1a64");
    let listed = manifest["files"].as_array().unwrap();
    assert_eq!(listed.len(), 5);
    for f in listed {
        let name = f["path"].as_str().unwrap();
        assert!(!name.contains('/') && !name.contains(".."), "paths are names in the run directory: {name}");
        assert_eq!(f["hash"].as_str().unwrap(), fnv1a64(&std::fs::read_to_string(out.join(name)).unwrap()), "{name}");
    }
    let topology = std::fs::read_to_string(out.join("topology-graph.json")).unwrap();
    assert_eq!(manifest["topology_hash"].as_str().unwrap(), fnv1a64(&topology));
    let topology_doc: Value = serde_json::from_str(&topology).unwrap();
    assert_eq!(topology_doc["layout_family"], manifest["layout_family"]);
    assert_eq!(topology_doc["layout_strategy"], manifest["layout_strategy"]);
    assert_eq!(topology_doc["boundary_shape"], manifest["boundary_shape"]);
    let building_doc = doc(&build(Archetype::Office, 1, 900.0).unwrap(), "building-ir.json");
    assert_eq!(building_doc["layout_family"], manifest["layout_family"]);
    assert_eq!(building_doc["layout_strategy"], manifest["layout_strategy"]);
    assert_eq!(building_doc["boundary_shape"], manifest["boundary_shape"]);
    assert!(building_doc["footprint"]["boundary"].as_array().unwrap().len() > 4);
    assert!(manifest["gross_area_m2"].as_f64().unwrap() > 800.0);
    let all = std::fs::read_to_string(out.join("manifest.json")).unwrap();
    assert!(!all.contains(out.to_str().unwrap()), "the manifest holds no machine's path");
    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn square_foot_briefs_are_converted_and_area_flags_are_exclusive() {
    let out = dir("sqft");
    let mut s = Session::new();
    s.execute("generate", &json!({"path": out, "archetype": "office", "seed": 2, "area_sqft": 10_000.0})).unwrap();
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["requested_area"], json!({"unit": "sqft", "value": 10000.0}));
    assert!((manifest["requested_area_m2"].as_f64().unwrap() - 929.0304).abs() < 0.01);
    std::fs::remove_dir_all(&out).unwrap();

    let e = s
        .execute("generate", &json!({"path": out, "archetype": "office", "seed": 2, "area_m2": 900.0, "area_sqft": 10_000.0}))
        .unwrap_err();
    assert!(e.starts_with("invalid_params: ") && e.contains("mutually exclusive"), "{e}");
}

#[test]
fn seeds_choose_different_layout_strategies() {
    let strategies: BTreeSet<String> = (0..3)
        .map(|seed| doc(&built(Archetype::Hospital, seed, 1800.0), "manifest.json")["layout_strategy"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(strategies.len(), 3, "{strategies:?}");
}

#[test]
fn seeds_choose_different_boundary_shapes() {
    let shapes: BTreeSet<String> = (0..5)
        .map(|seed| doc(&built(Archetype::Hospital, seed, 1800.0), "manifest.json")["boundary_shape"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(shapes.len(), 5, "{shapes:?}");
}

#[test]
fn seeds_choose_different_layout_families() {
    let families: BTreeSet<String> = [1, 4, 7, 10]
        .into_iter()
        .map(|seed| doc(&built(Archetype::Hospital, seed, 1800.0), "manifest.json")["layout_family"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(families.len(), 4, "{families:?}");
}

#[test]
fn the_graphs_meet_the_rules_as_a_reader_of_the_files_sees_them() {
    let private = ["private", "clinical"];
    let open = ["circulation", "shared", "public"];
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        let (nodes, access, entry) = read_topology(&doc(&b, "topology-graph.json"));
        let label = format!("{name} {seed} {area}");
        let ids: BTreeSet<String> = nodes.keys().cloned().collect();
        assert_eq!(ids.len(), nodes.len(), "{label}: ids are unique");
        assert!(nodes.len() >= 6, "{label}");
        assert!(ids.contains(&entry), "{label}");
        for (a, b) in &access {
            assert!(ids.contains(a) && ids.contains(b), "{label}: {a} -> {b}");
            let (ra, rb) = (nodes[a].1.as_str(), nodes[b].1.as_str());
            assert!(!(private.contains(&ra) && private.contains(&rb)), "{label}: {a} opens into {b}");
        }
        let from_open = reach(&access, nodes.iter().filter(|(_, (_, r))| open.contains(&r.as_str())).map(|(i, _)| i.clone()).collect());
        for (id, (_, role)) in &nodes {
            assert!(!private.contains(&role.as_str()) || from_open.contains(id), "{label}: {id} is cut off");
        }
        assert_eq!(reach(&access, BTreeSet::from([entry])), ids, "{label}: everything is reached from the entry");
        // Adjacency is in the file, kept apart from access, and no rule reads it.
        let kinds: BTreeSet<String> =
            doc(&b, "topology-graph.json")["edges"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap().to_string()).collect();
        assert!(kinds.contains("door") && kinds.contains("adjacent"), "{label}: {kinds:?}");
    }
}

#[test]
fn the_minimum_programs_are_present() {
    let kinds = |b: &Built| -> Vec<String> { read_topology(&doc(b, "topology-graph.json")).0.values().map(|(k, _)| k.clone()).collect() };
    for seed in 0..2 {
        let office = kinds(&built(Archetype::Office, seed, 900.0));
        for need in ["entry_lobby", "corridor", "open_office", "conference", "restroom"] {
            assert!(office.iter().any(|k| k == need), "office {seed}: {need}");
        }
        assert!(office.iter().any(|k| k == "private_office" || k == "focus_room"), "office {seed}");
        assert!(office.iter().any(|k| k == "it_storage" || k == "pantry"), "office {seed}");
        let clinic = kinds(&built(Archetype::Clinic, seed, 650.0));
        for need in ["entry_lobby", "waiting", "corridor", "restroom", "clean_storage"] {
            assert!(clinic.iter().any(|k| k == need), "clinic {seed}: {need}");
        }
        assert!(clinic.iter().filter(|k| *k == "exam").count() >= 2, "clinic {seed}: two exam rooms");
        assert!(clinic.iter().any(|k| k == "staff_room" || k == "nurse_station"), "clinic {seed}");
    }
}

fn node(id: &str, kind: &str, role: Role) -> Node {
    Node { id: id.into(), kind: kind.into(), role, area_m2: 10.0 }
}

fn edge(from: &str, to: &str, kind: &'static str) -> Edge {
    Edge { from: from.into(), to: to.into(), kind }
}

fn status(t: &Topology, id: &str) -> Status {
    report::graph_checks(t, Archetype::Clinic).into_iter().find(|c| c.id == id).unwrap_or_else(|| panic!("no rule {id}")).status
}

#[test]
fn each_rule_fails_on_a_graph_that_breaks_it_and_only_that_one() {
    // The smallest graph that meets them all, then one fault at a time.
    let good = Topology {
        entry: "lobby".into(),
        nodes: vec![
            node("lobby", "entry_lobby", Role::Public),
            node("hall", "corridor", Role::Circulation),
            node("exam_1", "exam", Role::Clinical),
            node("exam_2", "exam", Role::Clinical),
            node("office", "private_office", Role::Private),
        ],
        edges: vec![
            edge("lobby", "hall", "open"),
            edge("exam_1", "hall", "door"),
            edge("exam_2", "hall", "door"),
            edge("office", "hall", "door"),
        ],
    };
    let rules = ["GR-001", "GR-002", "GR-003", "GR-004", "GR-005", "GR-006"];
    for id in rules {
        assert_eq!(status(&good, id), Status::Pass, "{id}");
    }
    let with = |change: &dyn Fn(&mut Topology)| {
        let mut t = good.clone();
        change(&mut t);
        t
    };
    let cases: [(&str, Topology); 6] = [
        ("GR-001", with(&|t| t.edges.push(edge("exam_1", "nowhere", "door")))),
        ("GR-002", with(&|t| t.edges.retain(|e| e.from != "office"))),
        (
            "GR-003",
            with(&|t| {
                t.nodes.push(node("office_2", "private_office", Role::Private));
                t.edges.extend([edge("office_2", "office", "door"), edge("office_2", "hall", "door")]);
            }),
        ),
        ("GR-004", with(&|t| t.edges.push(edge("exam_1", "exam_2", "door")))),
        ("GR-005", with(&|t| t.edges.push(edge("exam_1", "office", "open")))),
        ("GR-006", with(&|t| t.nodes.push(node("store", "clean_storage", Role::Support)))),
    ];
    for (broken, t) in &cases {
        for id in rules {
            let want = if id == *broken { Status::Fail } else { Status::Pass };
            // A cut-off private space is also a lost one, and a cut-off one is also not reached from the entry.
            if matches!((*broken, id), ("GR-002", "GR-006")) {
                continue;
            }
            assert_eq!(status(t, id), want, "{broken} broken, rule {id}");
        }
    }
    // Soft relations are no access: two rooms that are only adjacent, or near, break nothing.
    let soft = with(&|t| t.edges.extend([edge("exam_1", "exam_2", "adjacent"), edge("exam_1", "office", "near")]));
    for id in rules {
        assert_eq!(status(&soft, id), Status::Pass, "{id}");
    }
    // A program missing a kind fails PR-001 and says which.
    let c = report::graph_checks(&good, Archetype::Clinic).into_iter().find(|c| c.id == "PR-001").unwrap();
    assert_eq!(c.status, Status::Fail);
    assert!(c.detail.contains("waiting room"), "{}", c.detail);
}

/// FNV-1a, so a golden value does not move with the standard library.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The rules and the report were hard-coded and now read their parameters from the packs: what they say, for every brief,
/// must not move by a byte. The golden values are those of the code that matched the hard-coded rules over sixteen briefs,
/// taken over these four.
#[test]
fn the_reports_and_rule_checks_are_the_same_bytes_as_when_the_rules_were_hard_coded() {
    let (mut reports, mut graphs, mut checks) = (String::new(), String::new(), String::new());
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        reports.push_str(&text(&b, "report.json"));
        graphs.push_str(&format!("{name} {seed} {}\n", fnv(&text(&b, "topology-graph.json"))));
        for c in &b.checks {
            checks.push_str(&format!("{}|{}|{:?}|{}\n", c.id, c.name, c.status, c.detail));
        }
    }
    assert_eq!(
        (fnv(&reports), fnv(&graphs), fnv(&checks)),
        (8587798742519512955, 11004902002662719616, 3559498174140652704),
        "the golden values"
    );
}

pub(super) fn count(s: &Session, f: impl Fn(&Element) -> bool) -> usize {
    s.model.elements.values().filter(|e| f(e)).count()
}

#[test]
fn the_script_runs_in_a_fresh_project_and_makes_a_plan_with_bounded_rooms() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        let script = text(&b, "script.bimcraft.txt");
        let mut ids = BTreeSet::new();
        let mut commands = 0;
        for l in script.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            commands += 1;
            ids.insert(l.split_whitespace().next().unwrap().to_string());
        }
        let label = format!("{name} {seed} {area}");
        assert!(commands >= 30, "{label}: {commands}");
        for id in ["new", "level", "grid", "wall", "door", "floor", "room", "text", "dimension", "tag", "sheet", "place"] {
            assert!(ids.contains(id), "{label}: {id}");
        }
        assert!(!ids.contains("generate") && !script.contains("\"path\""), "{label}: a script names no file and runs no generator");
        let mut s = Session::new();
        let replies = s.script(&script).unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(replies.len(), commands);
        assert!(replies.iter().all(|r| r.get("warning").is_none()), "{label}: a room is unbounded");
        let rooms = b.plan.as_ref().unwrap().cells.len() + 1;
        assert_eq!(count(&s, |e| matches!(e, Element::Room(_))), rooms, "{label}");
        assert!(count(&s, |e| matches!(e, Element::Wall(_))) >= rooms, "{label}: walls");
        let exits = b.plan.as_ref().unwrap().openings.iter().filter(|o| matches!(o.role, "back_door" | "service_exit")).count();
        assert!(exits >= 1, "{label}: a way out the back");
        assert_eq!(
            count(&s, |e| matches!(e, Element::Door(_))),
            rooms + exits,
            "{label}: a door for each room but the corridor, the way in, and each exit to outside"
        );
        assert!(count(&s, |e| matches!(e, Element::Window(_))) > 0, "{label}: windows");
        assert_eq!(count(&s, |e| matches!(e, Element::Sheet(_))), 1, "{label}");
        assert_eq!(count(&s, |e| matches!(e, Element::Viewport(_))), 1, "{label}");
        assert!(count(&s, |e| matches!(e, Element::Dimension(_))) >= 3, "{label}");
        assert_eq!(s.model.check(), Ok(()), "{label}");
    }
}

#[test]
fn generated_gridlines_cover_footprint_corridor_and_room_modules() {
    for (name, seed, area) in [("office", 1, 900.0), ("clinic", 9, 650.0), ("hospital", 8, 1800.0)] {
        let b = built(archetype(name), seed, area);
        let script = text(&b, "script.bimcraft.txt");
        let mut xs = BTreeSet::new();
        let mut ys = BTreeSet::new();
        let mut labels = 0;
        for line in script.lines().map(str::trim).filter(|l| l.starts_with("grid ")) {
            let params: Value = serde_json::from_str(line.split_once(' ').unwrap().1).unwrap();
            let start = params["start"].as_array().unwrap();
            let end = params["end"].as_array().unwrap();
            let a = [start[0].as_i64().unwrap(), start[1].as_i64().unwrap()];
            let z = [end[0].as_i64().unwrap(), end[1].as_i64().unwrap()];
            if a[0] == z[0] {
                xs.insert(a[0]);
            } else if a[1] == z[1] {
                ys.insert(a[1]);
            } else {
                panic!("{name} {seed}: diagonal grid {params}");
            }
            labels += usize::from(params["name"].as_str().is_some_and(|n| !n.is_empty()));
        }
        let building = doc(&b, "building-ir.json");
        let fp = &building["footprint"];
        let (width, depth) = (fp["width"].as_i64().unwrap(), fp["depth"].as_i64().unwrap());
        assert!([0, width].into_iter().all(|x| xs.contains(&x)), "{name} {seed}: {xs:?}");
        assert!([0, depth].into_iter().all(|y| ys.contains(&y)), "{name} {seed}: {ys:?}");
        assert!(xs.len() >= 4 && ys.len() >= 4 && labels >= (xs.len() + ys.len()) / 2, "{name} {seed}: {xs:?} {ys:?} labels {labels}");

        let spaces = building["spaces"].as_array().unwrap();
        let corridor = spaces.iter().find(|s| s["source_2d"].as_str().is_some_and(|id| id.contains("corridor"))).unwrap();
        let cb = &corridor["bounds"];
        assert!(ys.contains(&cb["y0"].as_i64().unwrap()) && ys.contains(&cb["y1"].as_i64().unwrap()), "{name} {seed}: {ys:?}");
        let room_x: BTreeSet<i64> =
            spaces.iter().flat_map(|s| [s["bounds"]["x0"].as_i64().unwrap(), s["bounds"]["x1"].as_i64().unwrap()]).collect();
        assert!(room_x.intersection(&xs).count() >= 6.min(room_x.len()), "{name} {seed}: {xs:?} of {room_x:?}");
    }
}

#[test]
fn the_script_is_saved_as_a_project_a_picture_and_a_drawing() {
    for (name, seed, area) in [("office", 1, 900.0), ("clinic", 3, 650.0)] {
        let b = built(archetype(name), seed, area);
        let out = dir(&format!("export-{name}"));
        std::fs::create_dir_all(&out).unwrap();
        let mut s = Session::new();
        s.script(&text(&b, "script.bimcraft.txt")).unwrap();
        for file in ["model.bimcraft", "plan.png", "plan.dxf"] {
            s.execute("save", &json!({"path": out.join(file)})).unwrap_or_else(|e| panic!("{name} {file}: {e}"));
            assert!(std::fs::metadata(out.join(file)).unwrap().len() > 0, "{file}");
        }
        let png = tiny_skia::Pixmap::decode_png(&std::fs::read(out.join("plan.png")).unwrap()).unwrap();
        let first = png.pixels().first().copied();
        let inked = png.pixels().iter().filter(|p| Some(**p) != first).count();
        assert!(inked > png.pixels().len() / 200, "{name}: the picture is blank ({inked} of {} pixels drawn)", png.pixels().len());
        let d = crate::dxf::read(&std::fs::read(out.join("plan.dxf")).unwrap()).unwrap();
        for layer in ["A-WALL", "A-DOOR"] {
            assert!(d.entities.iter().any(|e| e.layer == layer), "{name}: nothing on {layer}");
        }
        // The saved project opens again.
        let mut again = Session::new();
        again.execute("open", &json!({"path": out.join("model.bimcraft")})).unwrap();
        assert_eq!(again.model.elements.len(), s.model.elements.len());
        std::fs::remove_dir_all(&out).unwrap();
    }
}

#[test]
fn the_same_brief_gives_the_same_bytes_and_other_seeds_give_other_plans() {
    for (name, seed, area) in [("office", 1, 900.0), ("clinic", 3, 650.0)] {
        let (a, b) = (built(archetype(name), seed, area), built(archetype(name), seed, area));
        assert_eq!(a.files, b.files);
        assert_eq!(a.topology_hash, b.topology_hash);
    }
    for name in ["office", "clinic", "hospital"] {
        let a = archetype(name);
        let area = match name {
            "office" => 900.0,
            "clinic" => 650.0,
            _ => 1800.0,
        };
        let runs: Vec<Built> = (1..=3).map(|seed| built(a, seed, area)).collect();
        let scripts: BTreeSet<String> = runs.iter().map(|b| text(b, "script.bimcraft.txt")).collect();
        let hashes: BTreeSet<&str> = runs.iter().map(|b| b.topology_hash.as_str()).collect();
        assert_eq!(scripts.len(), 3, "{name}: scripts differ by seed");
        assert_eq!(hashes.len(), 3, "{name}: topologies differ by seed");
        // The seed is in the files it made.
        assert_eq!(doc(&runs[2], "manifest.json")["seed"], 3);
    }
    // A seed is any 64-bit number, written whole.
    let big = built(Archetype::Office, u64::MAX, 900.0);
    assert_eq!(doc(&big, "manifest.json")["seed"].as_u64(), Some(u64::MAX));
}

#[test]
fn the_report_says_what_was_checked_and_what_the_generator_cannot_do() {
    let b = built(Archetype::Clinic, 3, 650.0);
    let r = doc(&b, "report.json");
    assert_eq!((r["archetype"].as_str(), r["seed"].as_u64()), (Some("clinic"), Some(3)));
    let checks = r["checks"].as_array().unwrap();
    let ids: Vec<&str> = checks.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "GR-001", "GR-002", "GR-003", "GR-004", "GR-005", "GR-006", "PR-001", "LIFT-001", "LIFT-002", "LIFT-003", "LIFT-004", "SC-001",
            "SC-002", "RT-001", "RT-002", "RT-003", "LIFT-005", "LIFT-006", "LIFT-007", "GM-001"
        ]
    );
    assert!(checks.iter().all(|c| matches!(c["status"].as_str(), Some("PASS" | "WARN")) && !c["detail"].as_str().unwrap().is_empty()));
    assert_eq!(r["summary"]["fail"], 0);
    assert!(!r["capability_notes"].as_array().unwrap().is_empty());
    assert_eq!(r["rule_set"], report::RULE_SET);
    assert_eq!(r["scores"]["room_count"].as_u64(), Some(b.plan.as_ref().unwrap().cells.len() as u64 + 1));
    let net = r["scores"]["net_to_gross"].as_f64().unwrap();
    assert!((0.5..1.0).contains(&net), "{net}");
    assert_eq!(r["measured_rooms"].as_array().unwrap().len(), b.plan.as_ref().unwrap().cells.len() + 1);
    assert!(r["measured_rooms"].as_array().unwrap().iter().all(|m| m["net_area_m2"].as_f64().unwrap_or(0.0) > 0.0));
    // A plan far from the area asked for says so rather than passing quietly: the depth is held, so the length can't make up.
    let wide = built(Archetype::Office, 1, 5000.0);
    let wide_report = doc(&wide, "report.json");
    let drift: Vec<&str> = wide_report["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert!(drift.iter().all(|w| w.starts_with("GM-001")), "{drift:?}");
}

#[test]
fn opening_symbol_profiles_are_exported_for_training_labels() {
    let b = built(Archetype::Hospital, 3, 1800.0);
    let manifest = doc(&b, "manifest.json");
    let topology = doc(&b, "topology-graph.json");
    let building = doc(&b, "building-ir.json");
    let report = doc(&b, "report.json");
    for doc in [&topology, &building, &report] {
        assert_eq!(doc["opening_symbol_profile"], manifest["opening_symbol_profile"]);
    }
    let edges = topology["edges"].as_array().unwrap();
    assert!(
        edges
            .iter()
            .filter(|e| e["kind"] == "door")
            .all(|e| e["opening_symbol"].as_str().is_some() && e["opening_role"].as_str().is_some()),
        "{edges:#?}"
    );
    let openings = building["openings"].as_array().unwrap();
    let door_symbols: BTreeSet<&str> = openings.iter().filter(|o| o["kind"] == "door").filter_map(|o| o["symbol"].as_str()).collect();
    let window_symbols: BTreeSet<&str> = openings.iter().filter(|o| o["kind"] == "window").filter_map(|o| o["symbol"].as_str()).collect();
    assert!(door_symbols.len() >= 3, "{door_symbols:?}");
    assert!(window_symbols.len() >= 2, "{window_symbols:?}");
    assert!(openings.iter().all(|o| o["width_mm"].as_i64().unwrap_or(0) > 0));
    let script = text(&b, "script.bimcraft.txt");
    assert!(
        script.contains("\ntype ") && script.contains("\"category\":\"door\"") && script.contains("\"category\":\"window\""),
        "{script}"
    );
}

#[test]
fn areas_are_met_closely_across_the_range() {
    for area in [250.0, 1600.0, 5000.0] {
        for name in ["office", "clinic", "hospital"] {
            if let Ok(b) = build(archetype(name), 1, area) {
                let gross = b.plan.as_ref().unwrap().gross_m2();
                assert!((gross - area).abs() / area <= 0.05, "{name} {area}: {gross}");
            }
        }
    }
}

#[test]
fn the_catalog_lists_the_command_with_a_params_doc() {
    let c = crate::engine::find("generate").unwrap();
    for key in ["path", "archetype", "seed", "area_m2", "office", "clinic", "hospital"] {
        assert!(c.params.contains(key), "the params doc names {key}");
    }
}

#[test]
fn a_brief_the_seeds_layout_cannot_fit_is_tried_with_the_other_layouts() {
    // Office seed 1 at 180 m2 was refused for its own family and strategy; another fits it, and what is met today is unchanged.
    let a = archetype("office");
    let built = build(a, 1, 180.0).unwrap_or_else(|e| panic!("{e}"));
    assert!(built.gross_m2 > 150.0);
    assert!(build(a, 1, 40.0).is_err(), "a brief no layout fits is still refused");
}

#[test]
fn every_archetypes_script_says_its_building_type_as_a_drawing_property() {
    for (archetype, seed, area, kind) in [
        (Archetype::Office, 1, 900.0, "office"),
        (Archetype::Clinic, 3, 650.0, "clinic"),
        (Archetype::Hospital, 10, 1800.0, "hospital"),
        (Archetype::House, 3, 115.0, "residential"),
        (Archetype::StudioOffice, 3, 260.0, "office"),
        (Archetype::OfficePattern, 1, 300.0, "office"),
        (Archetype::MedicalDay, 1, 780.0, "hospital"),
    ] {
        let b = built(archetype, seed, area);
        let script = text(&b, "script.bimcraft.txt");
        let line = format!("drawing_property {}", json!({"name": "building_type", "value": kind}));
        assert_eq!(
            script.lines().filter(|l| l.starts_with("drawing_property ")).collect::<Vec<_>>(),
            [line.as_str()],
            "{}",
            archetype.name()
        );
        let mut s = Session::new();
        s.script(&script).unwrap_or_else(|e| panic!("{}: {e}", archetype.name()));
        assert_eq!(s.model.drawing_properties.get("building_type").map(String::as_str), Some(kind));
        let level = s.model.levels()[0].0;
        let dxf = crate::render::dxf(&crate::render::plan(&s.model, level, 1600, 1000), &s.model);
        assert!(
            dxf.contains(&format!("$CUSTOMPROPERTYTAG\n  1\nbuilding_type\n  9\n$CUSTOMPROPERTY\n  1\n{kind}\n")),
            "{}",
            archetype.name()
        );
    }
}
