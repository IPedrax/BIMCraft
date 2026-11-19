//! The Asset Manager: what a project draws with, gathered in one place. Its materials, fill types (filled region types),
//! fill patterns, line patterns, line styles and pictures, each with its id, its GUID and where it is used: what names it,
//! and where that is.
//!
//! What uses an asset is found as Purge Unused counts it (`Model::lib_refs_mut`), each place by who holds it: an element
//! (a detail line its style, a face painted its material), a type's layer, a family, another asset (a material its picture
//! and fill patterns, a fill type its patterns, a line style its pattern), a view template's overrides or a group's member.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::ids::Id;
use crate::library::Kind;
use crate::model::Model;

/// The kinds the Asset Manager lists, as `assets` names them, in the order its tabs have them.
pub const KINDS: [&str; 6] = ["materials", "fill_types", "fill_patterns", "line_patterns", "line_styles", "pictures"];

/// Their names, for an error.
pub const KIND_NAMES: &str = "materials, fill_types, fill_patterns, line_patterns, line_styles or pictures";

/// The library kind each of [`KINDS`] is.
fn library_kind(kind: &str) -> Option<Kind> {
    Some(match kind {
        "materials" => Kind::Material,
        "fill_types" => Kind::FilledRegionType,
        "fill_patterns" => Kind::FillPattern,
        "line_patterns" => Kind::LinePattern,
        "line_styles" => Kind::LineStyle,
        "pictures" => Kind::Picture,
        _ => return None,
    })
}

/// What uses an asset: who names it, and where that is.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum User {
    /// An element of the project, by id.
    Element(Id),
    /// A type, by its category and name; `what` says which part of it (a layer, its panel).
    Type {
        category: String,
        name: String,
        what: String,
    },
    Family(String),
    Material(String),
    FillType(String),
    LineStyle(String),
    GridType(String),
    ViewTemplate(String),
    /// A member of a group's type, detail or model.
    Group(String),
    /// What a phase filter draws a status with.
    PhaseOverride(String),
}

impl User {
    /// As `assets` answers it: `{kind, name}` or `{kind: element, id, category}`, with `what` where it says more.
    fn json(&self, m: &Model) -> Value {
        match self {
            User::Element(id) => {
                let category = m.elements.get(*id).map_or("", |e| e.category());
                json!({"kind": "element", "id": id, "category": category})
            }
            User::Type { category, name, what } => json!({"kind": "type", "category": category, "name": name, "what": what}),
            User::Family(n) => json!({"kind": "family", "name": n}),
            User::Material(n) => json!({"kind": "material", "name": n}),
            User::FillType(n) => json!({"kind": "fill_type", "name": n}),
            User::LineStyle(n) => json!({"kind": "line_style", "name": n}),
            User::GridType(n) => json!({"kind": "grid_type", "name": n}),
            User::ViewTemplate(n) => json!({"kind": "view_template", "name": n}),
            User::Group(n) => json!({"kind": "group", "name": n}),
            User::PhaseOverride(n) => json!({"kind": "phase_override", "name": n}),
        }
    }
}

/// Who uses each asset of the project, by its kind and name: each user once, in order.
pub fn users(m: &Model) -> BTreeMap<(Kind, String), Vec<User>> {
    let mut out: BTreeMap<(Kind, String), Vec<User>> = BTreeMap::new();
    let mut add = |kind: &Kind, name: &str, who: User| {
        if library_kind_listed(kind) && !name.is_empty() {
            let list = out.entry((kind.clone(), name.to_string())).or_default();
            if !list.contains(&who) {
                list.push(who);
            }
        }
    };
    for (id, e) in m.elements.iter() {
        e.clone().lib_refs_mut(&mut |k, n| add(k, n, User::Element(*id)));
    }
    let t = &*m.types;
    let layered = [
        ("wall", t.wall.iter().map(|(n, x)| (n, &x.layers[..])).collect::<Vec<_>>()),
        ("floor", t.floor.iter().map(|(n, x)| (n, &x.layers[..])).collect()),
        ("roof", t.roof.iter().map(|(n, x)| (n, &x.layers[..])).collect()),
        ("ceiling", t.ceiling.iter().map(|(n, x)| (n, &x.layers[..])).collect()),
        ("toposolid", t.toposolid.iter().map(|(n, x)| (n, &x.layers[..])).collect()),
    ];
    for (category, types) in layered {
        for (name, layers) in types {
            for (i, l) in layers.iter().enumerate() {
                let what = format!("layer {}", i + 1);
                add(&Kind::Material, &l.material, User::Type { category: category.into(), name: name.clone(), what });
            }
        }
    }
    for (category, types) in t.runs.iter() {
        for (name, r) in types {
            add(&Kind::Material, &r.material, User::Type { category: category.clone(), name: name.clone(), what: "material".into() });
        }
    }
    for (name, x) in &t.curtain_panel {
        add(&Kind::Material, &x.material, User::Type { category: "curtain_panel".into(), name: name.clone(), what: "material".into() });
    }
    for (category, types) in [("mullion", &t.mullion), ("corner_mullion", &t.corner_mullion)] {
        for (name, x) in types {
            add(&Kind::Material, &x.material, User::Type { category: category.into(), name: name.clone(), what: "material".into() });
        }
    }
    for (name, f) in m.families.iter() {
        f.clone().material_refs_mut(&mut |mat| add(&Kind::Material, mat, User::Family(name.clone())));
    }
    for (name, x) in m.materials.iter() {
        if let Some(tx) = &x.texture {
            add(&Kind::Picture, &tx.image, User::Material(name.clone()));
        }
        for p in [&x.cut_pattern, &x.surface_pattern].into_iter().flatten() {
            add(&Kind::FillPattern, p, User::Material(name.clone()));
        }
    }
    for (name, x) in &m.region_types {
        for p in [&x.foreground, &x.background].into_iter().flatten() {
            add(&Kind::FillPattern, p, User::FillType(name.clone()));
        }
    }
    for (name, x) in &m.line_styles {
        if let Some(p) = &x.pattern {
            add(&Kind::LinePattern, p, User::LineStyle(name.clone()));
        }
    }
    for (name, x) in &m.grid_types {
        if let Some(p) = &x.line_pattern {
            add(&Kind::LinePattern, p, User::GridType(name.clone()));
        }
    }
    for (name, vt) in &m.view_templates {
        crate::library::graphics_refs(&mut vt.graphics.clone(), &mut |k, n| add(k, n, User::ViewTemplate(name.clone())));
    }
    for (name, g) in m.detail_groups.iter().chain(m.model_groups.iter()) {
        for member in &g.members {
            member.element.clone().lib_refs_mut(&mut |k, n| add(k, n, User::Group(name.clone())));
        }
    }
    for (status, o) in &m.phase_overrides {
        for p in o.clone().patterns_mut() {
            add(&Kind::FillPattern, p, User::PhaseOverride(format!("{status:?}")));
        }
    }
    out
}

/// Whether the Asset Manager lists entries of `kind`.
fn library_kind_listed(kind: &Kind) -> bool {
    KINDS.iter().any(|k| library_kind(k).as_ref() == Some(kind))
}

/// The names of the project's assets of one of [`KINDS`], in order.
pub fn names(m: &Model, kind: &str) -> Vec<String> {
    match kind {
        "materials" => m.materials.keys().cloned().collect(),
        "fill_types" => m.region_types.keys().cloned().collect(),
        "fill_patterns" => m.fill_patterns.keys().cloned().collect(),
        "line_patterns" => m.line_patterns.keys().cloned().collect(),
        "line_styles" => m.line_styles.keys().cloned().collect(),
        "pictures" => m.images.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// The list for each kind `kind` names, or for every kind: `{materials: [row, …], …}`, a row `{name, id, guid, uses,
/// used_by}`, and a picture's `size` (px) and `label`, the name it came in with.
pub fn list(m: &Model, kind: Option<&str>) -> Value {
    let users = users(m);
    let mut out = serde_json::Map::new();
    for k in KINDS.iter().filter(|k| kind.is_none_or(|want| want == **k)) {
        let Some(lk) = library_kind(k) else { continue };
        let rows: Vec<Value> = names(m, k)
            .into_iter()
            .map(|name| {
                let by: Vec<Value> = users.get(&(lk.clone(), name.clone())).into_iter().flatten().map(|u| u.json(m)).collect();
                let mut row = json!({
                    "name": name,
                    "id": m.library.id(&lk, &name),
                    "guid": m.library.guid(&lk, &name),
                    "uses": by.len(),
                    "used_by": by,
                });
                if let Some(p) = m.images.get(&name) {
                    let (w, h) = p.size();
                    row["size"] = json!([w, h]);
                    row["label"] = json!(p.name());
                }
                row
            })
            .collect();
        out.insert((*k).to_string(), Value::Array(rows));
    }
    Value::Object(out)
}

/// What `delete_assets` takes away, by kind and name: each refused while something uses it, saying what.
pub fn delete(m: &mut Model, chosen: &[(String, String)]) -> Result<Vec<(String, String)>, String> {
    let users = users(m);
    for (kind, name) in chosen {
        let Some(lk) = library_kind(kind) else { return Err(format!("`{kind}` is {KIND_NAMES}")) };
        if !names(m, kind).contains(name) {
            return Err(format!("the project has no {} named `{name}`", lk.label()));
        }
        // What else of the chosen uses it goes with it, so a material and its own picture are taken together.
        let by: Vec<String> = users
            .get(&(lk.clone(), name.clone()))
            .into_iter()
            .flatten()
            .filter(|u| !chosen.iter().any(|(k, n)| user_is(u, k, n)))
            .map(|u| describe(u, m))
            .collect();
        if !by.is_empty() {
            let more = if by.len() > 3 { format!(" and {} more", by.len() - 3) } else { String::new() };
            return Err(format!(
                "{} `{name}` is used by {}{more}: take it off them first",
                lk.label(),
                by.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    for (kind, name) in chosen {
        match kind.as_str() {
            "materials" => {
                m.materials.remove(name);
            }
            "fill_types" => {
                m.region_types.remove(name);
            }
            "fill_patterns" => {
                m.fill_patterns.remove(name);
            }
            "line_patterns" => {
                m.line_patterns.remove(name);
            }
            "line_styles" => {
                m.line_styles.remove(name);
            }
            _ => {
                m.images.remove(name);
                m.pdfs.remove(name);
            }
        }
    }
    m.sync_library();
    Ok(chosen.to_vec())
}

/// Whether `u` is asset `name` of `kind`.
fn user_is(u: &User, kind: &str, name: &str) -> bool {
    match u {
        User::Material(n) => kind == "materials" && n == name,
        User::FillType(n) => kind == "fill_types" && n == name,
        User::LineStyle(n) => kind == "line_styles" && n == name,
        _ => false,
    }
}

/// How a use reads in a message: `wall 100234`, `wall type Generic 200`, `material Oak`.
fn describe(u: &User, m: &Model) -> String {
    match u {
        User::Element(id) => format!("{} {id}", m.elements.get(*id).map_or("element", |e| e.category())),
        User::Type { category, name, .. } => format!("{category} type {name}"),
        User::Family(n) => format!("family {n}"),
        User::Material(n) => format!("material {n}"),
        User::FillType(n) => format!("fill type {n}"),
        User::LineStyle(n) => format!("line style {n}"),
        User::GridType(n) => format!("grid type {n}"),
        User::ViewTemplate(n) => format!("view template {n}"),
        User::Group(n) => format!("group {n}"),
        User::PhaseOverride(n) => format!("phase override {n}"),
    }
}
