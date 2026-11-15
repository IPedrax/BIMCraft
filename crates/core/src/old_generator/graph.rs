//! The corridor plan's graphs as data: the topology a plan makes and the program and topology files it writes. The types are
//! the shared ones in `crate::generate::graph`.

use serde_json::{Value, json};

pub use crate::generate::graph::*;

use super::layout::{Band, Plan, mm2_to_m2};

impl Topology {
    pub fn of(plan: &Plan) -> Topology {
        let mut nodes = vec![Node {
            id: plan.program.corridor.id.clone(),
            kind: plan.program.corridor.kind.to_string(),
            role: plan.program.corridor.role,
            area_m2: mm2_to_m2(plan.corridor_area_mm2()),
        }];
        nodes.extend(plan.cells.iter().map(|c| Node {
            id: c.spec.id.clone(),
            kind: c.spec.kind.to_string(),
            role: c.spec.role,
            area_m2: mm2_to_m2(c.area_mm2()),
        }));
        let corridor = plan.program.corridor.id.clone();
        let mut edges: Vec<Edge> =
            plan.cells.iter().map(|c| Edge { from: c.spec.id.clone(), to: corridor.clone(), kind: "door" }).collect();
        for band in [Band::South, Band::North] {
            let cells: Vec<_> = plan.band(band).collect();
            edges.extend(cells.windows(2).filter_map(|pair| match pair {
                [a, b] => Some(Edge { from: a.spec.id.clone(), to: b.spec.id.clone(), kind: "adjacent" }),
                _ => None,
            }));
        }
        Topology { entry: plan.program.lobby.id.clone(), nodes, edges }
    }

    pub fn json(&self, plan: &Plan) -> Value {
        json!({
            "schema_version": SCHEMA_VERSION,
            "graph_kind": TOPOLOGY_KIND,
            "archetype": plan.archetype.name(),
            "seed": plan.seed,
            "layout_family": plan.layout_family.name(),
            "layout_strategy": plan.layout_strategy.name(),
            "boundary_shape": plan.boundary_shape.name(),
            "opening_symbol_profile": plan.opening_symbol_profile.name(),
            "entry": self.entry,
            "nodes": self.nodes.iter().map(|n| json!({
                "id": n.id, "kind": n.kind, "role": n.role.name(), "area_m2": n.area_m2, "level": "Level 1",
            })).collect::<Vec<_>>(),
            "edges": self.edges.iter().map(|e| {
                let mut edge = json!({"id": e.id(), "from": e.from, "to": e.to, "kind": e.kind});
                if ACCESS.contains(&e.kind)
                    && let Some(o) = plan.openings.iter().find(|o| o.kind == "door" && o.role == "room_access" && o.space == e.from)
                {
                    edge["opening_symbol"] = json!(o.symbol);
                    edge["opening_role"] = json!(o.role);
                    edge["opening_type"] = json!(o.type_name());
                    edge["width_mm"] = json!(o.width_mm);
                }
                edge
            }).collect::<Vec<_>>(),
        })
    }
}

/// What the building must hold, before any of it is placed.
pub fn program_json(plan: &Plan) -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "archetype": plan.archetype.name(),
        "seed": plan.seed,
        "layout_family": plan.layout_family.name(),
        "layout_strategy": plan.layout_strategy.name(),
        "boundary_shape": plan.boundary_shape.name(),
        "opening_symbol_profile": plan.opening_symbol_profile.name(),
        "requested_area_m2": plan.requested_m2,
        "spaces": plan.program.all().map(|s| json!({
            "id": s.id, "kind": s.kind, "label": s.label, "role": s.role.name(),
            "min_area_m2": s.min_m2, "target_area_m2": s.target_m2, "optional": false,
        })).collect::<Vec<_>>(),
        "edges": plan.cells.iter().map(|c| json!({"from": c.spec.id, "to": plan.program.corridor.id, "kind": "access_required"})).collect::<Vec<_>>(),
    })
}
