//! The graphs a plan is made from and the building as data: the program (what the building must hold), the topology (what
//! can be walked from where) and the placed building, each as JSON a person or another program can read.
//!
//! Only `door` and `open` edges are access. `adjacent` and `near` are softer relations that no rule reads.

use super::program::Role;

pub const SCHEMA_VERSION: u32 = 1;
/// What the topology file says it is, and what the building file says it is.
pub const TOPOLOGY_KIND: &str = "2d_topology";
pub const BUILDING_KIND: &str = "3d_bim";

/// The kinds of edge that mean one can walk from one space to the other.
pub const ACCESS: [&str; 2] = ["door", "open"];

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub role: Role,
    pub area_m2: f64,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: &'static str,
}

impl Edge {
    /// What a 3D object that came of this edge names it by: stable, and made of what the edge is.
    pub fn id(&self) -> String {
        format!("{}:{}:{}", self.kind, self.from, self.to)
    }
}

#[derive(Clone, Debug)]
pub struct Topology {
    /// The space the building is entered from outside through.
    pub entry: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Topology {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn access(&self) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(|e| ACCESS.contains(&e.kind))
    }
}
