//! Rule packs for topology graphs, and the checker that reads one graph with one pack.
//!
//! A pack is data (`data/graph-rules/<archetype>.json`): its rules each name an id, a classification and the parameters of
//! a predicate. The code owns each predicate, the pack owns its parameters. The house pack's rules are exploratory, authored
//! from one traced plan; the office, clinic and hospital packs hold the hard rules of the generator's first slice, which
//! the generator's report reads its parameters from.
//!
//! A graph is read kindly (`Graph::from_value`): the extractor's nodes have no kind, a generator's entry is a string and the
//! extractor's an object. Nothing here panics on a hostile graph; it is refused with a message that says what to fix.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::engine::Session;
use crate::generate::graph::{Edge as TopologyEdge, Node as TopologyNode, Topology};
use crate::generate::program::{Archetype, Role};
use crate::generate::report;
use crate::generate::swing;

mod medical;
#[cfg(test)]
mod medical_tests;
mod pattern;
#[cfg(test)]
mod pattern_tests;
mod studio;
#[cfg(test)]
mod studio_tests;
#[cfg(test)]
mod tests;

/// The biggest graph file read.
pub const MAX_FILE: u64 = 16 * 1024 * 1024;
/// The most nodes, and the most edges, a graph may hold.
pub const MAX_ITEMS: usize = 10_000;
const MAX_PATH: usize = 4096;
/// How many problems a rule's detail names before it says how many more there are.
const NAMED: usize = 5;

const SOURCES: [(&str, &str); 7] = [
    ("house", include_str!("../data/graph-rules/house.json")),
    ("office", include_str!("../data/graph-rules/office.json")),
    ("clinic", include_str!("../data/graph-rules/clinic.json")),
    ("hospital", include_str!("../data/graph-rules/hospital.json")),
    ("studio-office", include_str!("../data/graph-rules/studio-office.json")),
    ("office-pattern", include_str!("../data/graph-rules/office-pattern.json")),
    ("medical-day", include_str!("../data/graph-rules/medical-day.json")),
];

// ---- the pack

/// A kind of space a pack knows: its role, and the ranges of how many there are and how big each is (`None`: no limit).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kind {
    pub role: String,
    pub count: (f64, Option<f64>),
    pub area_m2: (f64, Option<f64>),
}

/// Two kinds that may be joined, and by which means: `swing`, `sliding`, `folding` or `open`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub pair: [String; 2],
    pub means: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub name: String,
    /// `hard` (a person approved it) or `exploratory`.
    pub classification: String,
    pub category: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub checkability: String,
    pub confidence: f64,
    pub rationale: String,
    pub exceptions: Vec<String>,
    pub provenance: String,
    #[serde(default)]
    pub approved_by: Option<String>,
    /// The parameters of the predicate the code owns.
    pub check: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub schema_version: String,
    pub pack_id: String,
    pub archetype: String,
    pub origin: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub reference_plans: Vec<String>,
    /// Every approved trace the pack draws on, which `reference_plans` (what its first rules were authored from) may not name.
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub reference_plan_count: usize,
    #[serde(default)]
    pub kinds: BTreeMap<String, Kind>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    /// Pairs that may be joined by a folding door, kept apart from `connections` (the first trace's `swing`, `sliding` and
    /// `open`): folding came with the Unit 1 trace.
    #[serde(default)]
    pub folding_connections: Vec<Connection>,
    pub rules: Vec<Rule>,
    /// Whether the pack's file has a `kinds` table. One it does not have is read as empty, and narrows nothing in a family
    /// that names the pack (`Pack::parse` sets it; it is no key of the file).
    #[serde(skip)]
    pub has_kinds: bool,
    /// Whether the pack's file has a `connections` or a `folding_connections` table.
    #[serde(skip)]
    pub has_connections: bool,
}

impl Pack {
    /// A pack file read, with which of its tables the file has: a key that is absent is a table the pack does not have.
    pub fn parse(text: &str) -> Result<Pack, String> {
        let mut pack: Pack = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let file: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        pack.has_kinds = file.get("kinds").is_some();
        pack.has_connections = file.get("connections").is_some() || file.get("folding_connections").is_some();
        Ok(pack)
    }

    /// The pack of `archetype` (house, office, clinic or hospital), parsed once.
    pub fn embedded(archetype: &str) -> Result<&'static Pack, String> {
        static PACKS: OnceLock<Vec<(&'static str, Result<Pack, String>)>> = OnceLock::new();
        let all = PACKS.get_or_init(|| {
            SOURCES
                .iter()
                .map(|(name, text)| (*name, Pack::parse(text).map_err(|e| format!("the embedded {name} rule pack: {e}"))))
                .collect()
        });
        match all.iter().find(|(name, _)| *name == archetype) {
            Some((_, Ok(pack))) => Ok(pack),
            Some((_, Err(e))) => Err(e.clone()),
            None => Err(format!("`archetype` is one of {}, not `{}`", names(), archetype.chars().take(40).collect::<String>())),
        }
    }
}

/// The archetypes that have a pack.
pub fn names() -> String {
    SOURCES.map(|(n, _)| n).join(", ")
}

// ---- the graph

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub kind: Option<String>,
    pub role: Option<String>,
    pub area_m2: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub operation: Option<String>,
    /// A swing door's side (a room id, or `exterior`) and jamb (`start` or `end`), when the graph says.
    pub opens_into: Option<String>,
    pub hinge: Option<String>,
    pub exterior: bool,
}

#[derive(Clone, Debug)]
pub struct Graph {
    pub entry: Option<String>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// How many entries a top-level `exterior_doors` list holds (doors with a room on one side only).
    pub exterior_doors: usize,
}

fn text(v: &Value, key: &str, what: &str) -> Result<Option<String>, String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.chars().take(200).collect())),
        Some(_) => Err(format!("{what}: `{key}` is text")),
    }
}

impl Graph {
    /// A topology graph as `topology --out` or `generate` writes it: `nodes`, `edges` and `entry` (a name, `{room}` or null).
    /// Unknown keys are left alone.
    pub fn from_value(v: &Value) -> Result<Graph, String> {
        let list = |key: &str| -> Result<&Vec<Value>, String> {
            let items = v.get(key).and_then(Value::as_array).ok_or_else(|| format!("the graph has no `{key}` list"))?;
            if items.len() > MAX_ITEMS {
                return Err(format!("the graph has {} `{key}`; at most {MAX_ITEMS} are read", items.len()));
            }
            Ok(items)
        };
        if !v.is_object() {
            return Err("the graph is a JSON object with `nodes`, `edges` and `entry`".into());
        }
        let mut nodes = Vec::new();
        for (i, n) in list("nodes")?.iter().enumerate() {
            let what = format!("node {i}");
            let id = text(n, "id", &what)?.filter(|s| !s.is_empty()).ok_or_else(|| format!("{what} has no `id`"))?;
            let area_m2 = match n.get("area_m2") {
                None | Some(Value::Null) => None,
                Some(a) => Some(a.as_f64().filter(|a| a.is_finite()).ok_or_else(|| format!("node `{id}`: `area_m2` is a finite number"))?),
            };
            nodes.push(Node { kind: text(n, "kind", &what)?, role: text(n, "role", &what)?, area_m2, id });
        }
        let mut edges = Vec::new();
        for (i, e) in list("edges")?.iter().enumerate() {
            let what = format!("edge {i}");
            let end = |key: &str| text(e, key, &what)?.ok_or_else(|| format!("{what} has no `{key}`"));
            edges.push(Edge {
                from: end("from")?,
                to: end("to")?,
                kind: text(e, "kind", &what)?.unwrap_or_default(),
                operation: text(e, "operation", &what)?,
                opens_into: text(e, "opens_into", &what)?,
                hinge: text(e, "hinge", &what)?,
                exterior: e.get("exterior") == Some(&Value::Bool(true)),
            });
        }
        let entry = match v.get("entry") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.chars().take(200).collect()),
            Some(o @ Value::Object(_)) => text(o, "room", "`entry`")?,
            Some(_) => return Err("`entry` is a space's name, an object with its `room`, or null".into()),
        };
        let exterior_doors = v.get("exterior_doors").and_then(Value::as_array).map_or(0, |a| a.len().min(MAX_ITEMS));
        Ok(Graph { entry, nodes, edges, exterior_doors })
    }
}

// ---- the checks

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pass,
    Fail,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Check {
    pub id: String,
    pub name: String,
    pub status: Status,
    pub detail: String,
}

impl Check {
    pub fn json(&self) -> Value {
        json!({"id": self.id, "name": self.name, "status": self.status.name(), "detail": self.detail})
    }
}

fn verdict(rule: &Rule, mut problems: Vec<String>, ok: String) -> Check {
    problems.sort();
    problems.dedup();
    let detail = match problems.len() {
        0 => ok,
        n if n <= NAMED => problems.join("; "),
        n => format!("{} (and {} more)", problems[..NAMED].join("; "), n - NAMED),
    };
    Check { id: rule.id.clone(), name: rule.name.clone(), status: if problems.is_empty() { Status::Pass } else { Status::Fail }, detail }
}

/// Every rule of `pack`, in the pack's order, judged on `graph`. An `Err` is a graph that cannot be judged by this pack
/// (an office graph whose spaces have no role, say), never a rule that failed.
pub fn check(pack: &Pack, graph: &Graph) -> Result<Vec<Check>, String> {
    if pack.archetype == "house" {
        let world = World::of(pack, graph);
        let mut checks = pack.rules.iter().map(|r| world.judge(r)).collect::<Result<Vec<_>, _>>()?;
        checks.extend(world.swing()?);
        return Ok(checks);
    }
    if pack.archetype == "studio-office" {
        return studio::check(pack, graph);
    }
    if pack.archetype == "medical-day" {
        return medical::check(pack, graph);
    }
    if pack.archetype == "office-pattern" {
        return pattern::check(pack, graph);
    }
    let archetype = Archetype::parse(&pack.archetype).ok_or_else(|| format!("no checker for the `{}` pack", pack.archetype))?;
    let done = report::graph_checks(&topology(graph)?, archetype);
    pack.rules
        .iter()
        .map(|r| {
            let c = done.iter().find(|c| c.id == r.id).ok_or_else(|| format!("the pack's rule {} has no check", r.id))?;
            Ok(Check {
                id: r.id.clone(),
                name: c.name.to_string(),
                status: if c.status == report::Status::Fail { Status::Fail } else { Status::Pass },
                detail: c.detail.clone(),
            })
        })
        .collect()
}

/// A graph as the generator's own type, for the rules that read it: every space needs a kind and a role.
fn topology(g: &Graph) -> Result<Topology, String> {
    let entry = g.entry.clone().ok_or("the graph has no `entry`, which these rules reach every space from")?;
    let mut nodes = Vec::new();
    for n in &g.nodes {
        let kind = n.kind.clone().ok_or_else(|| format!("node `{}` has no `kind`", n.id))?;
        let role = n.role.as_deref().ok_or_else(|| format!("node `{}` has no `role`", n.id))?;
        let role =
            Role::parse(role).ok_or_else(|| format!("node `{}` has the role `{role}`, which is not one of {}", n.id, Role::names()))?;
        nodes.push(TopologyNode { id: n.id.clone(), kind, role, area_m2: n.area_m2.unwrap_or(0.0) });
    }
    let edges = g
        .edges
        .iter()
        .filter(|e| !e.exterior)
        .map(|e| TopologyEdge {
            from: e.from.clone(),
            to: e.to.clone(),
            kind: match e.kind.as_str() {
                "door" => "door",
                "open" => "open",
                _ => "adjacent",
            },
        })
        .collect();
    Ok(Topology { entry, nodes, edges })
}

/// The house rules' view of a graph: what each space is, and who reaches whom over the doors and open edges inside it.
struct World<'a> {
    pack: &'a Pack,
    graph: &'a Graph,
    kind: BTreeMap<&'a str, Option<&'a str>>,
    /// The door and open edges that are inside the house, between two spaces.
    inside: Vec<&'a Edge>,
}

impl<'a> World<'a> {
    fn of(pack: &'a Pack, graph: &'a Graph) -> World<'a> {
        let kind: BTreeMap<&str, Option<&str>> = graph.nodes.iter().map(|n| (n.id.as_str(), n.kind.as_deref())).collect();
        let inside = graph
            .edges
            .iter()
            .filter(|e| {
                matches!(e.kind.as_str(), "door" | "open")
                    && !e.exterior
                    && kind.contains_key(e.from.as_str())
                    && kind.contains_key(e.to.as_str())
            })
            .collect();
        World { pack, graph, kind, inside }
    }

    fn kind_of(&self, id: &str) -> Option<&'a str> {
        self.kind.get(id).copied().flatten()
    }

    fn known(&self, id: &str) -> Option<&'a str> {
        self.kind_of(id).filter(|k| self.pack.kinds.contains_key(*k))
    }

    /// Who each space opens into by a door or an open edge.
    fn access(&self) -> BTreeMap<&'a str, BTreeSet<&'a str>> {
        let mut next: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for e in &self.inside {
            next.entry(e.from.as_str()).or_default().insert(e.to.as_str());
            next.entry(e.to.as_str()).or_default().insert(e.from.as_str());
        }
        next
    }

    fn judge(&self, rule: &Rule) -> Result<Check, String> {
        let (problems, ok) = match rule.id.as_str() {
            "HR-001" => self.kinds_known(),
            "HR-002" => self.edges_name_spaces(),
            "HR-003" => self.counts(),
            "HR-004" => self.areas(),
            "HR-005" => self.connections(),
            "HR-006" => self.one_door(rule)?,
            "HR-007" => self.joins_only(rule)?,
            "HR-008" => self.reach(rule)?,
            "HR-009" => self.entry_space(rule)?,
            "HR-010" | "HR-013" => self.reach(rule)?,
            "HR-011" => self.joins_only(rule)?,
            "HR-012" => self.folding(rule)?,
            other => return Err(format!("the pack's rule {other} has no evaluator")),
        };
        Ok(verdict(rule, problems, ok))
    }

    /// One check for each swing rule: the swing doors it covers must open into the kind and hang from the jamb it names. A
    /// door with neither field is not judged (graphs from before the fields, and hand-broken copies, stay as they were).
    fn swing(&self) -> Result<Vec<Check>, String> {
        let rules = swing::judged_rules()?;
        let exterior = swing::judged_exterior();
        let mut problems: Vec<Vec<String>> = vec![Vec::new(); rules.len()];
        let mut judged = vec![0_usize; rules.len()];
        let index = |id: &str| rules.iter().position(|r| r.id == id);
        for e in self.graph.edges.iter().filter(|e| e.kind == "door") {
            if !matches!(e.operation.as_deref(), Some("swing" | "double-swing")) {
                continue;
            }
            let (Some(oi), Some(hinge)) = (e.opens_into.as_deref(), e.hinge.as_deref()) else { continue };
            let (rule, why) = if e.exterior {
                let Some(rule) = exterior else { continue };
                let inner = if e.from == "exterior" { &e.to } else { &e.from };
                let bad = match rule.opens_into.as_str() {
                    "either" => false,
                    "exterior" => oi != "exterior",
                    _ => oi != inner.as_str(),
                };
                (rule, (bad, format!("exterior door of `{inner}` opens into `{oi}`, the rule says {}", rule.opens_into)))
            } else {
                let (Some(a), Some(b)) = (self.kind_of(&e.from), self.kind_of(&e.to)) else { continue };
                if !self.kind.contains_key(e.from.as_str()) || !self.kind.contains_key(e.to.as_str()) {
                    continue;
                }
                let Some(rule) = swing::judged_rule_for(a, b) else { continue };
                let bad = a != b
                    && rule.opens_into != "either"
                    && match self.kind_of(oi) {
                        Some(k) => k != rule.opens_into || (oi != e.from && oi != e.to),
                        None => true,
                    };
                (rule, (bad, format!("`{}` ({a}) to `{}` ({b}) opens into `{oi}`, the rule says {}", e.from, e.to, rule.opens_into)))
            };
            let Some(i) = index(&rule.id) else { continue };
            judged[i] += 1;
            if why.0 {
                problems[i].push(why.1);
            }
            if rule.hinge != "either" && hinge != rule.hinge {
                problems[i].push(format!("`{}` to `{}` hangs from the {hinge} jamb, the rule says {}", e.from, e.to, rule.hinge));
            }
        }
        Ok(rules
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let name = format!("door-swing {}-{}", r.pair[0], r.pair[1]);
                let ok = format!("{} swing doors judged ({}): opens into {}, hinge {}", judged[i], r.status, r.opens_into, r.hinge);
                let mut p = std::mem::take(&mut problems[i]);
                p.sort();
                p.dedup();
                let detail = match p.len() {
                    0 => ok,
                    n if n <= NAMED => p.join("; "),
                    n => format!("{} (and {} more)", p[..NAMED].join("; "), n - NAMED),
                };
                Check { id: r.id.clone(), name, status: if p.is_empty() { Status::Pass } else { Status::Fail }, detail }
            })
            .collect())
    }

    fn kinds_known(&self) -> (Vec<String>, String) {
        let problems = self
            .graph
            .nodes
            .iter()
            .filter(|n| self.known(&n.id).is_none())
            .map(|n| match &n.kind {
                None => format!("`{}` has no kind", n.id),
                Some(k) => format!("`{}` is a `{k}`, which the pack does not know", n.id),
            })
            .collect();
        (problems, format!("all {} spaces are of a kind the pack knows", self.graph.nodes.len()))
    }

    fn edges_name_spaces(&self) -> (Vec<String>, String) {
        let problems = self
            .graph
            .edges
            .iter()
            .filter(|e| !e.exterior)
            .flat_map(|e| [&e.from, &e.to])
            .filter(|id| !self.kind.contains_key(id.as_str()))
            .map(|id| format!("an edge names `{id}`, which is not a space"))
            .collect();
        (problems, format!("{} edges, all between spaces or to the exterior", self.graph.edges.len()))
    }

    fn counts(&self) -> (Vec<String>, String) {
        let mut problems = Vec::new();
        for (name, k) in &self.pack.kinds {
            let n = self.graph.nodes.iter().filter(|x| x.kind.as_deref() == Some(name)).count() as f64;
            if n < k.count.0 || k.count.1.is_some_and(|hi| n > hi) {
                let hi = k.count.1.map_or("no limit".to_string(), |h| h.to_string());
                problems.push(format!("{n} `{name}` spaces, the pack allows {} to {hi}", k.count.0));
            }
        }
        (problems, format!("every kind is within its count, over {} spaces", self.graph.nodes.len()))
    }

    fn areas(&self) -> (Vec<String>, String) {
        let mut problems = Vec::new();
        for n in &self.graph.nodes {
            let Some(kind) = self.known(&n.id) else { continue };
            let k = &self.pack.kinds[kind];
            match n.area_m2 {
                None => problems.push(format!("`{}` has no area_m2", n.id)),
                Some(a) if a < k.area_m2.0 || k.area_m2.1.is_some_and(|hi| a > hi) => {
                    let hi = k.area_m2.1.map_or("no limit".to_string(), |h| h.to_string());
                    problems.push(format!("`{}` is {a} m2, a `{kind}` is {} to {hi} m2", n.id, k.area_m2.0));
                }
                Some(_) => {}
            }
        }
        (problems, "every space is within its kind's area".into())
    }

    fn connections(&self) -> (Vec<String>, String) {
        let mut allowed: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
        for c in self.pack.connections.iter().chain(&self.pack.folding_connections) {
            let (a, b) = (c.pair[0].as_str(), c.pair[1].as_str());
            allowed.entry((a.min(b), a.max(b))).or_default().extend(c.means.iter().map(String::as_str));
        }
        let mut problems = Vec::new();
        for e in &self.inside {
            let (Some(a), Some(b)) = (self.known(&e.from), self.known(&e.to)) else { continue };
            let means = match (e.kind.as_str(), e.operation.as_deref()) {
                ("open", _) => "open",
                (_, Some("swing" | "double-swing")) => "swing",
                (_, Some("sliding" | "double-sliding")) => "sliding",
                (_, Some("folding")) => "folding",
                (_, Some(other)) => {
                    problems.push(format!("`{}` to `{}`: a `{other}` door is no means the pack allows", e.from, e.to));
                    continue;
                }
                (_, None) => {
                    problems.push(format!("`{}` to `{}`: the door has no `operation`", e.from, e.to));
                    continue;
                }
            };
            if !allowed.get(&(a.min(b), a.max(b))).is_some_and(|m| m.contains(means)) {
                problems.push(format!("`{}` ({a}) to `{}` ({b}) by {means} is not allowed", e.from, e.to));
            }
        }
        (problems, format!("all {} door and open edges join kinds the pack allows, by an allowed means", self.inside.len()))
    }

    fn param<'r>(rule: &'r Rule, key: &str) -> Result<&'r Value, String> {
        rule.check.get(key).ok_or_else(|| format!("the pack's rule {} has no `{key}`", rule.id))
    }

    fn word<'r>(rule: &'r Rule, key: &str) -> Result<&'r str, String> {
        Self::param(rule, key)?.as_str().ok_or_else(|| format!("the pack's rule {}: `{key}` is text", rule.id))
    }

    fn words<'r>(rule: &'r Rule, key: &str) -> Result<Vec<&'r str>, String> {
        let list = Self::param(rule, key)?.as_array().ok_or_else(|| format!("the pack's rule {}: `{key}` is a list", rule.id))?;
        list.iter().map(|v| v.as_str().ok_or_else(|| format!("the pack's rule {}: `{key}` is a list of text", rule.id))).collect()
    }

    fn of_kind(&self, kind: &str) -> impl Iterator<Item = &'a str> + '_ {
        let kind = kind.to_string();
        self.graph.nodes.iter().filter(move |n| n.kind.as_deref() == Some(kind.as_str())).map(|n| n.id.as_str())
    }

    fn one_door(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let kind = Self::word(rule, "kind")?;
        let n = Self::param(rule, "links")?.as_u64().ok_or_else(|| format!("the pack's rule {}: `links` is a number", rule.id))? as usize;
        let access = self.access();
        let mut links: BTreeMap<&str, usize> = BTreeMap::new();
        for e in &self.inside {
            *links.entry(e.from.as_str()).or_default() += 1;
            *links.entry(e.to.as_str()).or_default() += 1;
        }
        let problems = self
            .of_kind(kind)
            .filter_map(|id| {
                let (d, a) = (links.get(id).copied().unwrap_or(0), access.get(id).map_or(0, BTreeSet::len));
                (d != n || a != n).then(|| format!("`{id}` has {d} doors or open edges and {a} neighbours, not {n}"))
            })
            .collect();
        Ok((problems, format!("every `{kind}` has {n} way in and {n} neighbour")))
    }

    fn joins_only(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let kind = Self::word(rule, "kind")?;
        let only = Self::words(rule, "joins_only")?;
        let access = self.access();
        let mut problems: Vec<String> = self
            .of_kind(kind)
            .flat_map(|id| match access.get(id) {
                None => vec![format!("`{id}` opens into nothing")],
                Some(next) => next
                    .iter()
                    .filter(|x| !self.kind_of(x).is_some_and(|k| only.contains(&k)))
                    .map(|x| format!("`{id}` opens into `{x}`, a `{}`", self.kind_of(x).unwrap_or("space of no kind")))
                    .collect(),
            })
            .collect();
        let mut said = format!("every `{kind}` opens only into {}", only.join(", "));
        // Optionally, one of its ways in is a door of this operation.
        if let Some(op) = rule.check.get("at_least_one").and_then(Value::as_str) {
            for id in self.of_kind(kind) {
                let has = self.inside.iter().any(|e| {
                    (e.from == id || e.to == id)
                        && e.kind == "door"
                        && match op {
                            "sliding" => matches!(e.operation.as_deref(), Some("sliding" | "double-sliding")),
                            other => e.operation.as_deref() == Some(other),
                        }
                });
                if !has {
                    problems.push(format!("`{id}` has no {op} door"));
                }
            }
            said.push_str(&format!(", by at least one {op} door"));
        }
        Ok((problems, said))
    }

    fn reach(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let target = Self::word(rule, "target")?;
        let blocked = Self::words(rule, "through_blocked")?;
        let ok = format!("every `{target}` is reached from the entry without passing through {}", blocked.join(" or "));
        let Some(entry) = self.graph.entry.as_deref().filter(|e| self.kind.contains_key(e)) else {
            return Ok((vec!["there is no entry to reach anything from".into()], ok));
        };
        let access = self.access();
        let mut seen = BTreeSet::from([entry]);
        let mut queue = VecDeque::from([entry]);
        while let Some(here) = queue.pop_front() {
            for &there in access.get(here).into_iter().flatten() {
                if seen.insert(there) && !self.kind_of(there).is_some_and(|k| blocked.contains(&k)) {
                    queue.push_back(there);
                }
            }
        }
        let mut problems: Vec<String> =
            self.of_kind(target).filter(|id| !seen.contains(id)).map(|id| format!("`{id}` is not reached from `{entry}`")).collect();
        // Optionally, it is also next to a space of one of these kinds, or to the entry.
        if rule.check.get("next_to").is_some() {
            let next_to = Self::words(rule, "next_to")?;
            for id in self.of_kind(target) {
                let near =
                    access.get(id).into_iter().flatten().any(|x| *x == entry || self.kind_of(x).is_some_and(|k| next_to.contains(&k)));
                if !near {
                    problems.push(format!("`{id}` is not next to a {} or the entry", next_to.join(" or ")));
                }
            }
            return Ok((problems, format!("{ok}, and is next to a {} or the entry", next_to.join(" or "))));
        }
        Ok((problems, ok))
    }

    fn folding(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let (joins, to) = (Self::words(rule, "joins")?, Self::words(rule, "to")?);
        let in_set = |id: &str, set: &[&str]| self.kind_of(id).is_some_and(|k| set.contains(&k));
        let problems = self
            .inside
            .iter()
            .filter(|e| e.kind == "door" && e.operation.as_deref() == Some("folding"))
            .filter(|e| {
                let ok = |a: &str, b: &str| in_set(a, &joins) && in_set(b, &to);
                !(ok(&e.from, &e.to) || ok(&e.to, &e.from))
            })
            .map(|e| {
                format!(
                    "the folding door of `{}` ({}) to `{}` ({}) is not from a {} to a {}",
                    e.from,
                    self.kind_of(&e.from).unwrap_or("no kind"),
                    e.to,
                    self.kind_of(&e.to).unwrap_or("no kind"),
                    joins.join(" or "),
                    to.join(" or ")
                )
            })
            .collect();
        Ok((problems, format!("every folding door joins a {} to a {}", joins.join(" or "), to.join(" or "))))
    }

    fn entry_space(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let kinds = Self::words(rule, "kinds")?;
        let problem = match self.graph.entry.as_deref() {
            None => Some("the graph has no entry".to_string()),
            Some(e) if !self.kind.contains_key(e) => Some(format!("the entry `{e}` is not a space")),
            Some(e) if !self.kind_of(e).is_some_and(|k| kinds.contains(&k)) => {
                Some(format!("the entry `{e}` is a `{}`, not {}", self.kind_of(e).unwrap_or("space of no kind"), kinds.join(", ")))
            }
            Some(_) => None,
        };
        Ok((problem.into_iter().collect(), format!("the entry is a space of kind {}", kinds.join(", "))))
    }
}

// ---- the command

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    archetype: String,
    graph: Value,
}

fn read(graph: &Value) -> Result<Graph, String> {
    match graph {
        Value::Object(_) => Graph::from_value(graph),
        Value::String(path) if !path.trim().is_empty() && path.len() <= MAX_PATH && !path.contains('\0') => {
            let file = Path::new(path);
            let size = std::fs::metadata(file).map_err(|e| format!("`graph` {path}: {e}"))?.len();
            if size > MAX_FILE {
                return Err(format!("`graph` {path} is {size} bytes; at most {MAX_FILE} are read"));
            }
            let text = std::fs::read_to_string(file).map_err(|e| format!("`graph` {path}: {e}"))?;
            let value: Value = serde_json::from_str(&text).map_err(|e| format!("`graph` {path} is not JSON: {e}"))?;
            Graph::from_value(&value).map_err(|e| format!("`graph` {path}: {e}"))
        }
        _ => Err(format!("`graph` is the path of a topology graph (JSON), or the graph itself; at most {MAX_PATH} bytes of path")),
    }
}

/// The `rule_check` command: a topology graph judged by an archetype's rule pack.
pub fn rule_check(_: &mut Session, p: &Value) -> Result<Value, String> {
    let given: Params = serde_json::from_value(p.clone()).map_err(|e| {
        let why: String = e.to_string().chars().take(200).collect();
        format!("{why}; `rule_check` takes `archetype` ({}) and `graph`", names())
    })?;
    let pack = Pack::embedded(&given.archetype)?;
    let graph = read(&given.graph)?;
    let checks = check(pack, &graph)?;
    let failed = checks.iter().filter(|c| c.status == Status::Fail).count();
    Ok(json!({
        "archetype": pack.archetype,
        "pack_id": pack.pack_id,
        "pass": failed == 0,
        "summary": {"pass": checks.len() - failed, "fail": failed},
        "checks": checks.iter().map(Check::json).collect::<Vec<_>>(),
    }))
}
