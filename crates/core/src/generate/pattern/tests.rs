//! The office-pattern generator through its output: the files it writes, what the topology extractor reads from the project those
//! files build, and the pattern judged on that extracted plan the way the done check judges it (a port of its geometry rules).

use std::collections::BTreeSet;

use serde_json::Value;

use super::super::*;
use super::place::{self, Case, assemble, draw, graph_of};
use super::{Kind, LEAST_AREA};
use crate::graph_rules::{self, Graph as RuleGraph, Pack};

fn pattern(seed: u64, area: f64) -> Result<Built, Refusal> {
    build(Archetype::OfficePattern, seed, area)
}

fn file<'a>(b: &'a Built, name: &str) -> &'a str {
    &b.files.iter().find(|(n, _)| *n == name).unwrap().1
}

fn doc(b: &Built, name: &str) -> Value {
    serde_json::from_str(file(b, name)).unwrap()
}

/// The 20 briefs of the done check: seeds 1 to 20 over 200, 300, 400, 500 and 600 m2.
/// Double loaded and square, double loaded and chamfered with focus rooms, and the single-loaded one.
fn briefs() -> impl Iterator<Item = (u64, f64)> {
    [1_u64, 5, 12].into_iter().map(|s| (s, [200.0, 300.0, 400.0, 500.0, 600.0][(s as usize - 1) % 5]))
}

// ---- the role of a room, from its name and then its kind, in the order the done check tries them

type Test = fn(&str) -> bool;

const ROLES: [(&str, Test); 8] = [
    ("open_office", |v| {
        ["open office", "open_office", "openoffice", "open plan", "open_plan", "open work", "open_work", "workstation"]
            .iter()
            .any(|p| v.contains(p))
    }),
    ("lobby", |v| ["lobby", "reception", "foyer", "entry", "entrance"].iter().any(|p| v.contains(p))),
    ("corridor", |v| v.contains("corridor") || v == "hall" || v == "hallway" || v.contains("circulation")),
    ("focus", |v| ["focus", "quiet", "phone"].iter().any(|p| v.contains(p))),
    ("conference", |v| ["conference", "meeting", "board"].iter().any(|p| v.contains(p))),
    ("restroom", |v| ["restroom", "toilet", "wc", "bath", "washroom", "lavatory"].iter().any(|p| v.contains(p))),
    ("pantry", |v| ["pantry", "kitchen", "break", "tea"].iter().any(|p| v.contains(p))),
    ("private_office", |v| {
        v.contains("private")
            || v == "office"
            || (v.starts_with("office") && v[6..].starts_with([' ', '_']) && v[7..].starts_with(|c: char| c.is_ascii_digit()))
    }),
];

fn role_of(text: &str) -> Option<&'static str> {
    let v = text.trim().to_lowercase().replace('-', "_");
    ROLES.iter().find(|(_, f)| !v.is_empty() && f(&v)).map(|(r, _)| *r)
}

fn role(n: &Value) -> Option<&'static str> {
    ["name", "kind"].iter().find_map(|f| role_of(n[*f].as_str().unwrap_or("")))
}

#[test]
fn every_label_the_generator_uses_reads_as_its_own_role() {
    for (label, want) in [
        ("Lobby", "lobby"),
        ("Corridor", "corridor"),
        ("Open Office", "open_office"),
        ("Private Office 1", "private_office"),
        ("Private Office 12", "private_office"),
        ("Conference 1", "conference"),
        ("Restroom 3", "restroom"),
        ("Pantry 2", "pantry"),
        ("Focus Room 1", "focus"),
    ] {
        assert_eq!(role_of(label), Some(want), "{label}");
    }
    // The generator's labels are the pack's kinds' labels, so a plan's names say its kinds.
    let g = graph_of(&Case {
        south_d: 4000,
        north_d: 4000,
        corr: 2000,
        lobby_w: 4200,
        open_w: 6400,
        run: 12000,
        south: vec![(Kind::PrivateOffice, 3000), (Kind::Conference, 3000), (Kind::Restroom, 3000), (Kind::Pantry, 3000)],
        north: vec![(Kind::PrivateOffice, 4000), (Kind::FocusRoom, 4000), (Kind::Conference, 4000)],
        c_sw: 0,
        c_ne: 0,
    });
    let want = |k: Kind| match k {
        Kind::Lobby => "lobby",
        Kind::Hall => "corridor",
        Kind::OpenOffice => "open_office",
        Kind::PrivateOffice => "private_office",
        Kind::Conference => "conference",
        Kind::Restroom => "restroom",
        Kind::Pantry => "pantry",
        Kind::FocusRoom => "focus",
    };
    for r in &g.rooms {
        assert_eq!(role_of(&r.name), Some(want(r.kind)), "{}", r.name);
    }
    let ids: Vec<&str> = g.rooms.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "lobby",
            "corridor",
            "open_office",
            "private_office_1",
            "conference_1",
            "restroom_1",
            "pantry_1",
            "private_office_2",
            "focus_room_1",
            "conference_2"
        ]
    );
}

// ---- the pattern's geometry rules, on the extracted plan

fn pts(n: &Value) -> Vec<[f64; 2]> {
    n["boundary"].as_array().into_iter().flatten().filter_map(|p| Some([p[0].as_f64()?, p[1].as_f64()?])).collect()
}

fn centroid(p: &[[f64; 2]]) -> [f64; 2] {
    let n = p.len() as f64;
    [p.iter().map(|q| q[0]).sum::<f64>() / n, p.iter().map(|q| q[1]).sum::<f64>() / n]
}

fn proj(p: &[[f64; 2]], u: [f64; 2]) -> (f64, f64) {
    let v: Vec<f64> = p.iter().map(|q| q[0] * u[0] + q[1] * u[1]).collect();
    (v.iter().cloned().fold(f64::INFINITY, f64::min), v.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
}

/// The unit vector along the longest side of the corridor's minimum-area rectangle.
fn corridor_axis(p: &[[f64; 2]]) -> Option<[f64; 2]> {
    let mut best: Option<(f64, [f64; 2])> = None;
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        if l < 1e-6 {
            continue;
        }
        let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
        let v = [-u[1], u[0]];
        let (x, y) = (proj(p, u), proj(p, v));
        let (w, h) = (x.1 - x.0, y.1 - y.0);
        if best.is_none_or(|b| w * h < b.0) {
            best = Some((w * h, if w >= h { u } else { v }));
        }
    }
    best.map(|b| b.1)
}

/// Footprint corners with collinear points dropped.
fn outline_vertices(ext: &Value) -> usize {
    let p: Vec<[f64; 2]> =
        ext["footprint"]["outline"].as_array().into_iter().flatten().filter_map(|p| Some([p[0].as_f64()?, p[1].as_f64()?])).collect();
    (0..p.len())
        .filter(|&i| {
            let (a, b, c) = (p[(i + p.len() - 1) % p.len()], p[i], p[(i + 1) % p.len()]);
            let (ab, bc) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
            let (la, lb) = (ab[0].hypot(ab[1]), bc[0].hypot(bc[1]));
            la >= 1.0 && lb >= 1.0 && (ab[0] * bc[1] - ab[1] * bc[0]).abs() / (la * lb) > 1e-3
        })
        .count()
}

struct Facts {
    rooms: usize,
    focus: usize,
    double: bool,
    chamfered: bool,
}

/// What the done check says of an extracted plan whose rooms carry the generator's names: the problems, or the facts.
fn judge(ext: &Value) -> Result<Facts, Vec<String>> {
    let nodes: Vec<&Value> = ext["nodes"].as_array().into_iter().flatten().collect();
    let id = |n: &Value| n["id"].as_str().unwrap_or("").to_string();
    let roles: Vec<(String, Option<&str>)> = nodes.iter().map(|n| (id(n), role(n))).collect();
    let by = |r: &str| -> Vec<String> { roles.iter().filter(|(_, x)| *x == Some(r)).map(|(i, _)| i.clone()).collect() };
    let mut bad = Vec::new();
    let (lobby, corr, open) = (by("lobby"), by("corridor"), by("open_office"));
    if let Some((i, _)) = roles.iter().find(|(_, r)| r.is_none()) {
        bad.push(format!("`{i}` has no role"));
    }
    if lobby.len() != 1 || corr.len() != 1 || open.len() != 1 {
        bad.push(format!("{} lobbies, {} corridors, {} open offices", lobby.len(), corr.len(), open.len()));
        return Err(bad);
    }
    // Door and open links, and the exterior doors.
    let edges: Vec<&Value> = ext["edges"].as_array().into_iter().flatten().collect();
    let exterior = edges.iter().filter(|e| e["kind"] == "door" && e["exterior"] == true).count();
    let link = |a: &str, b: &str, door: bool| {
        edges.iter().any(|e| {
            e["exterior"] != true
                && (if door { e["kind"] == "door" } else { e["kind"] == "open" })
                && ((e["from"] == a && e["to"] == b) || (e["from"] == b && e["to"] == a))
        })
    };
    let (l, c, o) = (&lobby[0], &corr[0], &open[0]);
    if exterior != 1 || !edges.iter().any(|e| e["exterior"] == true && (e["from"] == l.as_str() || e["to"] == l.as_str())) {
        bad.push(format!("{exterior} exterior doors, one on the lobby wanted"));
    }
    if ext["entry"]["room"] != l.as_str() {
        bad.push("the entry is not the lobby".into());
    }
    if !link(l, c, true) && !link(l, c, false) {
        bad.push("the lobby does not join the corridor".into());
    }
    let side: Vec<&(String, Option<&str>)> =
        roles.iter().filter(|(_, r)| !matches!(r, Some("lobby" | "corridor" | "open_office"))).collect();
    for (i, _) in &side {
        if !link(i, c, true) {
            bad.push(format!("`{i}` has no door to the corridor"));
        }
        if link(i, o, true) || link(i, o, false) {
            bad.push(format!("`{i}` opens off the open office"));
        }
    }
    for r in ["private_office", "conference", "restroom", "pantry"] {
        if by(r).is_empty() {
            bad.push(format!("no {r}"));
        }
    }
    if !link(o, c, true) && !link(o, c, false) {
        bad.push("the open office does not join the corridor".into());
    }
    if link(o, l, true) || link(o, l, false) {
        bad.push("the open office joins the lobby".into());
    }
    // Geometry.
    let geo: std::collections::BTreeMap<String, Vec<[f64; 2]>> = nodes.iter().map(|n| (id(n), pts(n))).collect();
    if geo.values().any(|p| p.len() < 3) {
        bad.push("a room has no boundary".into());
        return Err(bad);
    }
    let (co, cl) = (centroid(&geo[o]), centroid(&geo[l]));
    let d = (co[0] - cl[0]).hypot(co[1] - cl[1]);
    let lo = [(co[0] - cl[0]) / d, (co[1] - cl[1]) / d];
    let ax = corridor_axis(&geo[c]).unwrap_or(lo);
    let u = if ax[0] * lo[0] + ax[1] * lo[1] >= 0.0 { ax } else { [-ax[0], -ax[1]] };
    let v = [-u[1], u[0]];
    let all: Vec<[f64; 2]> = geo.values().flatten().copied().collect();
    let others: Vec<&String> = geo.keys().filter(|k| *k != o && *k != l).collect();
    let tol = 300.0;
    if others.iter().any(|k| proj(&geo[*k], u).1 > proj(&geo[o], u).1 - tol) {
        bad.push("rooms reach past the open office along the plan".into());
    }
    if others.iter().any(|k| proj(&geo[*k], u).0 < proj(&geo[l], u).0 + tol) {
        bad.push("a room starts at the lobby's end".into());
    }
    let (ov, av) = (proj(&geo[o], v), proj(&all, v));
    let depth = (ov.1 - ov.0) / (av.1 - av.0).max(1e-6);
    if depth < 0.9 {
        bad.push(format!("the open office is {depth:.2} of the plan's depth"));
    }
    if u[0] * lo[0] + u[1] * lo[1] < 0.7 {
        bad.push("the corridor does not run from the lobby to the open office".into());
    }
    let cc = centroid(&geo[c]);
    let sides: BTreeSet<bool> = side
        .iter()
        .map(|(i, _)| {
            let m = centroid(&geo[i]);
            (m[0] - cc[0]) * v[0] + (m[1] - cc[1]) * v[1] > 0.0
        })
        .collect();
    let vs = outline_vertices(ext);
    if vs == 0 {
        bad.push("no footprint outline".into());
    }
    if bad.is_empty() {
        Ok(Facts { rooms: nodes.len(), focus: by("focus").len(), double: sides.len() == 2, chamfered: vs > 4 })
    } else {
        Err(bad)
    }
}

fn shoelace(poly: &[Value]) -> f64 {
    let p: Vec<[f64; 2]> = poly.iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()]).collect();
    (0..p.len()).map(|i| p[i][0] * p[(i + 1) % p.len()][1] - p[(i + 1) % p.len()][0] * p[i][1]).sum::<f64>() / 2.0
}

// ---- the files

#[test]
fn the_archetype_is_its_own_and_has_the_standard_files() {
    assert_eq!(Archetype::parse("office-pattern"), Some(Archetype::OfficePattern));
    assert!(Archetype::names().contains("office-pattern"));
    let b = pattern(1, 300.0).unwrap();
    let names: Vec<&str> = b.files.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        ["program-graph.json", "topology-graph.json", "building-ir.json", "script.bimcraft.txt", "report.json", "manifest.json"]
    );
    assert_eq!(doc(&b, "manifest.json")["archetype"], "office-pattern");
    assert_eq!(doc(&b, "topology-graph.json")["archetype"], "office-pattern");
    assert!(b.checks.iter().all(|c| c.status == report::Status::Pass), "{:?}", b.checks);
    let script = file(&b, "script.bimcraft.txt");
    let props: Vec<&str> = script.lines().filter(|l| l.starts_with("drawing_property ")).collect();
    assert_eq!(props, [r#"drawing_property {"name":"building_type","value":"office"}"#]);
    assert!(script.lines().filter(|l| l.starts_with("grid ")).count() >= 4);
}

#[test]
fn seeds_build_read_back_and_keep_the_pattern() {
    let pack = Pack::embedded("office-pattern").unwrap();
    let (mut hashes, mut counts) = (BTreeSet::new(), BTreeSet::new());
    let (mut single, mut double, mut focus, mut no_focus, mut chamfered, mut square) = (0, 0, 0, 0, 0, 0);
    let mut by_area: std::collections::BTreeMap<u64, Vec<f64>> = Default::default();
    for (seed, area) in briefs() {
        let b = pattern(seed, area).unwrap_or_else(|e| panic!("seed {seed} at {area}: {e}"));
        hashes.insert(b.topology_hash.clone());
        let graph = doc(&b, "topology-graph.json");
        let checks = graph_rules::check(pack, &RuleGraph::from_value(&graph).unwrap()).unwrap();
        assert!(checks.len() >= 5 && checks.iter().all(|c| c.status == graph_rules::Status::Pass), "seed {seed}: {checks:?}");
        assert!((b.gross_m2 - area).abs() / area < 0.06, "seed {seed}: {} m2 for {area}", b.gross_m2);
        let ir = doc(&b, "building-ir.json");
        let poly = ir["footprint"]["boundary"].as_array().unwrap();
        assert!(
            (shoelace(poly) / 1.0e6 - b.gross_m2).abs() < 0.01,
            "seed {seed}: polygon {} m2, gross {}",
            shoelace(poly) / 1.0e6,
            b.gross_m2
        );
        // Build the script as the done check does and read the plan back.
        let mut s = Session::new();
        s.script(file(&b, "script.bimcraft.txt")).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let ext = crate::topology::extract(&s.model);
        let facts = judge(&ext).unwrap_or_else(|e| panic!("seed {seed} at {area}: {e:?}"));
        counts.insert(facts.rooms);
        if facts.double {
            double += 1
        } else {
            single += 1
        }
        if facts.focus > 0 {
            focus += 1
        } else {
            no_focus += 1
        }
        if facts.chamfered {
            chamfered += 1
        } else {
            square += 1
        }
        let m = doc(&b, "manifest.json");
        assert_eq!(m["chamfers"].as_u64().unwrap() > 0, facts.chamfered, "seed {seed}: the manifest and the outline disagree");
        assert_eq!(m["shear"], 0);
        by_area.entry(area as u64).or_default().push(ext["footprint"]["area_m2"].as_f64().unwrap());
    }
    assert_eq!(hashes.len(), 3, "each brief has its own topology");
    assert!(counts.len() >= 2, "{counts:?} room counts");
    assert!(single > 0 && double > 0, "{single} single, {double} double loaded");
    assert!(focus > 0 && no_focus > 0, "{focus} with a focus room, {no_focus} without");
    assert!(chamfered > 0 && square > 0, "{chamfered} chamfered, {square} square");
    let mean = |a: u64| by_area[&a].iter().sum::<f64>() / by_area[&a].len() as f64;
    let means: Vec<f64> = by_area.keys().map(|a| mean(*a)).collect();
    assert!(means.windows(2).all(|w| w[1] > w[0]), "{means:?}");
    assert!(mean(600) >= 2.0 * mean(200), "{} against {}", mean(600), mean(200));
}

#[test]
fn the_same_brief_gives_the_same_bytes() {
    let (a, b) = (pattern(1, 200.0).unwrap(), pattern(1, 200.0).unwrap());
    assert_eq!(a.files, b.files);
    assert_ne!(pattern(2, 200.0).unwrap().topology_hash, a.topology_hash);
}

#[test]
fn a_brief_below_the_least_area_is_refused_and_writes_nothing() {
    let e = pattern(1, 20.0).err().unwrap();
    assert_eq!(e.code, "unsatisfiable_area");
    assert!(e.message.contains("170"), "{}", e.message);
    assert_eq!(pattern(1, f64::NAN).err().unwrap().code, "invalid_area");
    let dir = std::env::temp_dir().join(format!("bimcraft-pattern-refused-{}", std::process::id()));
    let mut s = Session::new();
    let e = generate(&mut s, &serde_json::json!({"path": dir, "archetype": "office-pattern", "seed": 1, "area_m2": 20.0})).unwrap_err();
    assert!(e.starts_with("unsatisfiable_area"), "{e}");
    assert!(!dir.exists());
}

#[test]
fn the_least_area_is_met_by_every_seed_and_a_brief_never_panics() {
    for seed in 0..4 {
        let b = pattern(seed, LEAST_AREA).unwrap_or_else(|e| panic!("seed {seed} at the least area: {e}"));
        assert!(b.rooms >= 7);
    }
    // Between the least area and the largest, a brief is met or refused with a typed error, never a panic.
    for area in [171.0, 175.0, 777.7, 1500.0, 3000.0, MAX_AREA_M2] {
        match pattern(3, area) {
            Ok(b) => assert!(b.rooms >= 7),
            Err(e) => assert!(matches!(e.code, "unplaceable_graph" | "unsatisfiable_area"), "{e}"),
        }
    }
}

#[test]
fn each_window_sits_on_one_exterior_wall_clear_of_its_ends() {
    for (seed, area) in briefs() {
        let case = (0..place::TRIES).find_map(|t| draw(seed, area, t).ok()).unwrap();
        let g = graph_of(&case);
        let l = assemble(&g, &case, seed).unwrap();
        assert!(!l.windows.is_empty());
        for w in &l.windows {
            let on = l.walls.iter().filter(|x| x.exterior && place::along(x, w.at).is_some_and(|(s, n)| s >= 300 && s <= n - 300)).count();
            assert_eq!(on, 1, "seed {seed}: window at {:?}", w.at);
        }
    }
}

#[test]
fn the_script_draws_each_door_and_window_once_in_the_dxf() {
    // Seed 1 is square and seed 2 chamfered at the sizes the done check runs.
    let mut chamfered = false;
    for seed in [1, 2, 3, 5, 8] {
        let b = pattern(seed, 300.0).unwrap();
        chamfered |= doc(&b, "manifest.json")["chamfers"].as_u64().unwrap() > 0;
        let script = file(&b, "script.bimcraft.txt");
        let doors = script.lines().filter(|l| l.starts_with("door ")).count();
        let windows = script.lines().filter(|l| l.starts_with("window ")).count();
        assert!(doors >= 7 && windows >= 1, "{doors} doors, {windows} windows");
        let dir = std::env::temp_dir().join(format!("bimcraft-pattern-dxf-{}-{seed}", std::process::id()));
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
        assert_eq!(inserts("A-DOOR"), doors, "seed {seed}");
        assert_eq!(inserts("A-GLAZ"), windows, "seed {seed}");
        // Each room's number is on A-AREA-IDEN once.
        let labels = |t: &str| {
            lines
                .windows(2)
                .enumerate()
                .filter(|(i, w)| {
                    w[0] == "1" && w[1] == t && lines[i.saturating_sub(40)..*i].windows(2).any(|p| p[0] == "8" && p[1] == "A-AREA-IDEN")
                })
                .count()
        };
        for r in &doc(&b, "topology-graph.json")["nodes"].as_array().unwrap().clone() {
            assert_eq!(labels(r["id"].as_str().unwrap()), 1, "seed {seed}: {}", r["id"]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(chamfered, "no chamfered plan among the seeds");
}
