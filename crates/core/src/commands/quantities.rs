//! `quantities`: what the project's beams, columns, footings, strip footings and floors come to in concrete, formwork and steel, a
//! level at a time. It reads the project and changes nothing. The rules are `crate::quantities`'s, the same ones a member's own
//! fields and the IFC use, so the totals here are the sum of the rows a schedule lists.

use std::collections::{BTreeMap, BTreeSet};

use super::bars::{column, write_table};
use super::*;
use crate::model::{Guid, Id};
use crate::quantities::{self, CATEGORIES, Take};
use crate::steel::Bars;

/// The most values of the parameter a call may split by.
pub const MAX_GROUPS: usize = 1_000;
/// The most categories a call may name (they are checked against the six that are counted).
const MAX_NAMES: usize = 64;
/// What a level's bars that name no host are called: they belong to no member, and are in the totals as a line of their own.
const UNHOSTED: &str = "unhosted bars";

fn round(x: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (x * k).round() / k
}

/// What a group of members come to.
#[derive(Clone, Copy, Default)]
struct Sum {
    count: usize,
    volume: f64,
    formwork: f64,
    rebar: f64,
}

impl Sum {
    fn add(&mut self, t: &Take) {
        self.count += 1;
        self.volume += t.volume_m3;
        self.formwork += t.formwork_m2;
        self.rebar += t.rebar_kg;
    }

    fn plus(&mut self, o: &Sum) {
        self.count += o.count;
        self.volume += o.volume;
        self.formwork += o.formwork;
        self.rebar += o.rebar;
    }

    /// The sums, and the steel and the forms per cubic metre of concrete (null where there is none).
    fn json(&self) -> Value {
        let per = |x: f64, places: i32| if self.volume > 0.0 { json!(round(x / self.volume, places)) } else { Value::Null };
        json!({
            "count": self.count, "volume_m3": round(self.volume, 3), "formwork_m2": round(self.formwork, 2),
            "rebar_kg": round(self.rebar, 2), "rebar_ratio": per(self.rebar, 1), "formwork_ratio": per(self.formwork, 2),
        })
    }
}

/// A parameter's value as the text a group is called, empty where there is none.
fn text_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(t)) => t.clone(),
        Some(v @ (Value::Number(_) | Value::Bool(_))) => v.to_string(),
        _ => String::new(),
    }
}

/// The categories a call names, among the ones that are counted; none given, all of them.
fn categories(c: &str, p: &Value) -> Result<Option<Vec<String>>, String> {
    let Some(list) = p.get("categories").filter(|v| !absent(Some(v))) else { return Ok(None) };
    let names = list
        .as_array()
        .filter(|a| a.len() <= MAX_NAMES)
        .ok_or_else(|| bad(c, format!("`categories` is a list of up to {MAX_NAMES} names")))?;
    let mut out = Vec::new();
    for v in names {
        let name = v.as_str().map(category).filter(|k| CATEGORIES.contains(&k.as_str()));
        out.push(name.ok_or_else(|| bad(c, format!("`categories` takes {}, got {v}", CATEGORIES.join(", "))))?);
    }
    Ok(Some(out))
}

pub(super) fn quantities(s: &mut Session, p: &Value) -> R {
    let c = "quantities";
    only(c, p, &["level", "ids", "id", "by", "categories", "path"])?;
    let m = &s.model;
    let level: Option<Guid> = if absent(p.get("level")) { None } else { Some(level_param(c, m, p, "level")?) };
    let chosen: Option<BTreeSet<Id>> =
        if absent(p.get("ids")) && absent(p.get("id")) { None } else { Some(ids_param(c, m, p)?.into_iter().collect()) };
    let wanted = categories(c, p)?;
    let by = string(c, p, "by")?;
    if let Some(name) = &by
        && !m.parameters.contains_key(name)
    {
        return Err(bad(c, format!("`{name}` isn't a project parameter; `parameters` lists them")));
    }
    let whole = level.is_none() && chosen.is_none() && wanted.is_none();
    let (ix, bars) = (quantities::Index::new(m), Bars::new(m));
    let order: Vec<(Guid, Id, String)> = m.levels().iter().map(|(id, l)| (m.guid(*id), *id, l.name.clone())).collect();
    // A level's place in elevation order; what has none (or no level) comes last.
    let place = |g: Option<Guid>| order.iter().position(|(guid, ..)| Some(*guid) == g).unwrap_or(order.len());
    let mut rows: BTreeMap<(usize, String, String), Sum> = BTreeMap::new();
    let (mut left_out, mut finishes, mut values) = (0usize, 0usize, BTreeSet::new());
    for (id, e) in m.elements.iter() {
        let category = e.category();
        let wanted_here = wanted.as_ref().is_none_or(|w| w.iter().any(|k| k == category));
        if !CATEGORIES.contains(&category) || !wanted_here || chosen.as_ref().is_some_and(|set| !set.contains(id)) {
            continue;
        }
        let at = m.level_of(e);
        if level.is_some() && at != level {
            continue;
        }
        let Some(t) = quantities::take(m, &ix, &bars, *id, e) else {
            left_out += 1;
            continue;
        };
        finishes += usize::from(matches!(e, Element::Floor(f) if !f.structural));
        let value = by.as_ref().map(|name| text_of(e.params().and_then(|q| q.get(name)))).unwrap_or_default();
        values.insert(value.clone());
        if values.len() > MAX_GROUPS {
            return Err(bad(c, format!("`by` has more than {MAX_GROUPS} values, the most a call splits by; name a parameter with fewer")));
        }
        rows.entry((place(at), category.to_string(), value)).or_default().add(&t);
    }
    // Bars that name no host are no member's: a line of their own on their level, so the project's steel is the steel list's.
    let mut counted: f64 = rows.values().map(|r| r.rebar).sum();
    if chosen.is_none() && wanted.is_none() {
        for (at, (guid, ..)) in order.iter().enumerate().filter(|(_, (guid, ..))| level.is_none_or(|l| l == *guid)) {
            let kg = bars.unhosted(m, *guid);
            if kg > 0.0 {
                rows.entry((at, UNHOSTED.to_string(), String::new())).or_default().rebar += kg;
                counted += kg;
            }
        }
    }
    let mut notes = vec![
        "BIMCraft's own rules, not any program's: a beam is counted between the faces of its concrete columns and a structural floor \
         over a side of it takes its thickness of forms and half the beam's width of concrete; a column stops at the floor over it; a \
         floor's forms are its soffit, net of what carries it, and its free edges (docs/areas/quantities.md)"
            .to_string(),
    ];
    if left_out > 0 {
        notes.push(format!(
            "{left_out} member(s) have no concrete here: a steel section is weighed under Weight, and a member needs its type"
        ));
    }
    if finishes > 0 {
        notes.push(format!("{finishes} floor(s) are not structural: their concrete is counted and they have no forms"));
    }
    if whole && bars.total(m) - counted > 0.005 {
        notes.push(format!(
            "{} kg of bars are hosted by an element this does not count (a steel section)",
            round(bars.total(m) - counted, 2)
        ));
    }
    let (mut by_level, mut total): (BTreeMap<usize, Sum>, Sum) = (BTreeMap::new(), Sum::default());
    for ((at, ..), sum) in &rows {
        by_level.entry(*at).or_default().plus(sum);
        total.plus(sum);
    }
    let name = |at: usize| order.get(at).map_or("", |(_, _, n)| n.as_str()).to_string();
    let row_json = |at: &usize, category: &str, value: &str, sum: &Sum| {
        let mut row = sum.json();
        row["level"] = json!(name(*at));
        row["category"] = json!(category);
        if by.is_some() {
            row["value"] = json!(value);
        }
        row
    };
    let list: Vec<Value> = rows.iter().map(|((at, category, value), sum)| row_json(at, category, value, sum)).collect();
    let levels: Vec<Value> = by_level
        .iter()
        .map(|(at, sum)| {
            let mut level = sum.json();
            level["level"] = json!(name(*at));
            level["id"] = json!(order.get(*at).map(|(_, id, _)| *id));
            level
        })
        .collect();
    let mut reply = json!({"rows": list, "levels": levels, "total": total.json(), "notes": notes});
    if let Some(path) = string(c, p, "path")? {
        use schedule::Kind as K;
        let mut columns = vec![("Level", K::Text), ("Category", K::Text)];
        if by.is_some() {
            columns.push(("Value", K::Text));
        }
        columns.extend([
            ("Count", K::Number),
            ("Volume", K::Number),
            ("Formwork Area", K::Area),
            ("Rebar Weight", K::Number),
            ("Rebar Ratio", K::Number),
        ]);
        let table_rows: Vec<Vec<Value>> = rows
            .iter()
            .map(|((at, category, value), sum)| {
                let r = sum.json();
                let mut cells = vec![json!(name(*at)), json!(category)];
                if by.is_some() {
                    cells.push(json!(value));
                }
                cells.extend([
                    r["count"].clone(),
                    r["volume_m3"].clone(),
                    r["formwork_m2"].clone(),
                    r["rebar_kg"].clone(),
                    r["rebar_ratio"].clone(),
                ]);
                cells
            })
            .collect();
        let table = schedule::Table {
            columns: columns.iter().map(|(h, k)| column(h, *k)).collect(),
            ids: vec![Vec::new(); table_rows.len()],
            rows: table_rows,
            totals: None,
        };
        write_table(c, &path, &table, "Quantities")?;
        if let Some(reply) = reply.as_object_mut() {
            reply.insert("path".into(), json!(path));
        }
    }
    Ok(reply)
}

/// `schedule` with `template` \"materials_summary\": the same call, a schedule of the six categories, with its fields, filters and sort
/// filled in where the call gives none. `level` is a level's name or id. It cannot split by a project parameter: a schedule of several
/// categories has none among its fields, so `quantities` does that.
pub(super) fn materials_summary(s: &mut Session, p: &Value) -> R {
    let c = "schedule";
    let m = &s.model;
    if string(c, p, "category")?.is_some_and(|k| category(&k) != schedule::MULTI) {
        return Err(bad(c, "`materials_summary` is a schedule of several categories"));
    }
    if !absent(p.get("by")) {
        return Err(bad(
            c,
            "`materials_summary` cannot split by a project parameter, which a schedule of several categories has no field for; `quantities` with `by` does",
        ));
    }
    if !absent(p.get("host")) || !absent(p.get("show")) {
        return Err(bad(c, "`materials_summary` takes `level`, not `host` or `show`"));
    }
    let level = if absent(p.get("level")) { None } else { Some(level_param(c, m, p, "level")?) };
    let level = level.and_then(|l| match m.elements.get(l) {
        Some(Element::Level(l)) => Some(l.name.clone()),
        _ => None,
    });
    let t = quantities::template(m, level.as_deref());
    let mut q = p.as_object().cloned().unwrap_or_default();
    for key in ["template", "level"] {
        q.remove(key);
    }
    q.entry("name").or_insert_with(|| json!(t.name));
    q.insert("category".into(), json!(schedule::MULTI));
    q.entry("categories").or_insert_with(|| json!(CATEGORIES));
    q.entry("fields").or_insert_with(|| serde_json::to_value(&t.fields).unwrap_or_default());
    q.entry("filters").or_insert_with(|| serde_json::to_value(&t.filters).unwrap_or_default());
    q.entry("sort").or_insert_with(|| serde_json::to_value(&t.sort).unwrap_or_default());
    q.entry("grouped").or_insert_with(|| json!(true));
    super::schedule(s, &Value::Object(q))
}
