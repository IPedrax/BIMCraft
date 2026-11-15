//! A studio-office layout as files: the script it compiles to, run in a scratch project and read back by the topology extractor;
//! the generated graph, with the net areas BIMCraft measured; the building and the report.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::super::graph::{BUILDING_KIND, SCHEMA_VERSION, TOPOLOGY_KIND};
use super::super::house::place::{DoorAt, Rect};
use super::super::house::{longest, sheet_for};
use super::super::layout::{EXTERIOR, INTERIOR};
use super::super::program::Archetype;
use super::super::report::{Check, Status};
use super::super::script::{capital, grid_name, sheet_size};
use super::super::{AreaRequest, Built, FILES, GENERATOR_VERSION, MANIFEST, Refusal, fnv1a64, pretty};
use super::place::Layout;
use super::{Means, StudioGraph};
use crate::engine::Session;
use crate::graph_rules::{self, Graph as RuleGraph, Pack};

pub const RULE_SET: &str = "studio-office-pack/1";

/// The commands of a studio-office script, and the room each `room` command makes.
pub struct Script {
    pub lines: Vec<(&'static str, Value)>,
    pub rooms: Vec<(usize, usize)>,
    pub text: String,
}

/// A try that ran: its script, the replies, the generated graph and the extracted one.
pub struct Done {
    pub script: Script,
    pub replies: Vec<Value>,
    pub graph: Value,
    pub checks: Vec<Check>,
}

fn line(id: &'static str, params: Value) -> (&'static str, Value) {
    (id, params)
}

pub fn compile(g: &StudioGraph, l: &Layout, seed: u64, area: AreaRequest) -> Script {
    let title = format!("Studio office {} m2, seed {seed}", area.m2.round());
    let mut lines = vec![
        line("new", json!({"name": title})),
        line("drawing_property", json!({"name": "building_type", "value": Archetype::StudioOffice.building_type()})),
        line("room_area", json!({"at": "finish"})),
        line("level", json!({"elevation": 3600, "name": "Roof"})),
    ];
    let mut xs: Vec<i64> = vec![0, l.width];
    let mut ys: Vec<i64> = vec![0, l.depth];
    for r in &l.rects {
        xs.extend([r.x0, r.x1]);
        ys.extend([r.y0, r.y1]);
    }
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();
    for (i, x) in xs.into_iter().enumerate() {
        lines.push(line("grid", json!({"start": [x, -1500], "end": [x, l.depth + 1500], "name": grid_name(i, false)})));
    }
    for (i, y) in ys.into_iter().enumerate() {
        lines.push(line("grid", json!({"start": [-1500, y], "end": [l.width + 1500, y], "name": grid_name(i, true)})));
    }
    for w in &l.walls {
        let kind = if w.exterior { EXTERIOR } else { INTERIOR };
        lines.push(line("wall", json!({"points": [w.a, w.b], "closed": false, "type": kind})));
    }
    lines.push(line("floor", json!({"boundary": l.boundary})));
    // The families the doors are of, with the types they use: one line for each.
    let mut types: BTreeMap<&str, BTreeMap<String, i64>> = BTreeMap::new();
    for d in &l.doors {
        types.entry(d.family).or_default().insert(d.ty.clone(), d.width);
    }
    for (family, set) in &types {
        let set: serde_json::Map<String, Value> = set.iter().map(|(ty, w)| (ty.clone(), json!({"Width": w, "Height": 2100}))).collect();
        lines.push(line("family", json!({"name": family, "types": set})));
    }
    for (i, d) in l.doors.iter().enumerate() {
        let mut params = json!({"at": d.at, "family": d.family, "type": d.ty, "mark": format!("D{}", i + 1)});
        if d.flip_facing {
            params["flip_facing"] = json!(true);
        }
        if d.flip_hand {
            params["flip_hand"] = json!(true);
        }
        lines.push(line("door", params));
    }
    for (i, w) in l.windows.iter().enumerate() {
        lines.push(line("window", json!({"at": w.at, "family": "Fixed Window", "type": w.ty, "mark": format!("W{}", i + 1)})));
    }
    for run in &l.separators {
        lines.push(line("room_separator", json!({"points": run})));
    }
    let mut rooms = Vec::new();
    for (i, r) in g.rooms.iter().enumerate() {
        rooms.push((lines.len(), i));
        lines.push(line("room", json!({"point": l.rects[i].mid(), "name": r.name, "number": r.id})));
    }
    for (vertical, offset) in [(false, 1300), (true, 1300)] {
        if let Some((a, b, side)) = longest(&l.boundary, vertical) {
            let (lo, hi) = if vertical { (a[1].min(b[1]), a[1].max(b[1])) } else { (a[0].min(b[0]), a[0].max(b[0])) };
            let (refs, at) = if vertical {
                ([[a[0] + 137, lo], [a[0] + 137, hi]], [a[0] + side * offset, (lo + hi) / 2])
            } else {
                ([[lo, a[1] + 137], [hi, a[1] + 137]], [(lo + hi) / 2, a[1] + side * offset])
            };
            lines.push(line("dimension", json!({"refs": refs, "at": at})));
        }
    }
    lines.push(line("tag", json!({})));
    let note = format!("Studio office of {} m2 gross, seed {seed}", (l.gross_mm2 as f64 / 1.0e6).round());
    lines.push(line("text", json!({"text": note, "at": [0, -2400]})));
    lines.push(line(
        "text",
        json!({"text": "Room areas are to wall finish faces. Made by the BIMCraft graph generator.", "at": [0, -3200]}),
    ));
    let (size, scale) = sheet_for(l.width, l.depth);
    let (sw, sh) = sheet_size(size);
    lines.push(line("sheet", json!({"number": "A101", "name": format!("{} Floor Plan", capital("studio office")), "size": size})));
    lines.push(line("place", json!({"sheet": "A101", "view": "Level 1", "scale": scale, "at": [sw / 2, sh / 2 + 10]})));
    let mut text = format!("# Generated by the BIMCraft graph generator: studio-office, seed {seed}. One command to a line.\n");
    for (id, params) in &lines {
        text.push_str(id);
        text.push(' ');
        text.push_str(&params.to_string());
        text.push('\n');
    }
    Script { lines, rooms, text }
}

fn id_of(g: &StudioGraph, i: usize) -> &str {
    &g.rooms[i].id
}

fn pair(a: &str, b: &str) -> (String, String) {
    if a <= b { (a.into(), b.into()) } else { (b.into(), a.into()) }
}

fn swing_fields(edge: &mut Value, d: &DoorAt) {
    if let (Some(o), Some(h)) = (&d.opens_into, d.hinge) {
        edge["opens_into"] = json!(o);
        edge["hinge"] = json!(h);
    }
}

/// The generated graph: its rooms with the net areas measured (by id), and its doors, open edges and ways out.
fn graph_json(g: &StudioGraph, l: &Layout, seed: u64, areas: &BTreeMap<String, f64>) -> Value {
    let nodes: Vec<Value> = g
        .rooms
        .iter()
        .map(|r| {
            json!({"id": r.id, "kind": r.kind.name(), "role": r.kind.role(), "name": r.name, "area_m2": areas.get(&r.id).copied().unwrap_or(0.0), "level": "Level 1"})
        })
        .collect();
    let mut edges = Vec::new();
    for d in &l.doors {
        let Some(k) = d.link else { continue };
        let link = &g.links[k];
        let (a, b) = (id_of(g, link.a), id_of(g, link.b));
        let mut edge = json!({
            "id": format!("door:{a}:{b}"), "from": a, "to": b, "kind": "door", "operation": d.op, "family": d.family, "type": d.ty,
            "width_mm": d.width,
        });
        swing_fields(&mut edge, d);
        edges.push(edge);
    }
    for link in g.links.iter().filter(|l| l.means == Means::Open) {
        let (a, b) = (id_of(g, link.a), id_of(g, link.b));
        edges.push(json!({"id": format!("open:{a}:{b}"), "from": a, "to": b, "kind": "open"}));
    }
    for d in l.doors.iter().filter(|d| d.link.is_none()) {
        let a = id_of(g, d.rooms.0);
        let mut edge = json!({
            "id": format!("door:{a}:exterior"), "from": a, "to": "exterior", "kind": "door", "exterior": true, "operation": d.op,
            "family": d.family, "type": d.ty, "width_mm": d.width,
        });
        swing_fields(&mut edge, d);
        edges.push(edge);
    }
    json!({
        "schema_version": SCHEMA_VERSION,
        "graph_kind": TOPOLOGY_KIND,
        "archetype": "studio-office",
        "seed": seed,
        "shell": l.shell,
        "entry": {"room": id_of(g, g.entry)},
        "nodes": nodes,
        "edges": edges,
    })
}

type Key = ((String, String), String, i64, Option<String>, Option<String>);
type Way = (String, String, i64, Option<String>, Option<String>);

/// What the extracted graph says of the generated one: nothing, when they are the same.
pub fn compare(g: &StudioGraph, l: &Layout, extracted: &Value) -> Result<(), String> {
    let mut problems = Vec::new();
    let ids: Vec<&str> = extracted["nodes"].as_array().into_iter().flatten().filter_map(|n| n["id"].as_str()).collect();
    for r in &g.rooms {
        if ids.iter().filter(|i| **i == r.id).count() != 1 {
            problems.push(format!("the room `{}` is not made once", r.id));
        }
    }
    if ids.len() != g.rooms.len() {
        problems.push(format!("{} rooms for {}", ids.len(), g.rooms.len()));
    }
    let mut want: BTreeMap<Key, i64> = BTreeMap::new();
    let mut want_ways: Vec<Way> = Vec::new();
    for d in &l.doors {
        let (oi, hg) = (d.opens_into.clone(), d.hinge.map(String::from));
        match d.link {
            Some(k) => {
                let (a, b) = (id_of(g, g.links[k].a), id_of(g, g.links[k].b));
                *want.entry((pair(a, b), d.op.to_string(), d.width, oi, hg)).or_default() += 1;
            }
            None => want_ways.push((id_of(g, d.rooms.0).to_string(), d.op.to_string(), d.width, oi, hg)),
        }
    }
    let mut got: BTreeMap<Key, i64> = BTreeMap::new();
    let mut got_ways: Vec<Way> = Vec::new();
    let mut opens: BTreeMap<(String, String), i64> = BTreeMap::new();
    for e in extracted["edges"].as_array().into_iter().flatten() {
        let (a, b) = (e["from"].as_str().unwrap_or_default(), e["to"].as_str().unwrap_or_default());
        let (oi, hg) = (e["opens_into"].as_str().map(String::from), e["hinge"].as_str().map(String::from));
        let op = e["operation"].as_str().unwrap_or("?").to_string();
        let w = e["width_mm"].as_f64().map_or(-1, |w| w.round() as i64);
        match e["kind"].as_str() {
            Some("door") if e["exterior"] == true => got_ways.push((a.to_string(), op, w, oi, hg)),
            Some("door") => *got.entry((pair(a, b), op, w, oi, hg)).or_default() += 1,
            Some("open") => *opens.entry(pair(a, b)).or_default() += 1,
            _ => {}
        }
    }
    want_ways.sort();
    got_ways.sort();
    if want != got {
        let show = |k: &Key| format!("{} to {} by {} {} opening into {:?}, hinge {:?}", k.0.0, k.0.1, k.1, k.2, k.3, k.4);
        let missing: Vec<_> = want.iter().filter(|(k, n)| got.get(*k) != Some(n)).map(|(k, _)| show(k)).collect();
        let extra: Vec<_> = got.iter().filter(|(k, n)| want.get(*k) != Some(n)).map(|(k, _)| show(k)).collect();
        problems.push(format!("doors: missing {missing:?}, extra {extra:?}"));
    }
    let want_open: BTreeMap<(String, String), i64> = g.links.iter().filter(|l| l.means == Means::Open).fold(BTreeMap::new(), |mut m, l| {
        *m.entry(pair(id_of(g, l.a), id_of(g, l.b))).or_default() += 1;
        m
    });
    if want_open != opens {
        problems.push(format!(
            "open edges: wanted {:?}, extracted {:?}",
            want_open.keys().collect::<Vec<_>>(),
            opens.keys().collect::<Vec<_>>()
        ));
    }
    if want_ways != got_ways {
        problems.push(format!("the ways out: wanted {want_ways:?}, extracted {got_ways:?}"));
    }
    if extracted["entry"]["room"].as_str() != Some(id_of(g, g.entry)) {
        problems.push(format!("the entry is {:?}, not `{}`", extracted["entry"]["room"], id_of(g, g.entry)));
    }
    if extracted["diagnostics"].as_array().is_some_and(|d| !d.is_empty()) {
        problems.push(format!("diagnostics {}", extracted["diagnostics"]));
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join("; ")) }
}

fn rules_pass(graph: &Value) -> Result<Vec<Value>, String> {
    let pack = Pack::embedded("studio-office")?;
    let g = RuleGraph::from_value(graph)?;
    let results = graph_rules::check(pack, &g)?;
    let failed: Vec<String> =
        results.iter().filter(|c| c.status == graph_rules::Status::Fail).map(|c| format!("{} {}: {}", c.id, c.name, c.detail)).collect();
    if failed.is_empty() { Ok(results.iter().map(graph_rules::Check::json).collect()) } else { Err(failed.join("; ")) }
}

/// Builds the layout in a scratch project and reads it back: its graph is what was generated, or why not.
pub fn verify(g: &StudioGraph, l: &Layout, seed: u64, area: AreaRequest) -> Result<Done, String> {
    let script = compile(g, l, seed, area);
    let mut scratch = Session::new();
    let replies = scratch.script(&script.text).map_err(|e| format!("the script does not run: {e}"))?;
    for (at, room) in &script.rooms {
        if let Some(w) = replies.get(*at).and_then(|r| r.get("warning")).and_then(Value::as_str) {
            return Err(format!("the room `{}` is not bounded: {w}", g.rooms[*room].id));
        }
    }
    let extracted = crate::topology::extract(&scratch.model);
    compare(g, l, &extracted)?;
    let areas: BTreeMap<String, f64> = extracted["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| Some((n["id"].as_str()?.to_string(), n["area_m2"].as_f64()?)))
        .collect();
    let graph = graph_json(g, l, seed, &areas);
    let generated = rules_pass(&graph).map_err(|e| format!("the generated graph fails the studio-office rules: {e}"))?;
    let mut built = extracted.clone();
    let kinds: BTreeMap<&str, &str> = g.rooms.iter().map(|r| (r.id.as_str(), r.kind.name())).collect();
    if let Some(nodes) = built["nodes"].as_array_mut() {
        for n in nodes {
            let kind = n["id"].as_str().and_then(|i| kinds.get(i)).copied();
            n["kind"] = json!(kind);
        }
    }
    built["archetype"] = json!("studio-office");
    rules_pass(&built).map_err(|e| format!("the built studio office fails the studio-office rules: {e}"))?;
    let deviation = (l.gross_mm2 as f64 / 1.0e6 - area.m2).abs() / area.m2;
    let checks = vec![
        Check {
            id: "SOG-001",
            name: "graph-passes-studio-office-pack",
            status: Status::Pass,
            detail: format!("{} rules pass on the generated graph", generated.len()),
        },
        Check {
            id: "SOG-002",
            name: "script-runs",
            status: Status::Pass,
            detail: format!("all {} commands ran in a fresh project", script.lines.len()),
        },
        Check {
            id: "SOG-003",
            name: "extracted-equals-generated",
            status: Status::Pass,
            detail: "the topology extractor finds the generated rooms, doors, open edges, ways out and entry".into(),
        },
        Check {
            id: "SOG-004",
            name: "built-passes-studio-office-pack",
            status: Status::Pass,
            detail: "the extracted graph, with the generated kinds, passes the studio-office rules".into(),
        },
        Check {
            id: "SOG-005",
            name: "area-fidelity",
            status: if deviation <= 0.06 { Status::Pass } else { Status::Warn },
            detail: format!("{:.2} m2 gross for {} m2 asked", l.gross_mm2 as f64 / 1.0e6, area.m2),
        },
    ];
    Ok(Done { script, replies, graph, checks })
}

fn program_json(g: &StudioGraph, seed: u64, area: AreaRequest) -> Value {
    let pack = Pack::embedded("studio-office").ok();
    let range =
        |kind: super::Kind| pack.and_then(|p| p.kinds.get(kind.name()).map(|k| json!([k.area_m2.0, k.area_m2.1]))).unwrap_or(Value::Null);
    json!({
        "schema_version": SCHEMA_VERSION,
        "archetype": "studio-office",
        "seed": seed,
        "requested_area_m2": area.m2,
        "pack_id": pack.map(|p| p.pack_id.clone()),
        "spaces": g.rooms.iter().map(|r| json!({"id": r.id, "kind": r.kind.name(), "label": r.name, "role": r.kind.role(), "area_m2_range": range(r.kind)})).collect::<Vec<_>>(),
        "edges": g.links.iter().map(|l| json!({
            "from": id_of(g, l.a), "to": id_of(g, l.b),
            "kind": match l.means { Means::Open => "open", _ => "door" },
            "means": match l.means { Means::Open => "open", Means::Swing => "swing", Means::DoubleSwing => "double-swing" },
        })).collect::<Vec<_>>(),
        "entry": id_of(g, g.entry),
    })
}

fn building_json(g: &StudioGraph, l: &Layout, seed: u64, topology_hash: &str, edges: usize) -> Value {
    let bounds = |r: &Rect| json!({"x0": r.x0, "y0": r.y0, "x1": r.x1, "y1": r.y1});
    json!({
        "schema_version": SCHEMA_VERSION,
        "graph_kind": BUILDING_KIND,
        "archetype": "studio-office",
        "seed": seed,
        "shell": l.shell,
        "units": "mm",
        "source_2d": {"file": "topology-graph.json", "topology_hash": topology_hash, "nodes": g.rooms.len(), "edges": edges},
        "footprint": {"width": l.width, "depth": l.depth, "boundary": l.boundary},
        "levels": [{"id": "level_1", "name": "Level 1", "elevation_mm": 0}, {"id": "level_roof", "name": "Roof", "elevation_mm": 3600}],
        "spaces": g.rooms.iter().enumerate().map(|(i, r)| json!({"id": format!("space_{}", r.id), "source_2d": r.id, "level": "level_1", "bounds": bounds(&l.rects[i])})).collect::<Vec<_>>(),
        "walls": l.walls.iter().enumerate().map(|(i, w)| json!({
            "id": format!("wall_{}", i + 1), "kind": if w.exterior { "exterior" } else { "partition" },
            "type": if w.exterior { EXTERIOR } else { INTERIOR }, "start": w.a, "end": w.b,
        })).collect::<Vec<_>>(),
        "openings": l.doors.iter().enumerate().map(|(i, d)| json!({
            "id": format!("door_{}", i + 1), "kind": "door", "at": d.at, "family": d.family, "type": d.ty, "operation": d.op,
            "width_mm": d.width, "source_2d": id_of(g, d.rooms.0),
            "source_topology_edge": d.rooms.1.map_or(format!("door:{}:exterior", id_of(g, d.rooms.0)), |b| format!("door:{}:{}", id_of(g, d.rooms.0), id_of(g, b))),
        })).chain(l.windows.iter().enumerate().map(|(i, w)| json!({
            "id": format!("window_{}", i + 1), "kind": "window", "at": w.at, "family": "Fixed Window", "type": w.ty, "width_mm": w.width,
        }))).collect::<Vec<_>>(),
        "slabs": [{"id": "slab_level_1", "kind": "floor", "level": "level_1", "boundary": l.boundary}],
        "vertical_circulation": [],
        "components": [],
        "systems": [],
    })
}

/// The run directory's files.
pub fn finish(g: &StudioGraph, l: &Layout, done: Done, seed: u64, area: AreaRequest) -> Result<Built, Refusal> {
    let topology_text = pretty(&done.graph)?;
    let topology_hash = fnv1a64(&topology_text);
    let edges = done.graph["edges"].as_array().map_or(0, Vec::len);
    let measured: Vec<Value> = done
        .script
        .rooms
        .iter()
        .filter_map(|(at, room)| Some(json!({"space": g.rooms[*room].id, "net_area_m2": done.replies.get(*at)?.get("area_m2")?})))
        .collect();
    let net: f64 = measured.iter().filter_map(|m| m["net_area_m2"].as_f64()).sum();
    let gross = l.gross_mm2 as f64 / 1.0e6;
    let count = |s: Status| done.checks.iter().filter(|c| c.status == s).count();
    let report = json!({
        "schema_version": SCHEMA_VERSION,
        "archetype": "studio-office",
        "seed": seed,
        "shell": l.shell,
        "rule_set": RULE_SET,
        "summary": {"pass": count(Status::Pass), "warn": count(Status::Warn), "fail": count(Status::Fail)},
        "checks": done.checks.iter().map(|c| json!({"id": c.id, "name": c.name, "status": match c.status { Status::Pass => "PASS", Status::Warn => "WARN", Status::Fail => "FAIL" }, "detail": c.detail})).collect::<Vec<_>>(),
        "warnings": done.checks.iter().filter(|c| c.status == Status::Warn).map(|c| format!("{}: {}", c.id, c.detail)).collect::<Vec<_>>(),
        "rules": rules_pass(&done.graph).unwrap_or_default(),
        "capability_notes": [
            "The graph is sampled from the studio-office rule pack (`data/graph-rules/studio-office.json`) by the seed, then placed in a shell; the topology extractor, run on the built project, gives the same graph, which the pack's rules judge.",
            "Areas in the building file are gross, to wall centre lines; the topology graph's `area_m2` and `measured_rooms` are the net areas BIMCraft computed for each room.",
            "One storey; shells bar, flush_west, flush_east and inset, turned and mirrored by the seed. Doors are of the Single Door and Double Door families, windows of the Fixed Window family; the passage is open to the studio, drawn with a room separation line. No stairs, roof or furniture.",
        ],
        "scores": {"room_count": g.rooms.len(), "net_to_gross": if gross > 0.0 { (net / gross * 1000.0).round() / 1000.0 } else { 0.0 }},
        "measured_rooms": measured,
    });
    let texts = [
        pretty(&program_json(g, seed, area))?,
        topology_text,
        pretty(&building_json(g, l, seed, &topology_hash, edges))?,
        done.script.text.clone(),
        pretty(&report)?,
    ];
    let named: Vec<Value> =
        FILES.iter().zip(&texts).map(|((key, name), text)| json!({"key": key, "path": name, "hash": fnv1a64(text)})).collect();
    let manifest = pretty(&json!({
        "schema_version": SCHEMA_VERSION,
        "generator_version": GENERATOR_VERSION,
        "archetype": "studio-office",
        "seed": seed,
        "shell": l.shell,
        "requested_area": {"value": area.value, "unit": area.unit},
        "requested_area_m2": area.m2,
        "gross_area_m2": gross,
        "hash_algorithm": "fnv1a64",
        "topology_hash": topology_hash,
        "files": named,
    }))?;
    let mut files: Vec<(&'static str, String)> = FILES.iter().map(|(_, name)| *name).zip(texts).collect();
    files.push((MANIFEST, manifest));
    Ok(Built {
        archetype: Archetype::StudioOffice,
        seed,
        plan: None,
        checks: done.checks,
        files,
        topology_hash,
        gross_m2: gross,
        rooms: g.rooms.len(),
    })
}
