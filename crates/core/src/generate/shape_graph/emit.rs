//! A laid-out plan as files: the script it compiles to (walls, doors, room separation lines, rooms), run in a scratch project and
//! read back by the topology extractor; the generated graph with the net areas BIMCraft measured; the program, the building and
//! the report.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::super::graph::{BUILDING_KIND, SCHEMA_VERSION, TOPOLOGY_KIND};
use super::super::house::swing;
use super::super::report::{Check, Status};
use super::family::{Conn, Family};
use super::layout::Laid;
use super::links::{Box4, Rc, boxed, by_room, outline, outside, shared, wall};
use super::plan::Room;
use super::tags::{self, TagAt};
use crate::engine::Session;
use crate::graph_rules::{self, Graph as RuleGraph, Pack};

pub const EXTERIOR: &str = "Exterior - 250mm";
pub const INTERIOR: &str = "Interior - 130mm";

#[derive(Clone, Debug)]
pub struct WallAt {
    pub exterior: bool,
    pub a: [i64; 2],
    pub b: [i64; 2],
}

#[derive(Clone, Debug)]
pub struct DoorAt {
    /// The rooms it joins; the second is none for the way in from outside.
    pub rooms: (usize, Option<usize>),
    pub conn: Conn,
    pub at: [i64; 2],
    pub family: &'static str,
    pub ty: String,
    pub width: i64,
    pub op: &'static str,
    pub flip_facing: bool,
    pub flip_hand: bool,
    /// A swing door's side (a room, or none for outside) and jamb, set by `orient`.
    pub opens_into: Option<Option<usize>>,
    pub hinge: Option<&'static str>,
}

/// The walls, doors and separation lines a plan draws.
#[derive(Clone, Debug)]
pub struct Drawn {
    pub walls: Vec<WallAt>,
    pub doors: Vec<DoorAt>,
    pub separators: Vec<Vec<[i64; 2]>>,
}

/// A door's family, type name, width and operation, by its connection type and how long a run of wall it has.
fn door_for(conn: Conn, run: i64) -> Option<(&'static str, String, i64, &'static str)> {
    let (family, op) = conn.door()?;
    let widths: &[i64] = match conn {
        Conn::Single => &[900, 800, 750],
        Conn::Double => &[1800, 1600, 1500, 1200],
        Conn::Sliding => &[1500, 1200, 900, 750],
        _ => &[900, 800, 750],
    };
    widths.iter().copied().find(|w| run >= w + 300).map(|w| (family, format!("{w} x 2100"), w, op))
}

/// Walls between rooms (a door or a separation line where two are linked), outside walls, and the way in.
pub fn draw(fam: &Family, rooms: &[Room], laid: &Laid) -> Result<Drawn, String> {
    let by = by_room(&laid.rects);
    let links: BTreeMap<(usize, usize), Conn> = laid.linked.links.iter().map(|(a, b, t)| ((*a, *b), *t)).collect();
    let mut d = Drawn { walls: Vec::new(), doors: Vec::new(), separators: Vec::new() };
    let rects = &laid.rects;
    for (n, ra) in rects.iter().enumerate() {
        for rb in &rects[n + 1..] {
            let (i, j) = (ra.0, rb.0);
            let Some(e) = shared(boxed(ra), boxed(rb)).filter(|_| i != j) else { continue };
            let t = links.get(&(i.min(j), i.max(j))).copied();
            if t == Some(Conn::OpenFull) {
                d.separators.push(vec![e.point(e.lo), e.point(e.hi)]);
                continue;
            }
            d.walls.push(WallAt { exterior: false, a: e.point(e.lo), b: e.point(e.hi) });
            if let Some(t) = t
                && wall(&by, i, j) == Some(e)
            {
                let (family, ty, width, op) = door_for(t, e.len()).ok_or_else(|| {
                    format!("the wall of {} mm between `{}` and `{}` is too short for its door", e.len(), rooms[i].id, rooms[j].id)
                })?;
                d.doors.push(DoorAt {
                    rooms: (i.min(j), Some(i.max(j))),
                    conn: t,
                    at: e.point((e.lo + e.hi) / 2),
                    family,
                    ty,
                    width,
                    op,
                    flip_facing: false,
                    flip_hand: false,
                    opens_into: None,
                    hinge: None,
                });
            }
        }
    }
    for r in rects {
        for e in outside(rects, boxed(r)) {
            d.walls.push(WallAt { exterior: true, a: e.point(e.lo), b: e.point(e.hi) });
        }
    }
    if let (Some((room, e)), Some(entry)) = (laid.linked.entry, &fam.entry) {
        let conn = entry.types.first().copied().unwrap_or(Conn::Single);
        let (family, ty, width, op) = door_for(conn, e.len()).ok_or("the way in is too short for its door")?;
        d.doors.push(DoorAt {
            rooms: (room, None),
            conn,
            at: e.point((e.lo + e.hi) / 2),
            family,
            ty,
            width,
            op,
            flip_facing: false,
            flip_hand: false,
            opens_into: None,
            hinge: None,
        });
    }
    // Links the layout made that have no door drawn mean a wall the longest-wall rule did not find.
    let linked_doors = laid.linked.links.iter().filter(|(_, _, t)| t.door().is_some()).count();
    if d.doors.iter().filter(|x| x.rooms.1.is_some()).count() != linked_doors {
        return Err("a link has no wall to hold its door".into());
    }
    orient(fam, rooms, laid, &mut d)?;
    Ok(d)
}

fn holds(r: &Rc, p: [f64; 2]) -> bool {
    p[0] > r.1 as f64 && p[0] < (r.1 + r.3) as f64 && p[1] > r.2 as f64 && p[1] < (r.2 + r.4) as f64
}

/// Sets each swing door's flips, and what the extractor will say of it, from the swing rules of the room kinds. A door whose host
/// wall is not found exactly once is an error.
fn orient(fam: &Family, rooms: &[Room], laid: &Laid, d: &mut Drawn) -> Result<(), String> {
    let exterior = swing::exterior_rule();
    let kind = |i: usize| fam.types[rooms[i].ty].kind.as_str();
    let room_at = |p: [f64; 2]| laid.rects.iter().find(|r| holds(r, p)).map(|r| r.0);
    for door in &mut d.doors {
        if !matches!(door.op, "swing" | "double-swing") {
            continue;
        }
        let at = [door.at[0] as f64, door.at[1] as f64];
        let hosts: Vec<&WallAt> = d
            .walls
            .iter()
            .filter(|w| {
                let (a, b) = ([w.a[0] as f64, w.a[1] as f64], [w.b[0] as f64, w.b[1] as f64]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = dx.hypot(dy);
                if len < 1.0 {
                    return false;
                }
                let s = ((at[0] - a[0]) * dx + (at[1] - a[1]) * dy) / len;
                let off = ((at[0] - a[0]) * dy - (at[1] - a[1]) * dx).abs() / len;
                off <= 1.0 && s >= 0.0 && s <= len
            })
            .collect();
        let [w] = hosts.as_slice() else {
            return Err(format!("a door at {:?} has {} host walls under it, not one", door.at, hosts.len()));
        };
        let (a, b) = ([w.a[0] as f64, w.a[1] as f64], [w.b[0] as f64, w.b[1] as f64]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let n = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
        let half = if w.exterior { 150.0 } else { 50.0 };
        let k = half + 150.0;
        let side_room = |sign: f64| room_at([at[0] + sign * n[0] * k, at[1] + sign * n[1] * k]);
        let (left, right) = (side_room(1.0), side_room(-1.0));
        // The default is the left side and the start jamb; a rule may ask for the other. The target is the room (none: outside)
        // the door should open into, when a rule enforces one.
        let mut hinge_end = false;
        let mut target: Option<Option<usize>> = None;
        match (left, right) {
            (Some(x), Some(y)) if x != y => {
                if let Some(r) = swing::rule_for(kind(x), kind(y)) {
                    hinge_end = r.hinge == "end";
                    let rk = r.opens_into.as_str();
                    if kind(x) != kind(y) && rk == kind(x) {
                        target = Some(Some(x));
                    } else if kind(x) != kind(y) && rk == kind(y) {
                        target = Some(Some(y));
                    }
                }
            }
            (Some(x), None) | (None, Some(x)) => {
                if let Some(r) = exterior {
                    hinge_end = r.hinge == "end";
                    target = match r.opens_into.as_str() {
                        "either" => None,
                        "exterior" => Some(None),
                        _ => Some(Some(x)),
                    };
                }
            }
            _ => {}
        }
        let face = target.is_some_and(|t| t != left);
        door.flip_facing = face;
        door.flip_hand = hinge_end;
        door.opens_into = Some(if face { right } else { left });
        door.hinge = Some(if hinge_end { "end" } else { "start" });
    }
    Ok(())
}

/// The names and numbers of the rooms: the room type's name, with 1, 2, 3 when the plan has several of the type; and `101` upward.
pub fn names(fam: &Family, rooms: &[Room]) -> Vec<(String, String)> {
    let mut seen: BTreeMap<usize, usize> = BTreeMap::new();
    let total: BTreeMap<usize, usize> = rooms.iter().fold(BTreeMap::new(), |mut m, r| {
        *m.entry(r.ty).or_default() += 1;
        m
    });
    rooms
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let k = seen.entry(r.ty).or_default();
            *k += 1;
            let base = &fam.types[r.ty].name;
            let name = if total[&r.ty] > 1 { format!("{base} {k}") } else { base.clone() };
            (name, format!("{}", 101 + i))
        })
        .collect()
}

/// The commands of a script, and the room each `room` command makes.
pub struct Script {
    pub lines: Vec<(&'static str, Value)>,
    pub rooms: Vec<(usize, usize)>,
    pub text: String,
}

fn folding_types(types: serde_json::Map<String, Value>) -> Value {
    super::super::house::folding_family(types)
}

/// Where each room's `room` command puts its point: the middle of its biggest box.
pub fn room_points(rooms: &[Room], laid: &Laid) -> Vec<[i64; 2]> {
    let by = by_room(&laid.rects);
    (0..rooms.len())
        .map(|i| {
            let big: Option<&Box4> = by.get(&i).and_then(|v| {
                v.iter().fold(None, |best: Option<&Box4>, b| if best.is_none_or(|x| b.2 * b.3 > x.2 * x.3) { Some(b) } else { best })
            });
            let (x, y, w, h) = big.copied().unwrap_or((0, 0, 0, 0));
            [x + w / 2, y + h / 2]
        })
        .collect()
}

/// The script of a plan; `tags` has, for a room whose tag does not fit, where its tag stands and its point.
pub fn compile(fam: &Family, rooms: &[Room], laid: &Laid, d: &Drawn, seed: u64, tags: &[Option<TagAt>], gridlines: bool) -> Script {
    let title = format!("{} {} rooms, seed {seed}", fam.name, rooms.len());
    let mut lines: Vec<(&'static str, Value)> = vec![
        ("new", json!({"name": title, "seed": seed})),
        ("room_area", json!({"at": "finish"})),
        ("drawing_property", json!({"name": "building_type", "value": fam.building_type})),
        // From here on the plan's DXF and DWG keep the delivery drawing rules.
        ("cad_profile", json!({"name": "delivery"})),
        ("material", json!({"name": "Plan Poche", "color": "#2f2f2f"})),
    ];
    for (name, thickness) in [(EXTERIOR, 250), (INTERIOR, 130)] {
        lines.push(("type", json!({"category": "wall", "name": name, "layers": [{"function": "structure", "material": "Plan Poche", "thickness": thickness}]})));
    }
    let mut types: BTreeMap<&str, BTreeMap<String, i64>> = BTreeMap::new();
    for door in &d.doors {
        types.entry(door.family).or_default().insert(door.ty.clone(), door.width);
    }
    for (family, set) in &types {
        let set: serde_json::Map<String, Value> = set.iter().map(|(ty, w)| (ty.clone(), json!({"Width": w, "Height": 2100}))).collect();
        if *family == super::super::house::FOLDING_FAMILY {
            lines.push(("family", folding_types(set)));
        } else {
            lines.push(("family", json!({"name": family, "types": set})));
        }
    }
    if gridlines {
        // One line at each distinct x and y of the outline's corners; the exterior walls are the outline.
        let segments: Vec<([i64; 2], [i64; 2])> = d.walls.iter().filter(|w| w.exterior).map(|w| (w.a, w.b)).collect();
        for grid in super::super::script::outline_grids(&super::super::script::segment_corners(&segments)) {
            lines.push(("grid", grid));
        }
    }
    for w in &d.walls {
        lines.push(("wall", json!({"points": [w.a, w.b], "closed": false, "type": if w.exterior { EXTERIOR } else { INTERIOR }})));
    }
    for (i, door) in d.doors.iter().enumerate() {
        let mut params = json!({"at": door.at, "family": door.family, "type": door.ty, "mark": format!("D{}", i + 1)});
        if door.flip_facing {
            params["flip_facing"] = json!(true);
        }
        if door.flip_hand {
            params["flip_hand"] = json!(true);
        }
        lines.push(("door", params));
    }
    for run in &d.separators {
        lines.push(("room_separator", json!({"points": run})));
    }
    let label = names(fam, rooms);
    let points = room_points(rooms, laid);
    let mut room_lines = Vec::new();
    for (i, (name, number)) in label.iter().enumerate() {
        room_lines.push((lines.len(), i));
        let mut params = json!({"point": points[i], "name": name, "number": number});
        if let Some(Some(t)) = tags.get(i) {
            params["point"] = json!(t.point);
            params["tag"] = json!(t.tag);
        }
        lines.push(("room", params));
    }
    let mut text = format!("# Generated by the BIMCraft shape graph generator: {}, seed {seed}. One command to a line.\n", fam.name);
    for (id, params) in &lines {
        text.push_str(id);
        text.push(' ');
        text.push_str(&params.to_string());
        text.push('\n');
    }
    Script { lines, rooms: room_lines, text }
}

/// The generated graph: rooms with the net areas measured, doors and open edges.
pub fn graph_json(fam: &Family, rooms: &[Room], laid: &Laid, d: &Drawn, shape: &str, seed: u64, areas: &BTreeMap<String, f64>) -> Value {
    let label = names(fam, rooms);
    let id_of = |i: usize| rooms[i].id.as_str();
    let nodes: Vec<Value> = rooms
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let ty = &fam.types[r.ty];
            json!({
                "id": r.id, "room_type": ty.key, "number": label[i].1, "kind": ty.kind, "role": ty.role, "name": label[i].0,
                "area_m2": areas.get(&label[i].1).copied().unwrap_or(0.0), "level": "Level 1",
            })
        })
        .collect();
    let mut edges = Vec::new();
    for door in &d.doors {
        let mut edge = match door.rooms.1 {
            Some(b) => {
                let a = door.rooms.0;
                json!({"id": format!("door:{}:{}", id_of(a), id_of(b)), "from": id_of(a), "to": id_of(b), "kind": "door"})
            }
            None => {
                let a = id_of(door.rooms.0);
                json!({"id": format!("door:{a}:exterior"), "from": a, "to": "exterior", "kind": "door", "exterior": true})
            }
        };
        edge["connection"] = json!(door.conn.id());
        edge["operation"] = json!(door.op);
        edge["family"] = json!(door.family);
        edge["type"] = json!(door.ty);
        edge["width_mm"] = json!(door.width);
        if let (Some(into), Some(h)) = (door.opens_into, door.hinge) {
            edge["opens_into"] = json!(into.map_or("exterior", id_of));
            edge["hinge"] = json!(h);
        }
        edges.push(edge);
    }
    for (a, b, t) in laid.linked.links.iter().filter(|(_, _, t)| t.is_open()) {
        edges.push(json!({"id": format!("open:{}:{}", id_of(*a), id_of(*b)), "from": id_of(*a), "to": id_of(*b), "kind": "open", "connection": t.id()}));
    }
    json!({
        "schema_version": SCHEMA_VERSION,
        "graph_kind": TOPOLOGY_KIND,
        "archetype": fam.name,
        "seed": seed,
        "shell": shape,
        "entry": {"room": laid.linked.entry.map(|(r, _)| id_of(r))},
        "nodes": nodes,
        "edges": edges,
    })
}

fn pair(a: &str, b: &str) -> (String, String) {
    if a <= b { (a.into(), b.into()) } else { (b.into(), a.into()) }
}

/// What the extracted graph says of the generated plan: nothing, when they are the same. Rooms are the numbers they were drawn with.
pub fn compare(fam: &Family, rooms: &[Room], laid: &Laid, d: &Drawn, extracted: &Value) -> Result<(), String> {
    let label = names(fam, rooms);
    let number = |i: usize| label[i].1.as_str();
    let mut problems = Vec::new();
    let ids: Vec<&str> = extracted["nodes"].as_array().into_iter().flatten().filter_map(|n| n["id"].as_str()).collect();
    for (i, (name, _)) in label.iter().enumerate() {
        if ids.iter().filter(|x| **x == number(i)).count() != 1 {
            problems.push(format!("the room `{name}` is not made once"));
        }
    }
    if ids.len() != rooms.len() {
        problems.push(format!("{} rooms for {}", ids.len(), rooms.len()));
    }
    type Key = ((String, String), String, i64, Option<String>, Option<String>);
    let mut want: BTreeMap<Key, i64> = BTreeMap::new();
    let mut want_exterior = None;
    for door in &d.doors {
        let into = door.opens_into.map(|o| o.map_or("exterior".to_string(), |r| number(r).to_string()));
        let hinge = door.hinge.map(String::from);
        match door.rooms.1 {
            Some(b) => {
                *want.entry((pair(number(door.rooms.0), number(b)), door.op.to_string(), door.width, into, hinge)).or_default() += 1;
            }
            None => want_exterior = Some((into, hinge)),
        }
    }
    let mut got: BTreeMap<Key, i64> = BTreeMap::new();
    let mut opens: BTreeSet<(String, String)> = BTreeSet::new();
    let (mut exterior, mut got_exterior) = (0, None);
    for e in extracted["edges"].as_array().into_iter().flatten() {
        let (a, b) = (e["from"].as_str().unwrap_or_default(), e["to"].as_str().unwrap_or_default());
        match e["kind"].as_str() {
            Some("door") if e["exterior"] == true => {
                exterior += 1;
                got_exterior = Some((e["opens_into"].as_str().map(String::from), e["hinge"].as_str().map(String::from)));
            }
            Some("door") => {
                let op = e["operation"].as_str().unwrap_or("?").to_string();
                let w = e["width_mm"].as_f64().map_or(-1, |w| w.round() as i64);
                *got.entry((pair(a, b), op, w, e["opens_into"].as_str().map(String::from), e["hinge"].as_str().map(String::from)))
                    .or_default() += 1;
            }
            Some("open") => {
                opens.insert(pair(a, b));
            }
            _ => {}
        }
    }
    if want != got {
        let miss: Vec<_> = want
            .iter()
            .filter(|(k, n)| got.get(*k) != Some(n))
            .map(|(k, _)| format!("{}-{} by {} {} into {:?} hinge {:?}", k.0.0, k.0.1, k.1, k.2, k.3, k.4))
            .collect();
        let extra: Vec<_> = got
            .iter()
            .filter(|(k, n)| want.get(*k) != Some(n))
            .map(|(k, _)| format!("{}-{} by {} {} into {:?} hinge {:?}", k.0.0, k.0.1, k.1, k.2, k.3, k.4))
            .collect();
        problems.push(format!("doors: missing {miss:?}, extra {extra:?}"));
    }
    let want_open: BTreeSet<(String, String)> =
        laid.linked.links.iter().filter(|(_, _, t)| t.is_open()).map(|(a, b, _)| pair(number(*a), number(*b))).collect();
    if want_open != opens {
        problems.push(format!("open edges: wanted {want_open:?}, extracted {opens:?}"));
    }
    if want_exterior != got_exterior {
        problems.push(format!("the outside door: wanted {want_exterior:?}, extracted {got_exterior:?}"));
    }
    if usize::from(laid.linked.entry.is_some()) != exterior {
        problems.push(format!("{exterior} outside doors"));
    }
    let entry = laid.linked.entry.map(|(r, _)| number(r));
    if extracted["entry"]["room"].as_str() != entry {
        problems.push(format!("the entry is {:?}, not {entry:?}", extracted["entry"]["room"]));
    }
    if extracted["diagnostics"].as_array().is_some_and(|x| !x.is_empty()) {
        problems.push(format!("diagnostics {}", extracted["diagnostics"]));
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join("; ")) }
}

fn rules_pass(pack_id: &str, graph: &Value) -> Result<Vec<Value>, String> {
    let pack = Pack::embedded(pack_id)?;
    let g = RuleGraph::from_value(graph)?;
    let results = graph_rules::check(pack, &g)?;
    let failed: Vec<String> =
        results.iter().filter(|c| c.status == graph_rules::Status::Fail).map(|c| format!("{} {}: {}", c.id, c.name, c.detail)).collect();
    if failed.is_empty() { Ok(results.iter().map(graph_rules::Check::json).collect()) } else { Err(failed.join("; ")) }
}

/// A plan that ran: its drawn parts, script, replies, graph and checks.
pub struct Done {
    pub drawn: Drawn,
    pub script: Script,
    pub replies: Vec<Value>,
    pub graph: Value,
    pub rules: Vec<Value>,
    pub checks: Vec<Check>,
}

/// Why a plan that passed the checks on rectangles did not make files.
pub enum Trouble {
    /// The saved model read back differently from the plan: the port's measures differ from BIMCraft's.
    Readback(String),
    /// The plan itself: a door with no wall, a rule of the family's pack.
    Plan(String),
}

/// Builds the plan in a scratch project and reads it back: its graph is what was generated, or why not.
pub fn verify(fam: &Family, rooms: &[Room], laid: &Laid, shape: &str, seed: u64, gridlines: bool) -> Result<Done, Trouble> {
    let drawn = draw(fam, rooms, laid).map_err(Trouble::Plan)?;
    let label = names(fam, rooms);
    let read = |script: &Script| -> Result<(Vec<Value>, Value), Trouble> {
        // The script's `new` has a seed: the scratch project must not leave it to the caller's.
        let (scratch, replies) = crate::ids::keeping_reproducible(|| {
            let mut scratch = Session::new();
            let replies = scratch.script(&script.text);
            (scratch, replies)
        });
        let replies = replies.map_err(|e| Trouble::Readback(format!("the script does not run: {e}")))?;
        for (at, room) in &script.rooms {
            if let Some(w) = replies.get(*at).and_then(|r| r.get("warning")).and_then(Value::as_str) {
                return Err(Trouble::Readback(format!("the room `{}` is not bounded: {w}", label[*room].0)));
            }
        }
        let extracted = crate::topology::extract(&scratch.model);
        compare(fam, rooms, laid, &drawn, &extracted).map_err(Trouble::Readback)?;
        Ok((replies, extracted))
    };
    let mut script = compile(fam, rooms, laid, &drawn, seed, &[], gridlines);
    let (mut replies, mut extracted) = read(&script)?;
    // A room whose tag does not fit in it has the tag outside the plan, with a leader: the script says where. What fits is
    // known only once the rooms are measured, so the script runs again with the tags in it.
    let at = tags::plan(fam, rooms, laid, &drawn, &extracted, &room_points(rooms, laid)).map_err(Trouble::Plan)?;
    if at.iter().any(Option::is_some) {
        script = compile(fam, rooms, laid, &drawn, seed, &at, gridlines);
        (replies, extracted) = read(&script)?;
    }
    let areas: BTreeMap<String, f64> = extracted["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| Some((n["id"].as_str()?.to_string(), n["area_m2"].as_f64()?)))
        .collect();
    for (i, r) in rooms.iter().enumerate() {
        let spec = &fam.types[r.ty];
        let a = areas.get(&label[i].1).copied().unwrap_or(0.0);
        if a < spec.area.0 || a > spec.area.1 {
            return Err(Trouble::Readback(format!(
                "the room `{}` measures {a:.2} m2, outside its {} to {}",
                label[i].0, spec.area.0, spec.area.1
            )));
        }
    }
    let graph = graph_json(fam, rooms, laid, &drawn, shape, seed, &areas);
    let mut checks = vec![
        Check {
            id: "SG-001",
            name: "script-runs",
            status: Status::Pass,
            detail: format!("all {} commands ran in a fresh project", script.lines.len()),
        },
        Check {
            id: "SG-002",
            name: "extracted-equals-generated",
            status: Status::Pass,
            detail: "the topology extractor finds the generated rooms, doors, open edges and entry".into(),
        },
    ];
    let mut rules = Vec::new();
    if let Some(pack_id) = &fam.pack_id {
        rules = rules_pass(pack_id, &graph).map_err(|e| Trouble::Plan(format!("the generated graph fails the {pack_id} rules: {e}")))?;
        let mut built = extracted.clone();
        let kinds: BTreeMap<&str, &str> = label.iter().zip(rooms).map(|((_, n), r)| (n.as_str(), fam.types[r.ty].kind.as_str())).collect();
        if let Some(nodes) = built["nodes"].as_array_mut() {
            for n in nodes {
                let kind = n["id"].as_str().and_then(|i| kinds.get(i)).copied();
                n["kind"] = json!(kind);
            }
        }
        built["archetype"] = json!(pack_id);
        rules_pass(pack_id, &built).map_err(|e| Trouble::Plan(format!("the built plan fails the {pack_id} rules: {e}")))?;
        checks.push(Check {
            id: "SG-003",
            name: "built-passes-pack",
            status: Status::Pass,
            detail: format!("{} rules pass on the generated graph and on the extracted one with the generated kinds", rules.len()),
        });
    }
    Ok(Done { drawn, script, replies, graph, rules, checks })
}

pub fn program_json(fam: &Family, rooms: &[Room], laid: &Laid, seed: u64, requested: i64, tol: i64) -> Value {
    let label = names(fam, rooms);
    let pack = fam.pack_id.as_deref().and_then(|p| Pack::embedded(p).ok());
    json!({
        "schema_version": SCHEMA_VERSION,
        "archetype": fam.name,
        "seed": seed,
        "rooms_requested": requested,
        "rooms_tolerance": tol,
        "pack_id": pack.map(|p| p.pack_id.clone()),
        "spaces": rooms.iter().enumerate().map(|(i, r)| {
            let ty = &fam.types[r.ty];
            json!({"id": r.id, "room_type": ty.key, "kind": ty.kind, "label": label[i].0, "role": ty.role, "area_m2_range": [ty.area.0, ty.area.1]})
        }).collect::<Vec<_>>(),
        "edges": laid.linked.links.iter().map(|(a, b, t)| json!({
            "from": rooms[*a].id, "to": rooms[*b].id, "kind": if t.is_open() { "open" } else { "door" }, "means": t.means(), "connection": t.id(),
        })).collect::<Vec<_>>(),
        "entry": laid.linked.entry.map(|(r, _)| rooms[r].id.as_str()),
    })
}

#[allow(clippy::too_many_arguments)]
pub fn building_json(
    fam: &Family,
    rooms: &[Room],
    laid: &Laid,
    d: &Drawn,
    shape: &str,
    seed: u64,
    topology_hash: &str,
    edges: usize,
) -> Value {
    let boundary: Vec<[i64; 2]> = outline(&laid.rects).map(|c| c.into_iter().map(|(x, y)| [x, y]).collect()).unwrap_or_default();
    let (width, depth) = (laid.rects.iter().map(|r| r.1 + r.3).max().unwrap_or(0), laid.rects.iter().map(|r| r.2 + r.4).max().unwrap_or(0));
    let by = by_room(&laid.rects);
    json!({
        "schema_version": SCHEMA_VERSION,
        "graph_kind": BUILDING_KIND,
        "archetype": fam.name,
        "seed": seed,
        "shell": shape,
        "units": "mm",
        "source_2d": {"file": "topology-graph.json", "topology_hash": topology_hash, "nodes": rooms.len(), "edges": edges},
        "footprint": {"width": width, "depth": depth, "boundary": boundary},
        "levels": [{"id": "level_1", "name": "Level 1", "elevation_mm": 0}],
        "spaces": rooms.iter().enumerate().map(|(i, r)| json!({
            "id": format!("space_{}", r.id), "source_2d": r.id, "level": "level_1",
            "bounds": by.get(&i).map(|v| v.iter().map(|b| json!({"x": b.0, "y": b.1, "width": b.2, "depth": b.3})).collect::<Vec<_>>()),
        })).collect::<Vec<_>>(),
        "walls": d.walls.iter().enumerate().map(|(i, w)| json!({
            "id": format!("wall_{}", i + 1), "kind": if w.exterior { "exterior" } else { "partition" },
            "type": if w.exterior { EXTERIOR } else { INTERIOR }, "start": w.a, "end": w.b,
        })).collect::<Vec<_>>(),
        "openings": d.doors.iter().enumerate().map(|(i, x)| json!({
            "id": format!("door_{}", i + 1), "kind": "door", "at": x.at, "family": x.family, "type": x.ty, "operation": x.op,
            "width_mm": x.width, "source_2d": rooms[x.rooms.0].id,
        })).collect::<Vec<_>>(),
        "vertical_circulation": [],
        "components": [],
        "systems": [],
    })
}
