//! The shape graph engine through its output: the files `generate --rooms` writes, and what BIMCraft reads back from the script.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::family::{Conn, Family, HOUSE_FAMILY, shapes};
use super::{ATTEMPTS, Request, build};
use crate::engine::Session;
use crate::graph_rules::Pack;

fn request(seed: u64, rooms: i64, tol: i64, shape: Option<&str>) -> Request {
    Request { archetype: "house".into(), seed, rooms, tolerance: tol, shape: shape.map(String::from), gridlines: false, interior: super::Interior::Aligned, bumps: false }
}

fn file(files: &[(&'static str, String)], name: &str) -> Value {
    let text = files.iter().find(|(n, _)| *n == name).map(|(_, t)| t.as_str()).unwrap_or("null");
    serde_json::from_str(text).unwrap_or(Value::Null)
}

/// The corners of the one loop the 250 mm walls of a script make, as a test reads it: from the script alone.
fn outline_corners(script: &str) -> usize {
    let mut segs: Vec<([i64; 2], [i64; 2])> = Vec::new();
    for line in script.lines().filter(|l| l.starts_with("wall ")) {
        let v: Value = serde_json::from_str(&line[5..]).unwrap_or(Value::Null);
        if v["type"] != "Exterior - 250mm" {
            continue;
        }
        let p = |i: usize| [v["points"][i][0].as_i64().unwrap_or(0), v["points"][i][1].as_i64().unwrap_or(0)];
        segs.push((p(0), p(1)));
    }
    let rects: Vec<super::links::Rc> = Vec::new();
    let _ = rects;
    let mut adj: std::collections::BTreeMap<[i64; 2], Vec<[i64; 2]>> = std::collections::BTreeMap::new();
    for (a, b) in &segs {
        adj.entry(*a).or_default().push(*b);
        adj.entry(*b).or_default().push(*a);
    }
    // Split points lying along a wall are corners only where the direction changes.
    let start = *adj.keys().next().unwrap_or(&[0, 0]);
    let (mut prev, mut here) = (start, adj[&start][0]);
    let mut path = vec![start];
    while here != start && path.len() <= adj.len() {
        path.push(here);
        let next = adj[&here].iter().copied().find(|n| *n != prev).unwrap_or(start);
        prev = here;
        here = next;
    }
    let n = path.len();
    (0..n)
        .filter(|&i| {
            let (a, b, c) = (path[(i + n - 1) % n], path[i], path[(i + 1) % n]);
            (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) != 0
        })
        .count()
}

#[test]
fn the_house_family_file_reads_in_order_and_stays_inside_its_pack() {
    let fam = Family::parse(HOUSE_FAMILY).expect("the house family parses");
    assert_eq!(fam.types.len(), 13);
    assert_eq!(fam.types[0].place, super::family::Place::EntryEnd, "the file's order is the engine's order");
    assert_eq!(fam.types[3].place, super::family::Place::Spine);
    assert_eq!(fam.count_limits(), (5, 30));
    assert_eq!(fam.pack_id.as_deref(), Some("house"));
}

#[test]
fn a_family_that_widens_its_pack_or_breaks_its_schema_is_refused() {
    let widen = HOUSE_FAMILY.replace("\"area_m2\": [7.0, 25.0]", "\"area_m2\": [7.0, 250.0]");
    assert!(Family::parse(&widen).is_err_and(|e| e.contains("wider")), "a bedroom bigger than the pack's");
    let alien = HOUSE_FAMILY.replace("\"count\": [1, 6]", "\"count\": [1, 60]");
    assert!(Family::parse(&alien).is_err(), "more bedrooms than the pack allows");
    let typo = HOUSE_FAMILY.replace("\"min_side_mm\": 3000", "\"min_side\": 3000");
    assert!(Family::parse(&typo).is_err(), "a misspelled key is an error");
    let report = HOUSE_FAMILY.replace("\"building_type\": \"residential\"", "\"building_type\": \"dwelling\"");
    assert!(Family::parse(&report).is_err_and(|e| e.contains("building_type")));
    let means = HOUSE_FAMILY.replace(
        "{\"from\": \"hall\", \"to\": \"garage\", \"use\": \"parent\", \"types\": [\"single\", \"double\"]}",
        "{\"from\": \"hall\", \"to\": \"garage\", \"use\": \"parent\", \"types\": [\"bifold\"]}",
    );
    assert!(Family::parse(&means).is_err_and(|e| e.contains("pack")), "a means the pack doesn't list for the pair");
}

#[test]
fn residential_kinds_use_the_bracket_form() {
    let kinds = [
        ("residential-single-family", "residential (single family)"),
        ("residential-duplex", "residential (duplex)"),
        ("residential-townhouse", "residential (townhouse)"),
        ("residential-multifamily-low-rise", "residential (multifamily low-rise)"),
        ("residential-multifamily-mid-rise", "residential (multifamily mid-rise)"),
        ("residential-multifamily-high-rise", "residential (multifamily high-rise)"),
    ];
    assert_eq!(super::family::RESIDENTIAL_KINDS, kinds);
    for (family, building_type) in kinds {
        assert!(super::family::building_type_ok(building_type), "{building_type}");
        assert!(!super::family::building_type_ok(family), "{family} is a family name, not a building type");
    }
}

#[test]
fn the_catalogue_has_nine_shapes_with_their_corners_levels_and_raises() {
    let all = shapes().expect("the catalogue reads");
    let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["bar", "narrow_bar", "l_shape", "corner_notch", "stepped", "t_shape", "side_nub", "opposite_wings", "court"]);
    let corners: Vec<usize> = all.iter().map(|s| s.corners).collect();
    assert_eq!(corners, [4, 4, 6, 6, 8, 8, 8, 8, 8]);
    let court = &all[8];
    assert_eq!((court.levels.a.clone(), court.levels.b.clone()), (vec![1, 0, 1], vec![0, 0, 0]));
    assert_eq!(court.raise, Some([0.45, 1.3]));
}

#[test]
fn the_engine_quotes_no_room_kind() {
    let pack = Pack::embedded("house").expect("pack");
    let sources = [
        include_str!("../shape_graph.rs"),
        include_str!("emit.rs"),
        include_str!("family.rs"),
        include_str!("fit.rs"),
        include_str!("layout.rs"),
        include_str!("links.rs"),
        include_str!("plan.rs"),
        include_str!("tests.rs"),
    ];
    for kind in pack.kinds.keys() {
        let quoted = format!("\"{kind}\"");
        for (i, text) in sources.iter().enumerate() {
            assert!(!text.contains(&quoted), "source {i} quotes the kind {kind}");
        }
    }
}

#[test]
fn a_house_by_rooms_writes_six_files_that_agree() {
    let built = build(&request(3, 12, 0, None), false).expect("a plan");
    let names: Vec<&str> = built.files.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        ["program-graph.json", "topology-graph.json", "building-ir.json", "script.bimcraft.txt", "report.json", "manifest.json"]
    );
    let manifest = file(&built.files, "manifest.json");
    assert_eq!(manifest["engine"], "shape-graph");
    assert_eq!(manifest["generator_version"], "shape-graph/1");
    assert_eq!(manifest["family"], "house");
    assert_eq!(manifest["rooms"], json!({"requested": 12, "tolerance": 0, "actual": 12}));
    assert!(manifest.get("requested_area").is_none() && manifest.get("requested_area_m2").is_none());
    let graph = file(&built.files, "topology-graph.json");
    assert_eq!(graph["shell"], manifest["shape"]);
    let numbers: Vec<String> =
        graph["nodes"].as_array().expect("nodes").iter().map(|n| n["number"].as_str().unwrap_or("").to_string()).collect();
    assert_eq!(numbers, (101..113).map(|n| n.to_string()).collect::<Vec<_>>());
    let nodes = graph["nodes"].as_array().expect("nodes");
    assert!(nodes.iter().all(|n| n["room_type"].is_string() && n["name"].is_string()));
    assert!(graph["edges"].as_array().expect("edges").iter().all(|e| e["connection"].is_string()));
    let report = file(&built.files, "report.json");
    let ids: BTreeSet<String> =
        report["checks"].as_array().expect("checks").iter().filter_map(|c| c["id"].as_str().map(String::from)).collect();
    for i in 1..=13 {
        assert!(ids.contains(&format!("HR-{i:03}")), "a check for HR-{i:03}");
    }
    let script = built.files.iter().find(|(n, _)| *n == "script.bimcraft.txt").map(|(_, t)| t.clone()).unwrap_or_default();
    assert!(script.contains("\"at\":\"finish\""));
    assert!(
        !script.contains("\"command\":\"grid\"")
            && !script.lines().any(|l| l.starts_with("grid ") || l.starts_with("text ") || l.starts_with("tag "))
    );
    assert!(!script.contains("/Users") && !script.contains("/tmp") && !script.contains("/private"), "no path in the script");
}

#[test]
fn the_script_runs_and_the_extracted_graph_has_the_generated_rooms() {
    let built = build(&request(5, 10, 0, None), false).expect("a plan");
    let script = built.files.iter().find(|(n, _)| *n == "script.bimcraft.txt").map(|(_, t)| t.clone()).unwrap_or_default();
    let mut session = Session::new();
    session.script(&script).expect("the script runs");
    let topology = crate::topology::extract(&session.model);
    assert_eq!(topology["nodes"].as_array().map_or(0, Vec::len), 10);
    assert!(topology["diagnostics"].as_array().is_none_or(Vec::is_empty));
}

#[test]
fn every_rooms_script_names_the_delivery_profile_once_after_the_building_type() {
    for (seed, rooms) in [(5, 10), (6, 18)] {
        let built = build(&request(seed, rooms, 0, None), false).expect("a plan");
        let script = built.files.iter().find(|(n, _)| *n == "script.bimcraft.txt").map(|(_, t)| t.clone()).unwrap_or_default();
        let lines: Vec<&str> = script.lines().collect();
        assert_eq!(lines.iter().filter(|l| l.starts_with("cad_profile ")).count(), 1);
        let at = lines.iter().position(|l| l.starts_with("drawing_property ")).expect("the building type");
        assert_eq!(lines[at + 1], r#"cad_profile {"name":"delivery"}"#);
        let mut s = Session::new();
        s.script(&script).expect("runs");
        assert_eq!(s.model.cad_profile, Some(crate::model::CadProfile::Delivery));
    }
}

#[test]
fn the_same_arguments_write_the_same_bytes() {
    let a = build(&request(9, 14, 2, None), false).expect("a plan");
    let b = build(&request(9, 14, 2, None), false).expect("a plan");
    assert_eq!(a.files, b.files);
    let c = build(&request(10, 14, 2, None), false).expect("a plan");
    assert_ne!(a.topology_hash, c.topology_hash);
}

#[test]
fn a_pinned_shape_is_the_outline_of_the_plan() {
    for (shape, corners) in [("l_shape", 6), ("corner_notch", 6), ("t_shape", 8), ("court", 8), ("narrow_bar", 4)] {
        let built = build(&request(2, 12, 0, Some(shape)), false).unwrap_or_else(|e| panic!("{shape}: {e}"));
        assert_eq!(file(&built.files, "manifest.json")["shape"], shape);
        let script = built.files.iter().find(|(n, _)| *n == "script.bimcraft.txt").map(|(_, t)| t.clone()).unwrap_or_default();
        assert_eq!(outline_corners(&script), corners, "{shape}");
    }
}

#[test]
fn the_number_of_rooms_is_asked_for_with_a_tolerance_and_the_seed_chooses() {
    let mut seen = BTreeSet::new();
    for seed in 1..=3 {
        let built = build(&request(seed, 12, 3, None), false).expect("a plan");
        assert!((9..=15).contains(&built.rooms));
        seen.insert(built.rooms);
    }
    assert!(seen.len() >= 2, "{seen:?}");
}

#[test]
fn a_call_is_refused_with_its_code() {
    let code = |r: Result<crate::generate::Built, crate::generate::Refusal>| r.err().map(|e| e.code);
    assert_eq!(code(build(&request(1, 2, 0, None), false)), Some("unsatisfiable_rooms"));
    assert_eq!(code(build(&request(1, 12, 0, Some("blob")), false)), Some("unknown_shape"));
    let mut other = request(1, 12, 0, None);
    other.archetype = "office".into();
    assert_eq!(code(build(&other, false)), Some("unknown_archetype"));
    let e = build(&request(1, 2, 0, None), false).err().expect("refused");
    assert!(e.message.contains('5') && e.message.contains("30"), "{}", e.message);
}

#[test]
fn a_bar_cannot_hold_a_large_program() {
    let e = build(&request(1, 28, 0, Some("bar")), false).err().expect("too many rooms for one spine");
    assert_eq!(e.code, "unplaceable_shape");
}

#[test]
fn the_attempts_are_few_enough_for_a_second() {
    assert_eq!(ATTEMPTS, 400);
    let start = std::time::Instant::now();
    let _ = build(&request(4, 24, 0, None), false);
    // Tests run in parallel on a build with debug assertions; the release CLI is held to one second by the done check.
    assert!(start.elapsed().as_secs_f64() < 5.0, "{:?}", start.elapsed());
}

/// How many entities the exported DXF of a plan holds on a layer.
fn dxf_layer(script: &str, layer: &str, name: &str) -> usize {
    let mut session = Session::new();
    session.script(script).expect("the script runs");
    let path = std::env::temp_dir().join(format!("shape-graph-{name}.dxf"));
    session.execute("save", &json!({"path": path.to_string_lossy()})).expect("saves a DXF");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut in_entities = false;
    let mut n = 0;
    for pair in lines.chunks(2) {
        if pair.len() < 2 {
            break;
        }
        if pair[0] == "2" && pair[1] == "ENTITIES" {
            in_entities = true;
        }
        if pair[0] == "0" && pair[1] == "ENDSEC" {
            in_entities = false;
        }
        if in_entities && pair[0] == "8" && pair[1] == layer {
            n += 1;
        }
    }
    n
}

#[test]
fn every_plans_dxf_has_its_walls_on_the_wall_layer() {
    // A material whose cut is a solid pattern lays paper over the wall lines, and a plan then has none in its DXF.
    for (rooms, seed) in [(5, 5), (5, 9), (8, 5), (5, 11), (8, 20), (12, 26)] {
        let built = build(&request(seed, rooms, 2, None), false).expect("a plan");
        let script = built.files.iter().find(|(n, _)| *n == "script.bimcraft.txt").map(|(_, t)| t.clone()).unwrap_or_default();
        assert!(dxf_layer(&script, "A-WALL", &format!("{rooms}-{seed}")) > 0, "{rooms} rooms, seed {seed}");
    }
}

#[test]
fn the_script_saves_the_same_project_every_time() {
    let built = build(&request(1, 8, 0, None), false).expect("a plan");
    let script = built.files.iter().find(|(n, _)| *n == "script.bimcraft.txt").map(|(_, t)| t.clone()).unwrap_or_default();
    let mut texts = Vec::new();
    for k in 0..2 {
        let mut session = Session::new();
        session.script(&script).expect("runs");
        let path = std::env::temp_dir().join(format!("shape-graph-model-{k}.bimcraft"));
        session.execute("save", &json!({"path": path.to_string_lossy()})).expect("saves");
        texts.push(std::fs::read_to_string(&path).unwrap_or_default());
        let _ = std::fs::remove_file(&path);
    }
    assert!(!texts[0].is_empty() && texts[0] == texts[1], "a seeded project's uid and GUIDs are the seed's");
}

#[test]
fn new_takes_a_whole_seed_and_a_seeded_project_is_reproducible() {
    let mut s = Session::new();
    for bad in [json!({"seed": -1}), json!({"seed": 1.5}), json!({"seed": "x"}), json!({"seed": 1e300})] {
        assert!(s.execute("new", &bad).is_err(), "{bad}");
    }
    let uid = |s: &Session| s.model.uid;
    s.execute("new", &json!({"seed": 7})).expect("seeded");
    let first = uid(&s);
    s.execute("new", &json!({"seed": 7})).expect("seeded");
    assert_eq!(first, uid(&s));
    s.execute("new", &json!({})).expect("random again");
    assert_ne!(first, uid(&s));
}

#[test]
fn a_scratch_project_leaves_the_callers_guids_random() {
    let _ = build(&request(1, 8, 0, None), false).expect("a plan");
    let (a, b) = (crate::ids::Guid::new(), crate::ids::Guid::new());
    assert_ne!(a, b, "still random");
}

#[test]
fn connection_types_know_their_door() {
    assert_eq!(Conn::Single.door(), Some(("Single Door", "swing")));
    assert_eq!(Conn::OpenFull.door(), None);
    assert!(Conn::OpenFull.is_open() && !Conn::Single.is_open());
}
