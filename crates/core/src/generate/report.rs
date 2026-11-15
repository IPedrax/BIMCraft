//! What was checked and how it went. Each rule has a stable id and says what it found. The rules on the access graph read
//! their roles and groups from the archetype's rule pack; the code owns each predicate.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::graph_rules::Pack;

use super::graph::Topology;
use super::program::{Archetype, Role};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

impl Status {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Check {
    pub id: &'static str,
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
}

pub(crate) fn check(id: &'static str, name: &'static str, problems: Vec<String>, ok: String) -> Check {
    match problems.first() {
        None => Check { id, name, status: Status::Pass, detail: ok },
        Some(first) => Check {
            id,
            name,
            status: Status::Fail,
            detail: format!("{first}{}", if problems.len() > 1 { format!(" (and {} more)", problems.len() - 1) } else { String::new() }),
        },
    }
}

/// A group of kinds a program must hold at least `at_least` of, named as the report says it.
pub(crate) struct Group {
    what: String,
    kinds: Vec<String>,
    at_least: usize,
}

/// What the rules on the access graph read from the archetype's pack: the roles GR-002 reaches from and to, the pairs of
/// roles GR-003 to GR-005 keep from opening into each other, and PR-001's groups. The code owns each predicate.
pub(crate) struct Rules {
    private: Vec<Role>,
    open: Vec<Role>,
    pairs: [(Role, Role); 3],
    pub(crate) groups: Vec<Group>,
}

fn roles(v: &Value, id: &str, key: &str) -> Result<Vec<Role>, String> {
    let list = v.get(key).and_then(Value::as_array).ok_or_else(|| format!("rule {id} has no `{key}` list"))?;
    list.iter()
        .map(|r| {
            r.as_str().and_then(Role::parse).ok_or_else(|| format!("rule {id}: `{key}` holds a role that is not one of {}", Role::names()))
        })
        .collect()
}

pub(crate) fn rules_of(archetype: Archetype) -> Result<Rules, String> {
    let pack = Pack::embedded(archetype.name())?;
    let params = |id: &str| -> Result<&Value, String> {
        pack.rules
            .iter()
            .find(|r| r.id == id)
            .map(|r| &r.check)
            .ok_or_else(|| format!("the {} rule pack has no rule {id}", archetype.name()))
    };
    let pair = |id: &str| -> Result<(Role, Role), String> {
        match roles(params(id)?, id, "between")?[..] {
            [a, b] => Ok((a, b)),
            _ => Err(format!("rule {id}: `between` names two roles")),
        }
    };
    let groups = params("PR-001")?
        .get("groups")
        .and_then(Value::as_array)
        .ok_or("rule PR-001 has no `groups` list")?
        .iter()
        .map(|g| {
            let what = g.get("what").and_then(Value::as_str).ok_or("a group of PR-001 has no `what`")?;
            let kinds = g.get("kinds").and_then(Value::as_array).ok_or("a group of PR-001 has no `kinds`")?;
            let at_least = g.get("at_least").and_then(Value::as_u64).ok_or("a group of PR-001 has no `at_least`")?;
            Ok(Group {
                what: what.to_string(),
                kinds: kinds.iter().filter_map(|k| k.as_str().map(str::to_string)).collect(),
                at_least: at_least as usize,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Rules {
        private: roles(params("GR-002")?, "GR-002", "private_roles")?,
        open: roles(params("GR-002")?, "GR-002", "open_roles")?,
        pairs: [pair("GR-003")?, pair("GR-004")?, pair("GR-005")?],
        groups,
    })
}

/// The spaces reached from `from` over door and open edges only.
fn reached(t: &Topology, from: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    let mut next: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in t.access() {
        next.entry(e.from.as_str()).or_default().push(e.to.as_str());
        next.entry(e.to.as_str()).or_default().push(e.from.as_str());
    }
    let mut seen: BTreeSet<String> = from.into_iter().collect();
    let mut frontier: Vec<String> = seen.iter().cloned().collect();
    while let Some(here) = frontier.pop() {
        for &there in next.get(here.as_str()).map(Vec::as_slice).unwrap_or_default() {
            if seen.insert(there.to_string()) {
                frontier.push(there.to_string());
            }
        }
    }
    seen
}

/// The rules on the access graph, which a plan must meet however it is laid out.
pub fn graph_checks(t: &Topology, archetype: Archetype) -> Vec<Check> {
    match rules_of(archetype) {
        Ok(rules) => checks_by(t, archetype, &rules),
        Err(e) => vec![Check {
            id: "PACK-001",
            name: "rule-pack-loads",
            status: Status::Fail,
            detail: format!("the {} rule pack can't be read: {e}", archetype.name()),
        }],
    }
}

fn checks_by(t: &Topology, archetype: Archetype, rules: &Rules) -> Vec<Check> {
    let role = |id: &str| t.node(id).map(|n| n.role);
    let missing: Vec<String> = t
        .edges
        .iter()
        .flat_map(|e| [&e.from, &e.to])
        .filter(|id| t.node(id).is_none())
        .map(|id| format!("an edge names `{id}`, which is not a space"))
        .collect();
    let open: Vec<String> = t.nodes.iter().filter(|n| rules.open.contains(&n.role)).map(|n| n.id.clone()).collect();
    let from_open = reached(t, open);
    let lost: Vec<String> = t
        .nodes
        .iter()
        .filter(|n| rules.private.contains(&n.role) && !from_open.contains(&n.id))
        .map(|n| format!("`{}` cannot be reached from a circulation, shared or public space", n.id))
        .collect();
    let joined = |a: Role, b: Role| -> Vec<String> {
        t.access()
            .filter(|e| role(&e.from) == Some(a) && role(&e.to) == Some(b))
            .map(|e| format!("`{}` opens straight into `{}`", e.from, e.to))
            .collect()
    };
    let [(a3, b3), (a4, b4), (a5, b5)] = rules.pairs;
    let private_private = joined(a3, b3);
    let clinical_clinical = joined(a4, b4);
    let mixed: Vec<String> = joined(a5, b5).into_iter().chain(joined(b5, a5)).collect();
    let from_entry = reached(t, [t.entry.clone()]);
    let cut_off: Vec<String> = t
        .nodes
        .iter()
        .filter(|n| !from_entry.contains(&n.id))
        .map(|n| format!("`{}` cannot be reached from the entry `{}`", n.id, t.entry))
        .collect();
    let access = t.access().count();
    vec![
        check("GR-001", "edges-name-spaces", missing, format!("{} edges, all between spaces", t.edges.len())),
        check(
            "GR-002",
            "private-reachable",
            lost,
            "every private and clinical space is reached from a circulation, shared or public one".into(),
        ),
        check("GR-003", "private-not-to-private", private_private, "no private space opens into another".into()),
        check("GR-004", "clinical-not-to-clinical", clinical_clinical, "no clinical space opens into another".into()),
        check("GR-005", "private-not-to-clinical", mixed, "no private space opens into a clinical one".into()),
        check(
            "GR-006",
            "entry-connected",
            cut_off,
            format!("all {} spaces are reached from `{}` over {access} door and open edges", t.nodes.len(), t.entry),
        ),
        program_check(t, archetype, &rules.groups),
    ]
}

fn program_check(t: &Topology, archetype: Archetype, groups: &[Group]) -> Check {
    let problems: Vec<String> = groups
        .iter()
        .filter(|g| t.nodes.iter().filter(|node| g.kinds.contains(&node.kind)).count() < g.at_least)
        .map(|g| format!("the {} program has no {}", archetype.name(), g.what))
        .collect();
    check("PR-001", "minimum-program", problems, format!("the {} minimum program is present", archetype.name()))
}
