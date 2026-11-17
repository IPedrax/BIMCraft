//! `ifc_survey`: what an IFC file holds, as counts and the names a format writes, so its owner can run it on a file that must not
//! leave the machine and paste the reply back. The reply has the schema, the units, whether the text is UTF-8, each storey's
//! elevation beside its placement and the height of its lowest product, and for each class how many there are, their predefined
//! types, what their bodies are made of, and how many are contained, aggregated, nested and typed; then the property sets and
//! quantity sets with the properties each carries, and the relations by class. It has no product's name, no id, no value and no
//! coordinate but a storey's elevation and heights (in the file's own length unit, which the reply names).
//!
//! The reply is capped at [`MAX_BYTES`]: a list keeps the rows that fit, and `left_out` says how many it held back.
//! `ponytail:` a placement's height is the sum of the heights up its chain of placements and takes no rotation into account;
//! upgrade if a survey shows a file that turns a storey on its side.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Map, Value, json};

use crate::step::{File, V};

/// The reply stays under this many bytes.
pub const MAX_BYTES: usize = 256 << 10;
/// Most rows of any list.
const MAX_ROWS: usize = 500;
/// Most properties a property set lists.
const MAX_PROPS: usize = 200;
/// Placements nest no deeper than this.
const DEPTH: usize = 64;
/// The bytes each list may take: together under [`MAX_BYTES`] with room for the rest.
const STOREYS: usize = 20 << 10;
const CLASSES: usize = 90 << 10;
const PROPERTY_SETS: usize = 70 << 10;
const QUANTITY_SETS: usize = 30 << 10;
const RELATIONS: usize = 20 << 10;

/// What is counted of one class.
#[derive(Default)]
struct Class {
    count: usize,
    predefined: BTreeMap<String, usize>,
    bodies: BTreeMap<String, usize>,
    /// Products whose body has one item, two, three or more.
    items: [usize; 3],
    contained: usize,
    aggregated: usize,
    nested: usize,
    typed: usize,
}

/// A property set or a quantity set, by name.
#[derive(Default)]
struct Set {
    count: usize,
    /// The classes of what it is on.
    on: BTreeMap<String, usize>,
    /// Each property or quantity by name, with the types it has and how many.
    members: BTreeMap<String, BTreeMap<String, usize>>,
    /// Members left out for the cap.
    more: usize,
}

/// The ids a parameter names: one reference, or a list of them.
fn refs(v: Option<&V>) -> Vec<u64> {
    match v {
        Some(V::Ref(r)) => vec![*r],
        Some(V::List(l)) => l.iter().filter_map(|x| if let V::Ref(r) = x { Some(*r) } else { None }).collect(),
        _ => Vec::new(),
    }
}

fn real(v: Option<&V>) -> Option<f64> {
    match v? {
        V::Real(x) => Some(*x),
        V::Int(i) => Some(*i as f64),
        V::Typed(_, inner) => real(Some(inner.as_ref())),
        _ => None,
    }
}

fn text(v: Option<&V>) -> Option<&str> {
    match v {
        Some(V::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// An enumeration's or a string's word, `""` for anything else.
fn word(v: Option<&V>) -> String {
    match v {
        Some(V::Enum(n) | V::Str(n)) => n.clone(),
        _ => String::new(),
    }
}

/// The kind of value a property holds: the type it is written with (`IFCLABEL`), else what it is.
fn kind(v: Option<&V>) -> String {
    match v {
        Some(V::Typed(t, _)) => t.clone(),
        Some(V::Str(_)) => "STRING".into(),
        Some(V::Int(_)) => "INTEGER".into(),
        Some(V::Real(_)) => "REAL".into(),
        Some(V::Enum(_)) => "ENUM".into(),
        Some(V::Ref(_)) => "REF".into(),
        Some(V::List(_)) => "LIST".into(),
        _ => "NULL".into(),
    }
}

/// The height of a placement above the origin: the heights of its chain of placements added up.
fn placement_z(f: &File, id: u64) -> Option<f64> {
    let (mut z, mut at) = (0.0, Some(id));
    for _ in 0..DEPTH {
        let Some(p) = at else { return Some(z) };
        let e = f.entities.get(&p)?;
        if e.name != "IFCLOCALPLACEMENT" {
            return None;
        }
        let axis = refs(e.args.get(1)).first().and_then(|a| f.entities.get(a))?;
        let point = refs(axis.args.first()).first().and_then(|l| f.entities.get(l))?;
        if let Some(V::List(c)) = point.args.first() {
            z += real(c.get(2)).unwrap_or(0.0);
        }
        at = refs(e.args.first()).first().copied();
    }
    None
}

/// The rows that fit in `bytes` and in [`MAX_ROWS`], and how many were left out.
fn fit(rows: Vec<Value>, bytes: usize) -> (Vec<Value>, usize) {
    let total = rows.len();
    let (mut used, mut kept) = (0, Vec::new());
    for r in rows {
        used += r.to_string().len();
        if kept.len() >= MAX_ROWS || used > bytes {
            break;
        }
        kept.push(r);
    }
    let left = total - kept.len();
    (kept, left)
}

fn set_rows(sets: BTreeMap<String, Set>, key: &str, bytes: usize) -> (Vec<Value>, usize) {
    let rows = sets
        .into_iter()
        .map(|(name, s)| {
            let mut row = json!({"name": name, "count": s.count, "on": s.on, "more": s.more});
            row[key] = json!(s.members);
            row
        })
        .collect();
    fit(rows, bytes)
}

/// The survey of `file`, read from `bytes`.
pub fn of(file: &File, bytes: &[u8]) -> Value {
    let mut ids: Vec<u64> = file.entities.keys().copied().collect();
    ids.sort_unstable();
    let get = |id: u64| file.entities.get(&id);
    let mut classes: BTreeMap<&str, Class> = BTreeMap::new();
    let mut relations: BTreeMap<&str, usize> = BTreeMap::new();
    let mut contained: HashSet<u64> = HashSet::new();
    let mut aggregated: HashSet<u64> = HashSet::new();
    let mut nested: HashSet<u64> = HashSet::new();
    let mut typed: HashSet<u64> = HashSet::new();
    let mut defined: Vec<(Vec<u64>, Vec<u64>)> = Vec::new();
    let mut by_storey: HashMap<u64, Vec<u64>> = HashMap::new();
    for id in &ids {
        let Some(e) = get(*id) else { continue };
        let class = classes.entry(e.name.as_str()).or_default();
        class.count += 1;
        for a in &e.args {
            if let V::Enum(n) = a
                && !matches!(n.as_str(), "T" | "F" | "U")
            {
                *class.predefined.entry(n.clone()).or_default() += 1;
            }
        }
        if e.name.starts_with("IFCREL") {
            *relations.entry(e.name.as_str()).or_default() += 1;
        }
        match e.name.as_str() {
            "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                let related = refs(e.args.get(4));
                if let Some(storey) = refs(e.args.get(5)).first() {
                    by_storey.entry(*storey).or_default().extend(related.iter().copied());
                }
                contained.extend(related);
            }
            "IFCRELAGGREGATES" => aggregated.extend(refs(e.args.get(5))),
            "IFCRELNESTS" => nested.extend(refs(e.args.get(5))),
            "IFCRELDEFINESBYTYPE" => typed.extend(refs(e.args.get(4))),
            "IFCRELDEFINESBYPROPERTIES" => defined.push((refs(e.args.get(4)), refs(e.args.get(5)))),
            _ => {}
        }
    }
    // What the relations say of each class, and what its products' bodies are made of.
    for id in &ids {
        let Some(e) = get(*id) else { continue };
        let class = classes.entry(e.name.as_str()).or_default();
        class.contained += usize::from(contained.contains(id));
        class.aggregated += usize::from(aggregated.contains(id));
        class.nested += usize::from(nested.contains(id));
        class.typed += usize::from(typed.contains(id));
        let shape = refs(e.args.get(6)).first().and_then(|s| get(*s)).filter(|s| s.name == "IFCPRODUCTDEFINITIONSHAPE");
        for rep in refs(shape.and_then(|s| s.args.get(2))).into_iter().filter_map(|r| get(r)) {
            if text(rep.args.get(1)) != Some("Body") {
                continue;
            }
            let items = refs(rep.args.get(3));
            if !items.is_empty() {
                class.items[items.len().min(3) - 1] += 1;
            }
            for item in items.into_iter().filter_map(|i| get(i)) {
                *class.bodies.entry(item.name.clone()).or_default() += 1;
            }
        }
    }
    let mut units = Map::new();
    for assignment in ids.iter().filter_map(|i| get(*i)).filter(|e| e.name == "IFCUNITASSIGNMENT") {
        for unit in refs(assignment.args.first()).into_iter().filter_map(|r| get(r)) {
            let (unit_type, name) = match unit.name.as_str() {
                "IFCSIUNIT" => (word(unit.args.get(1)), format!("{}{}", word(unit.args.get(2)), word(unit.args.get(3)))),
                "IFCCONVERSIONBASEDUNIT" => (word(unit.args.get(1)), word(unit.args.get(2))),
                _ => continue,
            };
            if !unit_type.is_empty() {
                units.insert(unit_type, json!(name));
            }
        }
    }
    let mut storeys: Vec<Value> = Vec::new();
    for id in &ids {
        let Some(e) = get(*id).filter(|e| e.name == "IFCBUILDINGSTOREY") else { continue };
        let placement = refs(e.args.get(5)).first().and_then(|p| placement_z(file, *p));
        let products = by_storey.get(id).map_or(&[][..], Vec::as_slice);
        let lowest = products
            .iter()
            .filter_map(|p| get(*p))
            .filter_map(|p| refs(p.args.get(5)).first().and_then(|q| placement_z(file, *q)))
            .fold(None, |m: Option<f64>, z| Some(m.map_or(z, |m| m.min(z))));
        storeys.push(
            json!({"elevation": real(e.args.get(9)), "placement_z": placement, "lowest_product_z": lowest, "products": products.len()}),
        );
    }
    // Property sets and quantity sets: their members, then the classes they are on.
    let mut psets: BTreeMap<String, Set> = BTreeMap::new();
    let mut qsets: BTreeMap<String, Set> = BTreeMap::new();
    for id in &ids {
        let Some(e) = get(*id) else { continue };
        let (target, list) = match e.name.as_str() {
            "IFCPROPERTYSET" => (&mut psets, 4),
            "IFCELEMENTQUANTITY" => (&mut qsets, 5),
            _ => continue,
        };
        let Some(name) = text(e.args.get(2)) else { continue };
        let set = target.entry(name.to_string()).or_default();
        set.count += 1;
        for member in refs(e.args.get(list)).into_iter().filter_map(|r| get(r)) {
            let Some(member_name) = text(member.args.first()) else { continue };
            let type_name = if member.name == "IFCPROPERTYSINGLEVALUE" { kind(member.args.get(2)) } else { member.name.clone() };
            if set.members.len() >= MAX_PROPS && !set.members.contains_key(member_name) {
                set.more += 1;
                continue;
            }
            *set.members.entry(member_name.to_string()).or_default().entry(type_name).or_default() += 1;
        }
    }
    for (objects, definitions) in &defined {
        for definition in definitions.iter().filter_map(|d| get(*d)) {
            let Some(name) = text(definition.args.get(2)) else { continue };
            let target = match definition.name.as_str() {
                "IFCPROPERTYSET" => &mut psets,
                "IFCELEMENTQUANTITY" => &mut qsets,
                _ => continue,
            };
            let Some(set) = target.get_mut(name) else { continue };
            for object in objects.iter().filter_map(|o| get(*o)) {
                *set.on.entry(object.name.clone()).or_default() += 1;
            }
        }
    }
    let mut list: Vec<(&str, Class)> = classes.into_iter().collect();
    list.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
    let class_rows = list
        .into_iter()
        .map(|(name, c)| {
            json!({"class": name, "count": c.count, "predefined": c.predefined, "bodies": c.bodies,
                "items": {"1": c.items[0], "2": c.items[1], "3+": c.items[2]},
                "contained": c.contained, "aggregated": c.aggregated, "nested": c.nested, "typed": c.typed})
        })
        .collect();
    let relation_rows = relations.into_iter().map(|(name, n)| json!({"class": name, "count": n})).collect();
    let (storeys, storeys_left) = fit(storeys, STOREYS);
    let (classes, classes_left) = fit(class_rows, CLASSES);
    let (property_sets, property_sets_left) = set_rows(psets, "properties", PROPERTY_SETS);
    let (quantity_sets, quantity_sets_left) = set_rows(qsets, "quantities", QUANTITY_SETS);
    let (relations, relations_left) = fit(relation_rows, RELATIONS);
    json!({
        "schema": file.schema,
        "encoding": {"valid_utf8": std::str::from_utf8(bytes).is_ok(), "non_ascii_bytes": bytes.iter().filter(|b| **b >= 0x80).count()},
        "units": units,
        "storeys": storeys,
        "classes": classes,
        "property_sets": property_sets,
        "quantity_sets": quantity_sets,
        "relations": relations,
        "left_out": {"storeys": storeys_left, "classes": classes_left, "property_sets": property_sets_left,
            "quantity_sets": quantity_sets_left, "relations": relations_left}
    })
}
