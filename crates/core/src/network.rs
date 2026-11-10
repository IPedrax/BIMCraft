//! The pipe network, worked out from the model and never stored: where pipes' ends and the pipe connectors of fixtures, fittings
//! and valves meet, which pipes join which, which ends are open and what looks wrong. Like wall joins, it is rebuilt from the
//! model each time it is asked for, so it can't go stale and needs no rules for renaming or deleting. A drawing in progress has
//! open ends and odd joins, so nothing here refuses a model: what looks wrong is a warning, as Revit's system checks are.
//!
//! Points within `tolerance` of each other are one node. A pipe's end on another pipe's length (a branch drawn to its centre
//! line, with no fitting) is a node too, splitting that pipe in two edges without splitting the element. A fitting's or a valve's
//! connectors are joined through it, as a fitting's connectors are linked in Revit so a system runs through it; a fixture's are
//! not, since its waste and its water are networks of their own.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::family::{Category, ConnectorKind, Domain};
use crate::ids::{Guid, Id};
use crate::model::{Element, Model};

/// A pipe: its id, its centre line's ends, its width, its system, and where its ends are among the points.
type Pipe = (Id, [f64; 3], [f64; 3], f64, String, usize);
/// A group being gathered: its elements, systems, domains and nodes.
type Gathered = (BTreeSet<Id>, BTreeSet<String>, BTreeSet<Domain>, Vec<usize>);

/// How close two points must be to be one node, mm, unless a query says otherwise.
pub const TOLERANCE: f64 = 1.0;
/// The largest tolerance a query may ask for, mm.
pub const MAX_TOLERANCE: f64 = 50.0;
/// The most points (pipes' ends and connectors) one query works on.
pub const MAX_POINTS: usize = 200_000;
/// The plan's squares, mm on a side, that the pipes are found by when a node may lie along one.
const SPAN: f64 = 1000.0;
/// The most squares one pipe goes in: a longer one across is not looked along.
const MAX_PIPE_CELLS: f64 = 1.0e6;
/// The most squares all the pipes of one query go in together: a few thousand pipes kilometres long on the diagonal would
/// otherwise fill billions.
pub const MAX_CELLS: usize = 4_000_000;

/// What meets at a node.
#[derive(Clone, Debug, PartialEq)]
pub enum Joint {
    /// A pipe's start (`end: false`) or end.
    RunEnd { run: Id, end: bool },
    /// A pipe passing through: the node lies on its length.
    RunSide { run: Id },
    /// A component's pipe connector, by its place in the family's list.
    Connector { element: Id, index: usize, domain: Option<Domain> },
}

impl Joint {
    pub fn element(&self) -> Id {
        match self {
            Joint::RunEnd { run, .. } | Joint::RunSide { run } => *run,
            Joint::Connector { element, .. } => *element,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub at: [f64; 3],
    pub joints: Vec<Joint>,
}

/// A piece of a pipe between two nodes, or the way through a fitting or a valve from one of its connectors to another.
#[derive(Clone, Debug)]
pub struct Edge {
    pub element: Id,
    pub from: usize,
    pub to: usize,
    /// mm; none through a fitting.
    pub length: Option<f64>,
}

/// Pipes and components joined to each other, and what they carry.
#[derive(Clone, Debug)]
pub struct Group {
    pub elements: Vec<Id>,
    /// The pipes' `system` names, those that have one.
    pub systems: Vec<String>,
    pub domains: Vec<Domain>,
}

#[derive(Clone, Debug)]
pub struct Warning {
    /// `systems_meet`, `domains_meet`, `joined_mid_run` or `smaller_downstream`.
    pub kind: &'static str,
    pub at: Option<[f64; 3]>,
    pub elements: Vec<Id>,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct Network {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub groups: Vec<Group>,
    /// The nodes where one end or one connector meets nothing.
    pub open: Vec<usize>,
    pub warnings: Vec<Warning>,
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Where along segment `a`–`b` the point nearest `p` lies (0 to 1) and how far `p` is from it.
fn along(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> (f64, f64) {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if len2 <= 0.0 {
        return (0.0, dist(p, a));
    }
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1] + (p[2] - a[2]) * d[2]) / len2).clamp(0.0, 1.0);
    (t, dist(p, [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t]))
}

struct Sets(Vec<usize>);

impl Sets {
    fn new(n: usize) -> Sets {
        Sets((0..n).collect())
    }
    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }
    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

/// The squares a pipe from `a` to `b` goes in, grown by `tol`: the first and last across x, then y. None when it goes in more
/// than `MAX_PIPE_CELLS`.
fn squares(a: [f64; 3], b: [f64; 3], tol: f64) -> Option<[i64; 4]> {
    let (x0, x1) = (a[0].min(b[0]) - tol, a[0].max(b[0]) + tol);
    let (y0, y1) = (a[1].min(b[1]) - tol, a[1].max(b[1]) + tol);
    let cells = ((x1 - x0) / SPAN + 1.0) * ((y1 - y0) / SPAN + 1.0);
    if cells > MAX_PIPE_CELLS {
        return None;
    }
    let k = |v: f64| (v / SPAN).floor() as i64;
    Some([k(x0), k(x1), k(y0), k(y1)])
}

/// How many squares those are.
fn square_count([x0, x1, y0, y1]: [i64; 4]) -> usize {
    let n = |lo: i64, hi: i64| usize::try_from(hi.saturating_sub(lo).saturating_add(1)).unwrap_or(0);
    n(x0, x1).saturating_mul(n(y0, y1))
}

/// The cell of a grid of `size` a point is in.
fn cell(p: [f64; 3], size: f64) -> (i64, i64, i64) {
    let k = |x: f64| (x / size).floor() as i64;
    (k(p[0]), k(p[1]), k(p[2]))
}

/// The network of the model's pipes and pipe connectors: of `system`'s pipes only (and what they join), of `level`'s
/// elements only, as asked; points within `tolerance` mm are one node.
pub fn network(m: &Model, system: Option<&str>, level: Option<Guid>, tolerance: f64) -> Result<Network, String> {
    let tol = tolerance.clamp(1e-6, MAX_TOLERANCE);
    // The pipes, their centre lines; the components' pipe connectors, placed.
    let mut runs: Vec<Pipe> = Vec::new();
    let mut points: Vec<([f64; 3], Joint)> = Vec::new();
    let mut through: Vec<Vec<usize>> = Vec::new();
    for (id, e) in m.elements.iter() {
        let id = *id;
        match e {
            Element::Pipe(r) => {
                if level.is_some_and(|l| l != r.level) {
                    continue;
                }
                let Some(g) = m.run_geom(e) else { continue };
                runs.push((id, g.a, g.b, g.width, r.system.clone(), points.len()));
                points.push((g.a, Joint::RunEnd { run: id, end: false }));
                points.push((g.b, Joint::RunEnd { run: id, end: true }));
            }
            Element::Component(c) => {
                if level.is_some_and(|l| l != c.level) {
                    continue;
                }
                let fitting =
                    m.families.get(&c.family).is_some_and(|f| matches!(f.category, Category::PipeFitting | Category::PipeAccessory));
                let mut mine = Vec::new();
                for (index, (k, at, _)) in m.component_connectors(c).into_iter().enumerate() {
                    if k.kind != ConnectorKind::Pipe {
                        continue;
                    }
                    let Some(at) = at else { continue };
                    mine.push(points.len());
                    points.push((at, Joint::Connector { element: id, index, domain: k.domain }));
                }
                if fitting && mine.len() > 1 {
                    through.push(mine);
                }
            }
            _ => {}
        }
        if points.len() > MAX_POINTS {
            return Err(format!("the model has more than {MAX_POINTS} pipe ends and connectors to join: ask for one `level` or `system`"));
        }
    }
    // Points within the tolerance are one node: each is compared with those in its cell and the cells round it.
    let mut sets = Sets::new(points.len());
    let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, (p, _)) in points.iter().enumerate() {
        let (x, y, z) = cell(*p, tol);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(others) = grid.get(&(x + dx, y + dy, z + dz)) {
                        for &j in others {
                            if dist(points[j].0, *p) <= tol {
                                sets.join(i, j);
                            }
                        }
                    }
                }
            }
        }
        grid.entry((x, y, z)).or_default().push(i);
    }
    let mut node_of: BTreeMap<usize, usize> = BTreeMap::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut at_point = vec![0; points.len()];
    for (i, (p, j)) in points.iter().enumerate() {
        let root = sets.find(i);
        let n = *node_of.entry(root).or_insert_with(|| {
            nodes.push(Node { at: *p, joints: Vec::new() });
            nodes.len() - 1
        });
        nodes[n].joints.push(j.clone());
        at_point[i] = n;
    }
    // A node on a pipe's length, away from its ends, splits it: found through a coarse grid of the pipes in plan. The squares
    // are counted before any is filled, so pipes that would fill too many are refused before they take the memory.
    let spans: Vec<Option<[i64; 4]>> = runs.iter().map(|(_, a, b, _, _, _)| squares(*a, *b, tol)).collect();
    let counts: Vec<usize> = spans.iter().map(|s| s.map_or(0, square_count)).collect();
    let total = counts.iter().fold(0usize, |t, n| t.saturating_add(*n));
    if total > MAX_CELLS {
        let (k, most) = counts.iter().enumerate().fold((0, 0), |best, (k, n)| if *n > best.1 { (k, *n) } else { best });
        let id = runs.get(k).map_or(0, |r| r.0);
        return Err(format!(
            "the pipes lie across {total} of the plan's {SPAN} mm squares, more than the {MAX_CELLS} one query looks through for \
             nodes along them (pipe {id} alone lies across {most}): shorten or delete pipes kilometres long, or ask for one `level`"
        ));
    }
    let mut by_plan: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (k, span) in spans.iter().enumerate() {
        let Some([x0, x1, y0, y1]) = *span else { continue };
        for cx in x0..=x1 {
            for cy in y0..=y1 {
                by_plan.entry((cx, cy)).or_default().push(k);
            }
        }
    }
    let mut sides: Vec<Vec<(f64, usize)>> = vec![Vec::new(); runs.len()];
    for (n, node) in nodes.iter().enumerate() {
        let p = node.at;
        let key = ((p[0] / SPAN).floor() as i64, (p[1] / SPAN).floor() as i64);
        let Some(near) = by_plan.get(&key) else { continue };
        for &k in near {
            let (id, a, b, _, _, _) = &runs[k];
            if node.joints.iter().any(|j| matches!(j, Joint::RunEnd { run, .. } if run == id)) {
                continue;
            }
            let (t, d) = along(p, *a, *b);
            let len = dist(*a, *b);
            if d <= tol && t * len > tol && (1.0 - t) * len > tol {
                sides[k].push((t, n));
            }
        }
    }
    for (k, list) in sides.iter().enumerate() {
        for &(_, n) in list {
            nodes[n].joints.push(Joint::RunSide { run: runs[k].0 });
        }
    }
    // Each pipe from its start through the nodes on it to its end; each fitting from its first connector to the others.
    let mut edges = Vec::new();
    for (k, (id, a, b, _, _, ends)) in runs.iter().enumerate() {
        let (first, last) = (at_point[*ends], at_point[*ends + 1]);
        let mut stops: Vec<(f64, usize)> = sides[k].clone();
        stops.sort_by(|x, y| x.0.total_cmp(&y.0));
        let len = dist(*a, *b);
        let mut prev = (0.0, first);
        for s in stops.into_iter().chain(std::iter::once((1.0, last))) {
            edges.push(Edge { element: *id, from: prev.1, to: s.1, length: Some((s.0 - prev.0) * len) });
            prev = s;
        }
    }
    for group in &through {
        for &i in &group[1..] {
            edges.push(Edge { element: points[i].1.element(), from: at_point[group[0]], to: at_point[i], length: None });
        }
    }
    // The groups: nodes joined by edges.
    let mut joined = Sets::new(nodes.len());
    for e in &edges {
        joined.join(e.from, e.to);
    }
    let mut group_of: BTreeMap<usize, usize> = BTreeMap::new();
    let mut members: Vec<Gathered> = Vec::new();
    let system_of: HashMap<Id, &str> = runs.iter().map(|(id, _, _, _, s, _)| (*id, s.as_str())).collect();
    for (n, node) in nodes.iter().enumerate() {
        let root = joined.find(n);
        let g = *group_of.entry(root).or_insert_with(|| {
            members.push(Default::default());
            members.len() - 1
        });
        let entry = &mut members[g];
        entry.3.push(n);
        for j in &node.joints {
            entry.0.insert(j.element());
            if let Some(s) = system_of.get(&j.element()).filter(|s| !s.is_empty()) {
                entry.1.insert(s.to_string());
            }
            if let Joint::Connector { domain: Some(d), .. } = j {
                entry.2.insert(*d);
            }
        }
    }
    // A system asked for keeps only the groups its pipes are in.
    let keep: Vec<bool> = members.iter().map(|(_, systems, _, _)| system.is_none_or(|s| systems.contains(s))).collect();
    let mut warnings = Vec::new();
    for (g, (elements, systems, domains, ns)) in members.iter().enumerate() {
        if !keep[g] {
            continue;
        }
        let at = ns.first().map(|&n| nodes[n].at);
        if systems.len() > 1 {
            let names: Vec<&str> = systems.iter().map(String::as_str).collect();
            warnings.push(Warning {
                kind: "systems_meet",
                at,
                elements: elements.iter().copied().collect(),
                message: format!("pipes of the systems {} are joined in one network", names.join(", ")),
            });
        }
        if domains.len() > 1 {
            let names: Vec<String> = domains
                .iter()
                .map(|d| serde_json::to_value(d).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default())
                .collect();
            warnings.push(Warning {
                kind: "domains_meet",
                at,
                elements: elements.iter().copied().collect(),
                message: format!("connectors of {} are joined in one network", names.join(", ")),
            });
        }
    }
    for (n, node) in nodes.iter().enumerate() {
        if !keep[group_of[&joined.find(n)]] {
            continue;
        }
        let on: Vec<Id> = node.joints.iter().filter_map(|j| if let Joint::RunSide { run } = j { Some(*run) } else { None }).collect();
        if !on.is_empty() {
            let mut elements: Vec<Id> = node.joints.iter().map(Joint::element).collect();
            elements.dedup();
            warnings.push(Warning {
                kind: "joined_mid_run",
                at: Some(node.at),
                elements,
                message: "a pipe or a connector meets another pipe along its length, with no fitting there".into(),
            });
        }
    }
    // A drain (a group with a waste connector) flows down: no pipe leaving a node downhill is narrower than one coming down to it.
    let width: HashMap<Id, f64> = runs.iter().map(|(id, _, _, w, _, _)| (*id, *w)).collect();
    // Each node's pipe pieces, in the edges' order, found once for all the nodes.
    let mut pieces: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, e) in edges.iter().enumerate() {
        if e.length.is_some() {
            pieces[e.from].push(i);
            if e.to != e.from {
                pieces[e.to].push(i);
            }
        }
    }
    let mut seen_drop = BTreeSet::new();
    for (n, node) in nodes.iter().enumerate() {
        let g = group_of[&joined.find(n)];
        if !keep[g] || !members[g].2.contains(&Domain::Waste) {
            continue;
        }
        let mut down: Vec<(Id, f64)> = Vec::new();
        let mut up: Vec<(Id, f64)> = Vec::new();
        for e in pieces[n].iter().map(|&i| &edges[i]) {
            let other = if e.from == n { e.to } else { e.from };
            let dz = nodes[other].at[2] - node.at[2];
            let w = width.get(&e.element).copied().unwrap_or(0.0);
            if dz < -tol {
                down.push((e.element, w));
            } else if dz > tol {
                up.push((e.element, w));
            }
        }
        let widest_in = up.iter().map(|x| x.1).fold(0.0, f64::max);
        for (id, w) in &down {
            if *w + 1e-9 < widest_in && seen_drop.insert((n, *id)) {
                let mut elements: Vec<Id> = up.iter().map(|x| x.0).collect();
                elements.push(*id);
                warnings.push(Warning {
                    kind: "smaller_downstream",
                    at: Some(node.at),
                    elements,
                    message: format!("a drain narrows downstream: {w} mm leaving where {widest_in} mm comes in"),
                });
            }
        }
    }
    let open = (0..nodes.len()).filter(|&n| keep[group_of[&joined.find(n)]] && nodes[n].joints.len() == 1).collect();
    let groups = members
        .into_iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|((elements, systems, domains, _), _)| Group {
            elements: elements.into_iter().collect(),
            systems: systems.into_iter().collect(),
            domains: domains.into_iter().collect(),
        })
        .collect();
    Ok(Network { nodes, edges, groups, open, warnings })
}
