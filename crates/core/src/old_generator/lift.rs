//! The lift: a plan's 2D topology made into the 3D/BIM graph. The topology says what is a room and what can be walked to; the
//! lift gives each room a level and a height, each wall its place and the rooms it bounds, and each door or window the wall it
//! stands in. Every object keeps the ids of the 2D it came from, so a reader can follow any wall or opening back to the plan.
//!
//! Walls are one to a BIMCraft wall element (one to a segment), so each can be named as a host. The script is compiled from
//! this graph, not from the plan.

use serde_json::{Value, json};

use super::graph::{BUILDING_KIND, SCHEMA_VERSION, Topology};
use super::layout::{EXTERIOR, INTERIOR, Plan};

pub const LEVEL_1: &str = "level_1";
pub const LEVEL_2: &str = "level_2";
pub const ROOF: &str = "level_roof";
/// The storey's height: `new` makes Level 2 here, and a wall with no top given is topped at it.
const STOREY_MM: i64 = 3000;
/// The level the script adds above the storey.
const ROOF_MM: i64 = 3600;

#[derive(Clone, Debug)]
pub struct Level {
    pub id: String,
    pub name: &'static str,
    pub elevation_mm: i64,
}

#[derive(Clone, Debug)]
pub struct Space {
    pub id: String,
    pub source_2d: String,
    pub level: String,
    pub base_mm: i64,
    pub top_mm: i64,
    /// `[x0, y0, x1, y1]` on wall centre lines.
    pub bounds: [i64; 4],
}

#[derive(Clone, Debug)]
pub struct Wall {
    pub id: String,
    pub kind: &'static str,
    pub wall_type: &'static str,
    pub level: String,
    pub top_level: String,
    pub base_mm: i64,
    pub top_mm: i64,
    pub thickness_mm: i64,
    pub start: [i64; 2],
    pub end: [i64; 2],
    /// The 2D spaces this wall bounds.
    pub source_2d_refs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Opening {
    pub id: String,
    pub kind: &'static str,
    pub host: String,
    pub at: [i64; 2],
    pub source_2d: String,
    /// The topology edge a room's door is the 3D of; the way in, the exits and the windows have none.
    pub source_edge: Option<String>,
    pub entry: bool,
    /// What it is for: `main_entry`, `back_door`, `service_exit`, `room_access` or `window`.
    pub role: &'static str,
    /// The symbol it is drawn with, and the BIMCraft type the script makes for it.
    pub symbol: &'static str,
    pub type_name: String,
    pub width_mm: i64,
    pub height_mm: i64,
    pub sill_mm: i64,
    pub operation: &'static str,
    pub flip_hand: bool,
}

#[derive(Clone, Debug)]
pub struct Slab {
    pub id: String,
    pub level: String,
    pub boundary: Vec<[i64; 2]>,
    pub source_2d_refs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Building {
    pub archetype: &'static str,
    pub seed: u64,
    pub layout_family: &'static str,
    pub layout_strategy: &'static str,
    pub boundary_shape: &'static str,
    pub opening_symbol_profile: &'static str,
    pub width: i64,
    pub depth: i64,
    pub boundary: Vec<[i64; 2]>,
    pub levels: Vec<Level>,
    pub spaces: Vec<Space>,
    pub walls: Vec<Wall>,
    pub openings: Vec<Opening>,
    pub slabs: Vec<Slab>,
}

fn thickness_of(wall_type: &str) -> i64 {
    if wall_type == EXTERIOR {
        300
    } else if wall_type == INTERIOR {
        100
    } else {
        0
    }
}

/// How far two spans overlap along a line, 0 when they only touch.
fn overlap(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0)
}

/// Whether a wall segment runs along an edge of the rectangle, for some length.
fn bounds_along(a: [i64; 2], b: [i64; 2], r: [i64; 4]) -> bool {
    let [x0, y0, x1, y1] = r;
    if a[0] == b[0] {
        (a[0] == x0 || a[0] == x1) && overlap((a[1].min(b[1]), a[1].max(b[1])), (y0, y1)) > 0
    } else if a[1] == b[1] {
        (a[1] == y0 || a[1] == y1) && overlap((a[0].min(b[0]), a[0].max(b[0])), (x0, x1)) > 0
    } else {
        false
    }
}

fn on_segment(p: [i64; 2], a: [i64; 2], b: [i64; 2]) -> bool {
    let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let within = |lo: i64, hi: i64, v: i64| v >= lo.min(hi) && v <= lo.max(hi);
    cross == 0 && within(a[0], b[0], p[0]) && within(a[1], b[1], p[1])
}

fn exterior_id(plan: &Plan, index: usize, a: [i64; 2], b: [i64; 2]) -> String {
    match (a, b) {
        ([0, _], [0, _]) => "wall_ext_west".to_string(),
        ([x, _], [x2, _]) if x == x2 && x == plan.width => "wall_ext_east".into(),
        ([_, 0], [_, 0]) => "wall_ext_south".into(),
        ([_, y], [_, y2]) if y == y2 && y == plan.depth => "wall_ext_north".into(),
        _ => format!("wall_ext_{index}"),
    }
}

impl Building {
    /// The 3D graph of a plan and its topology. A part of the plan that can't be placed is left unplaced (an unnamed wall, a
    /// room with no space) for the checks on the lift to find, rather than passed over.
    pub fn lift(plan: &Plan, topology: &Topology) -> Building {
        let levels = vec![
            Level { id: LEVEL_1.into(), name: "Level 1", elevation_mm: 0 },
            Level { id: LEVEL_2.into(), name: "Level 2", elevation_mm: STOREY_MM },
            Level { id: ROOF.into(), name: "Roof", elevation_mm: ROOF_MM },
        ];
        let rect = |id: &str| plan.rect_of(id);
        let spaces: Vec<Space> = topology
            .nodes
            .iter()
            .filter_map(|n| {
                Some(Space {
                    id: format!("space_{}", n.id),
                    source_2d: n.id.clone(),
                    level: LEVEL_1.into(),
                    base_mm: 0,
                    top_mm: STOREY_MM,
                    bounds: rect(&n.id)?,
                })
            })
            .collect();

        let mut walls = Vec::new();
        for w in &plan.walls {
            let mut points = w.points.clone();
            if let (true, Some(&first)) = (w.closed, w.points.first()) {
                points.push(first);
            }
            let wall_type = if w.kind == "exterior" { EXTERIOR } else { INTERIOR };
            for (seg_index, seg) in points.windows(2).enumerate() {
                let [a, b] = [seg[0], seg[1]];
                let id = if w.kind == "exterior" {
                    exterior_id(plan, seg_index, a, b)
                } else {
                    plan.inner_wall_id(w.kind, a, b).unwrap_or_else(|| format!("wall_unplaced_{}", walls.len()))
                };
                let source_2d_refs =
                    topology.nodes.iter().filter(|n| rect(&n.id).is_some_and(|r| bounds_along(a, b, r))).map(|n| n.id.clone()).collect();
                walls.push(Wall {
                    id,
                    kind: w.kind,
                    wall_type,
                    level: LEVEL_1.into(),
                    top_level: LEVEL_2.into(),
                    base_mm: 0,
                    top_mm: STOREY_MM,
                    thickness_mm: thickness_of(wall_type),
                    start: a,
                    end: b,
                    source_2d_refs,
                });
            }
        }

        let (mut openings, mut windows) = (Vec::new(), 0);
        for o in &plan.openings {
            let (host, id, source_edge) = match (o.kind, o.role) {
                ("door", "main_entry") => (exterior_host(&walls, o.at), "door_entry".to_string(), None),
                ("door", "back_door" | "service_exit") => {
                    (exterior_host(&walls, o.at), format!("door_{}_{}", o.role.trim_end_matches("_door"), o.space), None)
                }
                ("door", _) => (plan.corridor_wall_of(&o.space).to_string(), format!("door_{}", o.space), door_edge(topology, &o.space)),
                _ => {
                    windows += 1;
                    (exterior_host(&walls, o.at), format!("window_{windows}_{}", o.space), None)
                }
            };
            openings.push(Opening {
                id,
                kind: o.kind,
                host,
                at: o.at,
                source_2d: o.space.clone(),
                source_edge,
                entry: o.entry,
                role: o.role,
                symbol: o.symbol,
                type_name: o.type_name(),
                width_mm: o.width_mm,
                height_mm: o.height_mm,
                sill_mm: o.sill_mm,
                operation: o.operation,
                flip_hand: o.flip_hand,
            });
        }

        let (w, d) = (plan.width, plan.depth);
        let slabs = vec![Slab {
            id: "slab_level_1".into(),
            level: LEVEL_1.into(),
            boundary: plan.boundary.clone(),
            source_2d_refs: topology.nodes.iter().map(|n| n.id.clone()).collect(),
        }];
        Building {
            archetype: plan.archetype.name(),
            seed: plan.seed,
            layout_family: plan.layout_family.name(),
            layout_strategy: plan.layout_strategy.name(),
            boundary_shape: plan.boundary_shape.name(),
            opening_symbol_profile: plan.opening_symbol_profile.name(),
            width: w,
            depth: d,
            boundary: plan.boundary.clone(),
            levels,
            spaces,
            walls,
            openings,
            slabs,
        }
    }

    pub fn level(&self, id: &str) -> Option<&Level> {
        self.levels.iter().find(|l| l.id == id)
    }

    pub fn wall(&self, id: &str) -> Option<&Wall> {
        self.walls.iter().find(|w| w.id == id)
    }

    /// The file: every object with the 2D it came from; `topology_hash` names the topology file this was lifted from.
    pub fn json(&self, topology_hash: &str, nodes: usize, edges: usize) -> Value {
        json!({
            "schema_version": SCHEMA_VERSION,
            "graph_kind": BUILDING_KIND,
            "archetype": self.archetype,
            "seed": self.seed,
            "layout_family": self.layout_family,
            "layout_strategy": self.layout_strategy,
            "boundary_shape": self.boundary_shape,
            "opening_symbol_profile": self.opening_symbol_profile,
            "units": "mm",
            "source_2d": {"file": "topology-graph.json", "topology_hash": topology_hash, "nodes": nodes, "edges": edges},
            "footprint": {"width": self.width, "depth": self.depth, "boundary": self.boundary},
            "levels": self.levels.iter().map(|l| json!({"id": l.id, "name": l.name, "elevation_mm": l.elevation_mm})).collect::<Vec<_>>(),
            "spaces": self.spaces.iter().map(|s| json!({
                "id": s.id, "source_2d": s.source_2d, "level": s.level, "base_mm": s.base_mm, "top_mm": s.top_mm,
                "bounds": {"x0": s.bounds[0], "y0": s.bounds[1], "x1": s.bounds[2], "y1": s.bounds[3]},
            })).collect::<Vec<_>>(),
            "walls": self.walls.iter().map(|w| json!({
                "id": w.id, "kind": w.kind, "type": w.wall_type, "level": w.level, "top_level": w.top_level,
                "base_mm": w.base_mm, "top_mm": w.top_mm, "thickness_mm": w.thickness_mm,
                "start": w.start, "end": w.end, "source_2d_refs": w.source_2d_refs,
            })).collect::<Vec<_>>(),
            "openings": self.openings.iter().map(|o| json!({
                "id": o.id, "kind": o.kind, "host": o.host, "at": o.at, "entry": o.entry,
                "role": o.role, "symbol": o.symbol, "type": o.type_name,
                "width_mm": o.width_mm, "height_mm": o.height_mm, "sill_mm": o.sill_mm,
                "operation": o.operation, "flip_hand": o.flip_hand,
                "source_2d": o.source_2d, "source_topology_edge": o.source_edge,
            })).collect::<Vec<_>>(),
            "slabs": self.slabs.iter().map(|s| json!({
                "id": s.id, "kind": "floor", "level": s.level, "boundary": s.boundary, "source_2d_refs": s.source_2d_refs,
            })).collect::<Vec<_>>(),
            "vertical_circulation": [],
            "components": [],
            "systems": [],
        })
    }
}

fn exterior_host(walls: &[Wall], at: [i64; 2]) -> String {
    walls
        .iter()
        .find(|w| w.kind == "exterior" && on_segment(at, w.start, w.end))
        .map(|w| w.id.clone())
        .unwrap_or_else(|| "wall_unplaced_opening".into())
}

/// The door edge of a room to the corridor, by its id.
fn door_edge(topology: &Topology, space: &str) -> Option<String> {
    topology.edges.iter().find(|e| e.kind == "door" && e.from == space).map(|e| e.id())
}
