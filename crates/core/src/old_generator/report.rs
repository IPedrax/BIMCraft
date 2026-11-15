//! The corridor plan's report: the rules on the lift and on what BIMCraft made of the script, and the report file. A failed
//! rule means no files are written, so a report on disk holds passes and warnings only. The checks and the rules on the
//! access graph are the shared ones in `crate::generate::report`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::geom::P2;
use crate::ids::Id;
use crate::model::{Element, Model};

pub use crate::generate::report::*;

use super::graph::{SCHEMA_VERSION, Topology};
use super::layout::Plan;
use super::lift::Building;
use super::openings::{self, DOOR_SYMBOLS, WINDOW_SYMBOLS};
use super::script::{REQUIRED, Script};

pub const RULE_SET: &str = "slice1-hard-rules/1";
/// The least commands a generated script holds: a plan with its walls, doors, rooms and annotation is never shorter.
pub const MIN_COMMANDS: usize = 30;
/// How far from the requested area the plan may land before the report warns.
const AREA_TOLERANCE: f64 = 0.05;

/// How many openings of each symbol a plan has, doors and windows apart: what the manifest and the report say.
pub fn opening_symbols(plan: &Plan) -> Value {
    let (doors, windows) = openings::symbol_counts(plan.openings.iter().map(|o| (o.kind, o.symbol)));
    json!({"doors": doors, "windows": windows})
}

/// The fewest walls a lifted building holds: four outside walls make a building.
const MIN_WALLS: usize = 4;

/// The rules on the lift from the 2D topology to the 3D graph: each 2D space, wall and opening is carried over whole and
/// names what it came from. `LIFT-005` and `LIFT-006` are what BIMCraft made of it, in `run_checks`.
pub fn lift_checks(b: &Building, t: &Topology) -> Vec<Check> {
    let level_ok = |id: &str| b.level(id).is_some();
    let node_ok = |id: &str| t.node(id).is_some();

    let mut spaces: Vec<String> = Vec::new();
    for n in &t.nodes {
        match b.spaces.iter().filter(|s| s.source_2d == n.id).count() {
            1 => {}
            0 => spaces.push(format!("the 2D space `{}` is not lifted to a 3D space", n.id)),
            k => spaces.push(format!("the 2D space `{}` is lifted to {k} 3D spaces", n.id)),
        }
    }
    for s in &b.spaces {
        if !node_ok(&s.source_2d) {
            spaces.push(format!("the 3D space `{}` comes of `{}`, which is no 2D space", s.id, s.source_2d));
        }
        if !level_ok(&s.level) {
            spaces.push(format!("the 3D space `{}` is on level `{}`, which the building does not have", s.id, s.level));
        }
        if s.base_mm >= s.top_mm {
            spaces.push(format!("the 3D space `{}` has its top ({} mm) at or under its base ({} mm)", s.id, s.top_mm, s.base_mm));
        }
    }
    let lifted = check(
        "LIFT-001",
        "every-2d-space-lifted",
        spaces,
        format!("all {} 2D spaces are lifted to a 3D space on a level, with a height", t.nodes.len()),
    );

    let mut walls: Vec<String> = Vec::new();
    if b.walls.len() < MIN_WALLS {
        walls.push(format!("the building has {} walls, fewer than {MIN_WALLS}", b.walls.len()));
    }
    let mut seen = BTreeSet::new();
    for w in &b.walls {
        if !seen.insert(w.id.as_str()) {
            walls.push(format!("two walls are called `{}`", w.id));
        }
        if w.source_2d_refs.is_empty() {
            walls.push(format!("the wall `{}` names no 2D space it bounds", w.id));
        }
        for r in w.source_2d_refs.iter().filter(|r| !node_ok(r)) {
            walls.push(format!("the wall `{}` bounds `{r}`, which is no 2D space", w.id));
        }
        if !level_ok(&w.level) || !level_ok(&w.top_level) {
            walls.push(format!(
                "the wall `{}` stands between levels `{}` and `{}`, which the building does not both have",
                w.id, w.level, w.top_level
            ));
        }
        if w.base_mm >= w.top_mm {
            walls.push(format!("the wall `{}` has its top ({} mm) at or under its base ({} mm)", w.id, w.top_mm, w.base_mm));
        }
        if w.thickness_mm <= 0 || w.wall_type.is_empty() {
            walls.push(format!("the wall `{}` has no type or thickness", w.id));
        }
    }
    let walls = check(
        "LIFT-002",
        "walls-carry-provenance",
        walls,
        format!("all {} walls name the 2D spaces they bound, a level, a height and a thickness", b.walls.len()),
    );

    let mut openings: Vec<String> = Vec::new();
    if b.openings.is_empty() {
        openings.push("the building has no doors or windows".into());
    }
    let mut seen = BTreeSet::new();
    for o in &b.openings {
        if !seen.insert(o.id.as_str()) {
            openings.push(format!("two openings are called `{}`", o.id));
        }
        match b.wall(&o.host) {
            None => openings.push(format!("the {} `{}` is hosted by `{}`, which is no wall", o.kind, o.id, o.host)),
            Some(w) if !on_segment(o.at, w.start, w.end) => {
                openings.push(format!("the {} `{}` at {:?} is not on its host wall `{}`", o.kind, o.id, o.at, w.id));
            }
            Some(_) => {}
        }
        if !node_ok(&o.source_2d) {
            openings.push(format!("the {} `{}` comes of `{}`, which is no 2D space", o.kind, o.id, o.source_2d));
        }
        let known = if o.kind == "door" { DOOR_SYMBOLS.contains(&o.symbol) } else { WINDOW_SYMBOLS.contains(&o.symbol) };
        if !known {
            openings.push(format!("the {} `{}` has the symbol `{}`, which no {} has", o.kind, o.id, o.symbol, o.kind));
        }
        if o.width_mm <= 0 || o.height_mm <= 0 || o.sill_mm < 0 || o.type_name.is_empty() {
            openings.push(format!("the {} `{}` has no size or type", o.kind, o.id));
        }
        match (&o.source_edge, o.kind == "door" && o.role == "room_access") {
            (Some(e), _) if !t.access().any(|a| a.id() == *e && (a.from == o.source_2d || a.to == o.source_2d)) => {
                openings.push(format!("the {} `{}` comes of the edge `{e}`, which is no door or open edge of its space", o.kind, o.id));
            }
            (None, true) => openings.push(format!("the door `{}` of `{}` names no topology edge", o.id, o.source_2d)),
            _ => {}
        }
    }
    let openings = check(
        "LIFT-003",
        "openings-hosted",
        openings,
        format!("all {} openings stand on a wall of the building and name their 2D source", b.openings.len()),
    );

    let mut slabs: Vec<String> = Vec::new();
    if b.slabs.is_empty() {
        slabs.push("the building has no slab".into());
    }
    for s in &b.slabs {
        if !level_ok(&s.level) {
            slabs.push(format!("the slab `{}` is on level `{}`, which the building does not have", s.id, s.level));
        }
        if s.boundary.len() < 3 {
            slabs.push(format!("the slab `{}` has no outline", s.id));
        }
    }
    let slabs = check("LIFT-004", "slabs-on-levels", slabs, format!("all {} slabs are on a level of the building", b.slabs.len()));
    vec![lifted, walls, openings, slabs]
}

/// Whether `p` is on the straight segment from `a` to `b`, to the millimetre.
fn on_segment(p: [i64; 2], a: [i64; 2], b: [i64; 2]) -> bool {
    let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let length = (((b[0] - a[0]).pow(2) + (b[1] - a[1]).pow(2)) as f64).sqrt();
    let within = |lo: i64, hi: i64, v: i64| v >= lo.min(hi) && v <= lo.max(hi);
    length > 0.0 && (cross as f64 / length).abs() <= 1.0 && within(a[0], b[0], p[0]) && within(a[1], b[1], p[1])
}

/// The wall elements BIMCraft made, by the id of the building's wall each command was for.
fn made_walls<'a>(script: &'a Script, replies: &[Value]) -> BTreeMap<&'a str, Id> {
    script.lines.iter().zip(replies).filter_map(|(l, r)| Some((l.wall_of.as_deref()?, r.get("ids")?.get(0)?.as_u64()?))).collect()
}

/// LIFT-005: each door and window is in the wall the building says, as BIMCraft hosted it when the script ran.
fn hosting_check(building: &Building, script: &Script, replies: &[Value]) -> Check {
    let walls = made_walls(script, replies);
    let mut problems = Vec::new();
    let mut made: BTreeSet<&str> = BTreeSet::new();
    for (l, r) in script.lines.iter().zip(replies) {
        let Some(id) = l.opening_of.as_deref() else { continue };
        made.insert(id);
        let Some(o) = building.openings.iter().find(|o| o.id == id) else {
            problems.push(format!("the script makes `{id}`, which the building does not have"));
            continue;
        };
        let Some(wanted) = walls.get(o.host.as_str()) else {
            problems.push(format!("`{id}` is hosted by `{}`, which the script does not make", o.host));
            continue;
        };
        match r.get("host").and_then(Value::as_u64) {
            Some(got) if got == *wanted => {}
            Some(got) => problems.push(format!("`{id}` is in the wall {got} in BIMCraft, not in `{}` ({wanted})", o.host)),
            None => problems.push(format!("BIMCraft hosted `{id}` in no wall, not in `{}`", o.host)),
        }
    }
    problems.extend(
        building.openings.iter().filter(|o| !made.contains(o.id.as_str())).map(|o| format!("`{}` is not made by the script", o.id)),
    );
    check(
        "LIFT-005",
        "openings-hosted-as-lifted",
        problems,
        format!("all {} openings are in the walls the building names, as BIMCraft hosts them", building.openings.len()),
    )
}

/// LIFT-007: each door and window is in BIMCraft with the type, size and operation its symbol gives it in the building.
fn symbol_check(building: &Building, script: &Script, replies: &[Value], model: &Model) -> Check {
    let mut problems = Vec::new();
    for (l, r) in script.lines.iter().zip(replies) {
        let Some(id) = l.opening_of.as_deref() else { continue };
        let Some(o) = building.openings.iter().find(|o| o.id == id) else { continue };
        let Some(element) = r.get("id").and_then(Value::as_u64).and_then(|e| model.elements.get(e)) else {
            problems.push(format!("`{id}` is not in the project"));
            continue;
        };
        let (Element::Door(made) | Element::Window(made)) = element else {
            problems.push(format!("`{id}` is no door or window in the project"));
            continue;
        };
        let types = if o.kind == "door" { &model.types.door } else { &model.types.window };
        let Some(t) = types.get(&made.type_name) else {
            problems.push(format!("`{id}` has the type `{}`, which the project does not have", made.type_name));
            continue;
        };
        let operation = t.operation.map(|op| op.name());
        let mut differs = Vec::new();
        if made.type_name != o.type_name {
            differs.push(format!("type `{}` for `{}`", made.type_name, o.type_name));
        }
        if t.width.round() as i64 != o.width_mm || t.height.round() as i64 != o.height_mm {
            differs.push(format!("{} x {} mm for {} x {} mm", t.width, t.height, o.width_mm, o.height_mm));
        }
        if o.kind == "window" && made.sill.round() as i64 != o.sill_mm {
            differs.push(format!("sill {} mm for {} mm", made.sill, o.sill_mm));
        }
        if operation != Some(o.operation) {
            differs.push(format!("operation {operation:?} for {:?}", o.operation));
        }
        if made.flip_hand != o.flip_hand {
            differs.push(format!("flip_hand {} for {}", made.flip_hand, o.flip_hand));
        }
        if !differs.is_empty() {
            problems.push(format!("`{id}` is in BIMCraft as {}", differs.join(", ")));
        }
    }
    check(
        "LIFT-007",
        "opening-symbols-as-lifted",
        problems,
        format!("all {} openings are in BIMCraft with the type, size and operation of their symbol", building.openings.len()),
    )
}

/// LIFT-006: BIMCraft's walls and levels are the building's: the same walls, the same place, the same heights.
fn matching_check(building: &Building, script: &Script, replies: &[Value], model: &Model) -> Check {
    let walls = made_walls(script, replies);
    let mut problems = Vec::new();
    for l in script.lines.iter().filter_map(|l| l.wall_of.as_deref()) {
        if !building.walls.iter().any(|w| w.id == l) {
            problems.push(format!("the script makes the wall `{l}`, which the building does not have"));
        }
    }
    let wall_elements = model.elements.values().filter(|e| matches!(e, Element::Wall(_))).count();
    if wall_elements != building.walls.len() {
        problems.push(format!("BIMCraft has {wall_elements} walls, the building {}", building.walls.len()));
    }
    let level = |id: &str| building.levels.iter().find(|l| l.id == id);
    for w in &building.walls {
        let Some(&element) = walls.get(w.id.as_str()) else {
            problems.push(format!("the wall `{}` is not made by the script", w.id));
            continue;
        };
        let Some(made) = model.wall(element) else {
            problems.push(format!("the wall `{}` is not in the project", w.id));
            continue;
        };
        let at = |p: P2| [p.0.round() as i64, p.1.round() as i64];
        let mut differs = Vec::new();
        if made.type_name != w.wall_type || model.thickness(made).round() as i64 != w.thickness_mm {
            differs.push(format!("type {} ({} mm) for {} ({} mm)", made.type_name, model.thickness(made), w.wall_type, w.thickness_mm));
        }
        if at(made.start) != w.start || at(made.end) != w.end {
            differs.push(format!("from {:?} to {:?} for {:?} to {:?}", at(made.start), at(made.end), w.start, w.end));
        }
        if model.own_base(made).round() as i64 != w.base_mm || model.own_top(made).round() as i64 != w.top_mm {
            differs.push(format!("{} to {} mm for {} to {} mm", model.own_base(made), model.own_top(made), w.base_mm, w.top_mm));
        }
        let base = model.level(made.level).map(|l| (l.name.as_str(), l.elevation.round() as i64));
        if base != level(&w.level).map(|l| (l.name, l.elevation_mm)) {
            differs.push(format!("base level {base:?}"));
        }
        let top = made.top_level.and_then(|t| model.level(t)).map(|l| (l.name.as_str(), l.elevation.round() as i64));
        if top != level(&w.top_level).map(|l| (l.name, l.elevation_mm)) {
            differs.push(format!("top level {top:?}"));
        }
        if !differs.is_empty() {
            problems.push(format!("the wall `{}` is in BIMCraft as {}", w.id, differs.join(", ")));
        }
    }
    for l in &building.levels {
        let found = model.levels().iter().any(|(_, m)| m.name == l.name && m.elevation.round() as i64 == l.elevation_mm);
        if !found {
            problems.push(format!("the level `{}` at {} mm is not in the project", l.name, l.elevation_mm));
        }
    }
    check(
        "LIFT-006",
        "ir-matches-bimcraft",
        problems,
        format!("BIMCraft holds the building's {} walls and {} levels as the graph has them", building.walls.len(), building.levels.len()),
    )
}

/// The rules on the script and on what BIMCraft did with it. `replies` are the commands' answers, line for line.
pub fn run_checks(plan: &Plan, building: &Building, script: &Script, replies: &[Value], model: &Model) -> Vec<Check> {
    let ids: BTreeSet<&str> = script.lines.iter().map(|l| l.id).collect();
    let mut problems: Vec<String> =
        REQUIRED.iter().filter(|id| !ids.contains(*id)).map(|id| format!("the script has no `{id}` command")).collect();
    if script.lines.len() < MIN_COMMANDS {
        problems.push(format!("the script has {} commands, fewer than {MIN_COMMANDS}", script.lines.len()));
    }
    let commands = check("SC-001", "script-commands", problems, format!("{} commands, every required one among them", script.lines.len()));
    let own: Vec<String> = script
        .lines
        .iter()
        .filter(|l| l.id == "generate" || l.params.get("path").is_some())
        .map(|l| format!("`{}` takes a file or runs the generator, which a script must not", l.id))
        .collect();
    let closed = check("SC-002", "script-is-self-contained", own, "no command names a file or runs the generator".into());
    let ran = check(
        "RT-001",
        "script-runs",
        if replies.len() == script.lines.len() {
            Vec::new()
        } else {
            vec![format!("{} of {} commands ran", replies.len(), script.lines.len())]
        },
        format!("all {} commands ran in a fresh project", script.lines.len()),
    );
    let unbounded: Vec<String> = script
        .lines
        .iter()
        .zip(replies)
        .filter_map(|(l, r)| {
            let space = l.room_of.as_ref()?;
            let net = r.get("area_m2").and_then(Value::as_f64).unwrap_or(0.0);
            match r.get("warning") {
                Some(w) => Some(format!("the room of `{space}` is unbounded: {}", w.as_str().unwrap_or_default())),
                None if net <= 0.0 => Some(format!("the room of `{space}` has no area")),
                None => None,
            }
        })
        .collect();
    let rooms = check("RT-002", "rooms-bounded", unbounded, format!("all {} rooms are bounded by walls", plan.cells.len() + 1));
    let valid =
        check("RT-003", "model-valid", model.check().err().into_iter().collect(), "the project passes the model's own check".into());
    let hosting = hosting_check(building, script, replies);
    let matching = matching_check(building, script, replies, model);
    let symbols = symbol_check(building, script, replies, model);
    let deviation = (plan.gross_m2() - plan.requested_m2).abs() / plan.requested_m2.max(f64::MIN_POSITIVE);
    let area = if deviation <= AREA_TOLERANCE {
        Check {
            id: "GM-001",
            name: "area-fidelity",
            status: Status::Pass,
            detail: format!("{} m2 gross for {} m2 asked", plan.gross_m2(), plan.requested_m2),
        }
    } else {
        Check {
            id: "GM-001",
            name: "area-fidelity",
            status: Status::Warn,
            detail: format!(
                "{} m2 gross for {} m2 asked: the plan's depth is held between 12 and 24 m",
                plan.gross_m2(),
                plan.requested_m2
            ),
        }
    };
    vec![commands, closed, ran, rooms, valid, hosting, matching, symbols, area]
}

/// The report: checks, warnings, what the generator could not do, and scores a ranking can start from.
pub fn json(plan: &Plan, checks: &[Check], script: &Script, replies: &[Value]) -> Value {
    let count = |s: Status| checks.iter().filter(|c| c.status == s).count();
    let measured: Vec<Value> = script
        .lines
        .iter()
        .zip(replies)
        .filter_map(|(l, r)| Some(json!({"space": l.room_of.as_ref()?, "net_area_m2": r.get("area_m2")?})))
        .collect();
    let net: f64 = measured.iter().filter_map(|m| m["net_area_m2"].as_f64()).sum();
    let required = rules_of(plan.archetype).map_or(0, |r| r.groups.len());
    let passing = checks.iter().filter(|c| c.status != Status::Fail).count();
    json!({
        "schema_version": SCHEMA_VERSION,
        "archetype": plan.archetype.name(),
        "seed": plan.seed,
        "layout_family": plan.layout_family.name(),
        "layout_strategy": plan.layout_strategy.name(),
        "boundary_shape": plan.boundary_shape.name(),
        "opening_symbol_profile": plan.opening_symbol_profile.name(),
        "opening_symbols": opening_symbols(plan),
        "rule_set": RULE_SET,
        "summary": {"pass": count(Status::Pass), "warn": count(Status::Warn), "fail": count(Status::Fail)},
        "checks": checks.iter().map(|c| json!({"id": c.id, "name": c.name, "status": c.status.name(), "detail": c.detail})).collect::<Vec<_>>(),
        "warnings": checks.iter().filter(|c| c.status == Status::Warn).map(|c| format!("{}: {}", c.id, c.detail)).collect::<Vec<_>>(),
        "capability_notes": [
            "Areas in the topology and building graphs are gross, measured to wall centre lines; `measured_rooms` holds the net areas BIMCraft computed for each room.",
            "One storey with seeded corridor-bar, central-commons, or clustered-commons layout families; rectangle, L-shaped, or stepped-bar footprints; and double-loaded, single-loaded or offset corridor strategies. No stairs, shafts, structure or MEP.",
            "Every room has one door, to the corridor; the lobby is also the building's entry, and one or two back-of-house rooms have an exit to outside. Doors and windows take their symbol (`opening_symbol_profile`, `opening_symbols`) from the seed: each symbol is a BIMCraft door or window type with its own width, height, sill and operation, so the drawing shows the width and the swing of each; BIMCraft's plan symbols do not draw a double leaf or a window style apart from its width.",
            "The rules are hard-coded in this slice (rule set slice1-hard-rules/1); versioned rule packs are future work.",
        ],
        "scores": {
            "rule_checks_passing": format!("{passing}/{}", checks.len()),
            "program_groups_required": required,
            "room_count": plan.cells.len() + 1,
            "net_to_gross": if plan.gross_m2() > 0.0 { (net / plan.gross_m2() * 1000.0).round() / 1000.0 } else { 0.0 },
        },
        "measured_rooms": measured,
    })
}

pub fn failures(checks: &[Check]) -> Vec<&Check> {
    checks.iter().filter(|c| c.status == Status::Fail).collect()
}
