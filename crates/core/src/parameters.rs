//! Everything done with project parameters as a set: dictionaries, import and export, batch changes, merging, what uses
//! each, and moving family parameters between families. The one parameter's own edit is the `parameter` command.
//!
//! A parameter's identity is its library GUID, whatever it is called here. A *dictionary* is a plain file of definitions
//! (`.bimparams`) a team keeps wherever it likes; a project holds a copy of the definitions it uses and which dictionary
//! each came from ([`ProjectParameter::dictionary`]), so it never needs the file to open, and a newer file changes nothing
//! until it is imported. Nothing here fetches anything: a dictionary is a local path.
//!
//! An import is read ([`table`]), planned ([`plan`]) and only then applied, so a dry run says what would happen, row by row,
//! with the same code that does it. Revit's shared parameter files are read and never written.

pub mod table;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::commands::{bad, category, flag, only, parameter, string};
use crate::engine::Session;
use crate::family::{self, Category};
use crate::ids::Guid;
use crate::library::{Entry, Kind as LibKind};
use crate::model::{Element, MAX_TEXT, Model, PARAMETER_CATEGORIES, ParameterKind, ProjectParameter, Scope};

type R = Result<Value, String>;

/// The extension of a dictionary's file.
pub const DICTIONARY_EXTENSION: &str = "bimparams";
/// The version of the dictionary file this BIMCraft writes; a file of a newer one is refused.
pub const DICTIONARY_FORMAT: u32 = 1;
/// The most changes one batch makes.
const MAX_OPS: usize = 8192;
/// How many conflicts a refusal names.
const NAMED: usize = 12;

/// What a parameter is, apart from any project: what a dictionary holds for it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guid: Option<Guid>,
    pub name: String,
    #[serde(default)]
    pub kind: ParameterKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    /// Each element's or each type's, where the file says: where it doesn't, a project keeps what it has, and a new one is an instance's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    /// The categories it is usually for, as the file names them; a project may take others.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
}

/// A dictionary's file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dictionary {
    pub format: u32,
    /// Made once, so a dictionary renamed is the same dictionary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guid: Option<Guid>,
    pub name: String,
    /// A label of the team's own: `2026 standard`. Nothing compares it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// Counts up with every change to the file: what tells a project it is behind.
    #[serde(default)]
    pub revision: u64,
    /// The day it last changed, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub changed: String,
    #[serde(default)]
    pub parameters: Vec<Definition>,
}

/// A dictionary a project has taken parameters from: where it was and the version it was at.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DictionaryRef {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// The file's revision when it was last imported.
    #[serde(default)]
    pub revision: u64,
}

/// The categories whose elements have no type: a type parameter can't be for them.
pub const TYPELESS: [&str; 4] = ["room", "stair", "ramp", "mass"];

/// The category of project parameters a family's elements are.
pub fn family_category(c: Category) -> &'static str {
    match c {
        Category::Door => "door",
        Category::Window => "window",
        _ => "component",
    }
}

/// The library entry of the type `e` is of: a system type's, or its family's. None for what has no type.
pub fn type_entry(e: &Element) -> Option<(LibKind, String)> {
    use crate::model::run_arm;
    if let Some((f, t)) = e.family() {
        return Some((LibKind::FamilyType, format!("{f}: {t}")));
    }
    let name = match e {
        Element::Wall(w) => &w.type_name,
        Element::Floor(x) => &x.type_name,
        Element::Ceiling(x) => &x.type_name,
        Element::Roof(x) => &x.type_name,
        Element::Railing(x) => &x.type_name,
        Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => &c.type_name,
        Element::Door(o) | Element::Window(o) => &o.type_name,
        run_arm!(r) => &r.type_name,
        _ => return None,
    };
    Some((LibKind::Type(e.category().to_string()), name.clone()))
}

/// The GUID of the type `e` is of.
pub fn type_guid(m: &Model, e: &Element) -> Option<Guid> {
    let (kind, name) = type_entry(e)?;
    m.library.guid(&kind, &name)
}

/// What `e` reads of project parameter `name`, which is `p`: its own, or its type's.
pub fn value_of<'a>(m: &Model, e: &'a Element, name: &str, p: &'a ProjectParameter) -> Option<&'a serde_json::Value> {
    match p.scope {
        Scope::Instance => e.params()?.get(name),
        Scope::Type => p.type_values.get(&type_guid(m, e)?),
    }
}

/// The category of parameters the library's type entry `kind` and `name` is for.
fn category_of_type(m: &Model, kind: &LibKind, name: &str) -> Option<String> {
    match kind {
        LibKind::Type(c) => Some(c.clone()),
        LibKind::FamilyType => {
            let (family, _) = name.split_once(": ")?;
            Some(family_category(m.families.get(family)?.category).to_string())
        }
        _ => None,
    }
}

/// A family with a parameter of `name` that isn't tied to `p`, the project's of that name.
pub(crate) fn family_clash<'a>(m: &'a Model, name: &str, p: &ProjectParameter) -> Option<&'a str> {
    let own = m.library.guid(&LibKind::Parameter, name);
    let ours = |g: Guid| Some(g) == own || p.same_as.contains(&g);
    m.families.iter().find(|(_, f)| f.parameters.iter().any(|q| q.name == name && !q.definition.is_some_and(ours))).map(|(n, _)| n.as_str())
}

/// A family with a parameter tied to the project's `name`.
pub(crate) fn family_tied<'a>(m: &'a Model, name: &str) -> Option<&'a str> {
    let own = m.library.guid(&LibKind::Parameter, name)?;
    m.families.iter().find(|(_, f)| f.parameters.iter().any(|q| q.definition == Some(own))).map(|(n, _)| n.as_str())
}

/// `p`'s values of types no longer of its categories, taken off.
pub(crate) fn keep_type_values_of_categories(m: &Model, p: &mut ProjectParameter) {
    if p.type_values.is_empty() {
        return;
    }
    let categories = p.categories.clone();
    p.type_values.retain(|g, _| {
        m.library.by_guid(*g).and_then(|(_, e)| category_of_type(m, &e.kind, &e.name)).is_some_and(|c| categories.contains(&c))
    });
}

/// `p`, the parameter `name`, made `scope`: to a type's, its elements' values are wiped; to an instance's, each type's value is
/// copied onto its elements. What happened and to how many.
pub(crate) fn switch_scope(m: &mut Model, name: &str, p: &mut ProjectParameter, scope: Scope) -> (&'static str, usize) {
    let families = m.families.clone();
    let theirs = |e: &Element| e.family().and_then(|(f, _)| families.get(f)).is_some_and(|f| f.parameters.iter().any(|q| q.name == name));
    let mut n = 0;
    p.scope = scope;
    if scope == Scope::Type {
        for e in m.elements.values_mut() {
            if p.categories.iter().any(|c| c == e.category())
                && !theirs(e)
                && let Some(params) = e.params_mut()
                && params.remove(name).is_some()
            {
                n += 1;
            }
        }
        return ("wiped", n);
    }
    let held = std::mem::take(&mut p.type_values);
    let guids: Vec<(crate::ids::Id, Guid)> = m
        .elements
        .iter()
        .filter(|(_, e)| p.categories.iter().any(|c| c == e.category()) && !theirs(e))
        .filter_map(|(id, e)| Some((*id, type_guid(m, e)?)))
        .collect();
    for (id, g) in guids {
        if let (Some(v), Some(params)) = (held.get(&g), m.elements.get_mut(id).and_then(Element::params_mut)) {
            params.insert(name.to_string(), v.clone());
            n += 1;
        }
    }
    ("copied", n)
}

/// A parameter's kind, as a family's parameter holds it: an integer, an area and a volume go in a family as numbers.
pub fn family_kind(k: ParameterKind) -> family::Kind {
    match k {
        ParameterKind::Text => family::Kind::Text,
        ParameterKind::Length => family::Kind::Length,
        ParameterKind::Angle => family::Kind::Angle,
        ParameterKind::YesNo => family::Kind::YesNo,
        ParameterKind::Number | ParameterKind::Integer | ParameterKind::Area | ParameterKind::Volume => family::Kind::Number,
    }
}

/// A family's parameter's kind, as a definition holds it: a material's name as text.
pub fn definition_kind(k: family::Kind) -> ParameterKind {
    match k {
        family::Kind::Text | family::Kind::Material => ParameterKind::Text,
        family::Kind::Length => ParameterKind::Length,
        family::Kind::Angle => ParameterKind::Angle,
        family::Kind::YesNo => ParameterKind::YesNo,
        family::Kind::Number => ParameterKind::Number,
    }
}

/// The word a table gives a kind.
pub fn kind_word(k: ParameterKind) -> &'static str {
    match k {
        ParameterKind::Text => "text",
        ParameterKind::Number => "number",
        ParameterKind::Integer => "integer",
        ParameterKind::Length => "length",
        ParameterKind::Area => "area",
        ParameterKind::Volume => "volume",
        ParameterKind::Angle => "angle",
        ParameterKind::YesNo => "yes/no",
    }
}

/// Categories that are ours under another program's name, Revit's above all.
const NAMED_CATEGORIES: [(&str, &str); 22] = [
    ("walls", "wall"),
    ("floors", "floor"),
    ("ceilings", "ceiling"),
    ("roofs", "roof"),
    ("doors", "door"),
    ("windows", "window"),
    ("rooms", "room"),
    ("columns", "column"),
    ("structuralcolumns", "structuralcolumn"),
    ("structuralfoundations", "footing"),
    ("structuralfoundation", "footing"),
    ("structuralframing", "beam"),
    ("stairs", "stair"),
    ("ramps", "ramp"),
    ("railings", "railing"),
    ("mass", "mass"),
    ("pipes", "pipe"),
    ("ducts", "duct"),
    ("conduits", "conduit"),
    ("cabletrays", "cabletray"),
    ("wires", "wire"),
    ("cables", "cable"),
];

/// Categories we have no category of, whose elements here are components: a parameter for one of them is for all components.
const BROAD_CATEGORIES: [&str; 24] = [
    "furniture",
    "furnituresystems",
    "casework",
    "plumbingfixtures",
    "mechanicalequipment",
    "electricalequipment",
    "electricalfixtures",
    "lightingfixtures",
    "lightingdevices",
    "genericmodels",
    "specialtyequipment",
    "sprinklers",
    "firealarmdevices",
    "communicationdevices",
    "datadevices",
    "securitydevices",
    "nursecalldevices",
    "telephonedevices",
    "planting",
    "entourage",
    "site",
    "parking",
    "medicalequipment",
    "foodserviceequipment",
];

/// The category `raw` names by itself, and whether it stands for more than that category holds.
pub fn match_category(raw: &str) -> Option<(&'static str, bool)> {
    let trimmed = raw.trim();
    let bare = trimmed.strip_prefix("OST_").unwrap_or(trimmed);
    let key: String = bare.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    if let Some(c) = PARAMETER_CATEGORIES.iter().find(|c| **c == key) {
        return Some((c, false));
    }
    if let Some((_, c)) = NAMED_CATEGORIES.iter().find(|(n, _)| *n == key) {
        return Some((c, false));
    }
    BROAD_CATEGORIES.contains(&key.as_str()).then_some(("component", true))
}

/// `name` rewritten in `formula` to `new`, whole names only. A formula's names may hold spaces, so what stands next to it across
/// spaces is part of the same name: `Height` isn't in `Height2`, `Finish Height` or `Height 2`.
pub fn rename_in_formula(formula: &str, name: &str, new: &str) -> String {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    if name.is_empty() {
        return formula.to_string();
    }
    let mut out = String::with_capacity(formula.len());
    let mut from = 0;
    while let Some(at) = formula.get(from..).and_then(|r| r.find(name)).map(|i| i + from) {
        let end = at + name.len();
        out.push_str(formula.get(from..at).unwrap_or(""));
        let before = formula.get(..at).unwrap_or("").trim_end().chars().next_back();
        let after = formula.get(end..).unwrap_or("").trim_start().chars().next();
        out.push_str(if before.is_none_or(|c| !word(c)) && after.is_none_or(|c| !word(c)) { new } else { name });
        from = end;
    }
    out.push_str(formula.get(from..).unwrap_or(""));
    out
}

/// Whether `formula` names `name`, as a whole word.
pub fn formula_names(formula: &str, name: &str) -> bool {
    rename_in_formula(formula, name, "\u{1}") != formula
}

/// What `e` holds of parameter `old`, held under `new`: its value, and a schedule's calculated fields. (What the library's
/// names reach, a schedule's fields and filters and a tag's label, `Element::lib_refs_mut` reaches.)
pub(crate) fn rename_values(e: &mut Element, old: &str, new: &str) {
    if let Element::Schedule(sc) = e {
        for f in &mut sc.fields {
            if let Some(text) = &mut f.formula {
                *text = rename_in_formula(text, old, new);
            }
        }
    }
    if let Some(params) = e.params_mut()
        && let Some(v) = params.remove(old)
    {
        params.insert(new.to_string(), v);
    }
}

/// A row of an import, once planned: what happens to it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Planned {
    /// What the file calls it.
    pub name: String,
    /// What it will be called here.
    pub here: String,
    pub guid: Option<Guid>,
    pub kind: ParameterKind,
    /// Each element's or each type's, as it will be.
    pub scope: Scope,
    /// `create`, `modify`, `existing`, `conflict` or `skip`.
    pub status: &'static str,
    /// What changes, or what stands in the way.
    pub notes: Vec<String>,
    /// A name taken here by another GUID: it waits for `same`, `separate` or `skip`.
    pub decide: bool,
    /// The categories it will be for.
    pub categories: Vec<String>,
    /// The file's names for categories no category here answers.
    pub unmapped: Vec<String>,
    /// The file's names for categories that land on a wider one here.
    pub broad: Vec<String>,
    #[serde(skip)]
    definition: Definition,
    #[serde(skip)]
    same_as: Option<Guid>,
    #[serde(skip)]
    existing: Option<String>,
}

/// What a person, or an agent, has said about an import.
#[derive(Debug, Default)]
pub struct Choices {
    /// The file's names for categories, each to a category here or, none, to be left out.
    pub map: BTreeMap<String, Option<String>>,
    /// A parameter here of the same name and another GUID: `same`, `separate` or `skip`.
    pub resolve: BTreeMap<String, String>,
    /// The categories of the rows that name none.
    pub categories: Vec<String>,
    /// Take off the categories the file doesn't list.
    pub replace_categories: bool,
    /// Instance or type, by the file's name, over what the file says.
    pub scopes: BTreeMap<String, Scope>,
    /// Only these, by the file's name.
    pub names: Option<BTreeSet<String>>,
}

/// The parameter here that `guid` is: its own, or one it has been said to be the same as.
pub fn here_by_guid(m: &Model, guid: Guid) -> Option<String> {
    if let Some((_, e)) = m.library.by_guid(guid)
        && e.kind == LibKind::Parameter
    {
        return Some(e.name.clone());
    }
    m.parameters.iter().find(|(_, p)| p.same_as.contains(&guid)).map(|(n, _)| n.clone())
}

/// A name this project has free, from `base`: itself, else ` 2`, ` 3`…
fn free_name(m: &Model, base: &str, taken: &BTreeSet<String>) -> String {
    std::iter::once(base.to_string())
        .chain((2..).map(|i| format!("{base} {i}")))
        .find(|n| !m.parameters.contains_key(n) && !taken.contains(n))
        .unwrap_or_else(|| base.to_string())
}

/// What `rows` would do to `m`, as `choices` say. Changes nothing.
pub fn plan(m: &Model, rows: &[Definition], choices: &Choices) -> Vec<Planned> {
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for row in rows {
        if choices.names.as_ref().is_some_and(|n| !n.contains(&row.name)) {
            continue;
        }
        let mut one = Planned { name: row.name.clone(), guid: row.guid, kind: row.kind, definition: row.clone(), ..Default::default() };
        let said = choices.scopes.get(&row.name).copied().or(row.scope);
        // A row that stops short still says what it would be.
        one.scope = said.unwrap_or_default();
        // The categories, through the map, then by their names.
        let mut cats: BTreeSet<String> = BTreeSet::new();
        for raw in &row.categories {
            match choices.map.get(raw) {
                Some(Some(ours)) if PARAMETER_CATEGORIES.contains(&category(ours).as_str()) => {
                    cats.insert(category(ours));
                }
                Some(_) => {}
                None => match m.parameter_map.get(raw) {
                    Some(Some(ours)) if PARAMETER_CATEGORIES.contains(&ours.as_str()) => {
                        cats.insert(ours.clone());
                    }
                    Some(_) => {}
                    None => match match_category(raw) {
                        Some((c, broad)) => {
                            cats.insert(c.to_string());
                            if broad {
                                one.broad.push(raw.clone());
                            }
                        }
                        None => one.unmapped.push(raw.clone()),
                    },
                },
            }
        }
        if row.categories.is_empty() {
            cats.extend(choices.categories.iter().map(|c| category(c)).filter(|c| PARAMETER_CATEGORIES.contains(&c.as_str())));
        }
        let clean = crate::import::parameter_name(&row.name).unwrap_or_default();
        if clean.is_empty() {
            one.status = "skip";
            one.notes.push("a name starts with a letter or `_`; this one can't be made into one".into());
            out.push(one);
            continue;
        }
        if clean != row.name {
            one.notes.push(format!("named `{clean}`: a name holds letters, digits, `_` and spaces"));
        }
        let by_guid = row.guid.and_then(|g| here_by_guid(m, g));
        let by_name = m.parameters.contains_key(&clean).then(|| clean.clone());
        let resolved = choices.resolve.get(&row.name).map(String::as_str);
        // Which parameter here it is, if any.
        let mut here = by_guid.clone();
        if here.is_none() && by_name.is_some() {
            match (row.guid, resolved) {
                // A table with no GUIDs says who it is by name.
                (None, _) | (_, Some("same")) => {
                    here = by_name.clone();
                    if row.guid.is_some() {
                        one.same_as = row.guid;
                        one.notes.push(format!("the same as `{clean}` here, from now on"));
                    }
                }
                (_, Some("skip")) => {
                    one.status = "skip";
                    one.here = clean;
                    out.push(one);
                    continue;
                }
                (_, Some("separate")) => {}
                _ => {
                    one.status = "conflict";
                    one.decide = true;
                    one.here = clean.clone();
                    one.notes.push(format!("`{clean}` here is another parameter, with another GUID: is it the same one?"));
                    out.push(one);
                    continue;
                }
            }
        }
        match here {
            Some(name) => {
                let Some(p) = m.parameters.get(&name) else { continue };
                one.existing = Some(name.clone());
                let mut wanted = cats.clone();
                if !choices.replace_categories || row.categories.is_empty() {
                    wanted.extend(p.categories.iter().cloned());
                }
                // The file's name is the parameter's only if its GUID is the parameter's own, not one it was said to be.
                let own = row.guid.is_some_and(|g| m.library.by_guid(g).is_some_and(|(_, e)| e.kind == LibKind::Parameter));
                let new_name = if own && clean != name && !m.parameters.contains_key(&clean) { clean.clone() } else { name.clone() };
                if new_name != name {
                    one.notes.push(format!("renamed from `{name}`"));
                }
                if own && clean != name && m.parameters.contains_key(&clean) {
                    one.notes.push(format!("stays `{name}`: `{clean}` here is another parameter"));
                }
                if row.kind != p.kind {
                    if has_values(m, &name) {
                        one.status = "conflict";
                        one.notes.push(format!(
                            "`{name}` holds {} here and has values; the file says {}. Take them away first",
                            kind_word(p.kind),
                            kind_word(row.kind)
                        ));
                    } else {
                        one.notes.push(format!("{} instead of {}", kind_word(row.kind), kind_word(p.kind)));
                    }
                }
                for (word, changed) in [
                    ("description", !row.description.is_empty() && row.description != p.description),
                    ("group", !row.group.is_empty() && row.group != p.group),
                    ("whether it can be changed", row.read_only != p.read_only),
                ] {
                    if changed {
                        one.notes.push(format!("its {word}"));
                    }
                }
                one.scope = said.unwrap_or(p.scope);
                if one.scope != p.scope {
                    one.notes.push(match one.scope {
                        Scope::Type => "a type parameter instead of an instance one: its elements' values are wiped".to_string(),
                        Scope::Instance => {
                            "an instance parameter instead of a type one: each type's value is copied to its elements".to_string()
                        }
                    });
                }
                let now: BTreeSet<String> = p.categories.iter().cloned().collect();
                let added: Vec<&String> = wanted.difference(&now).collect();
                let gone: Vec<&String> = now.difference(&wanted).collect();
                if !added.is_empty() {
                    one.notes.push(format!("for {} too", join(&added)));
                }
                if !gone.is_empty() {
                    one.notes.push(format!("no longer for {}: their elements lose their values", join(&gone)));
                }
                if wanted.is_empty() {
                    one.status = "conflict";
                    one.notes.push("it needs a category".into());
                }
                if one.status.is_empty() {
                    let changed = one.notes.iter().any(|n| !n.starts_with("stays ") && !n.starts_with("named "));
                    one.status = if changed { "modify" } else { "existing" };
                }
                one.here = new_name;
                one.categories = wanted.into_iter().collect();
            }
            None => {
                one.scope = said.unwrap_or_default();
                one.here = free_name(m, &clean, &taken);
                if one.here != clean {
                    one.notes.push(format!("kept apart from `{clean}` here, as `{}`", one.here));
                }
                if cats.is_empty() {
                    one.status = "conflict";
                    one.notes.push("it needs a category: give it one, or map the file's".into());
                } else {
                    one.status = "create";
                }
                one.categories = cats.into_iter().collect();
            }
        }
        if one.status != "conflict"
            && one.scope == Scope::Type
            && let Some(c) = one.categories.iter().find(|c| TYPELESS.contains(&c.as_str()))
        {
            let note = format!("{c}s have no types: make it an instance parameter, or take {c} off");
            one.status = "conflict";
            one.notes.push(note);
        }
        if one.status != "conflict"
            && (crate::paint::reserved(&one.here) || crate::phasing::reserved(&one.here) || crate::massing::reserved(&one.here))
        {
            one.status = "conflict";
            one.notes.push(format!("elements hold other things under `{}`; rename it in the file", one.here));
        }
        if one.status != "conflict"
            && let Some(c) = one.categories.iter().find(|c| crate::schedule::own_field(c, &one.here))
        {
            let note = format!("{c}s have a `{}` of their own; rename it in the file", one.here);
            one.status = "conflict";
            one.notes.push(note);
        }
        if one.status != "conflict"
            && one.categories.iter().any(|c| c == "component")
            && let Some(f) = m.families.iter().find_map(|(n, f)| {
                let tied = |g: Guid| Some(g) == row.guid || m.library.guid(&LibKind::Parameter, &one.here) == Some(g);
                f.parameters.iter().any(|q| q.name == one.here && !q.definition.is_some_and(tied)).then_some(n)
            })
        {
            let note = format!("family `{f}` has a parameter `{}`; rename it in the file", one.here);
            one.status = "conflict";
            one.notes.push(note);
        }
        taken.insert(one.here.clone());
        out.push(one);
    }
    out
}

fn join<S: std::fmt::Display>(items: &[S]) -> String {
    items.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
}

/// Whether an element of its categories holds a value of project parameter `name`.
fn has_values(m: &Model, name: &str) -> bool {
    let Some(p) = m.parameters.get(name) else { return false };
    !p.type_values.is_empty() || m.elements.values().any(|e| holds_value(m, e, name) && p.categories.iter().any(|c| c == e.category()))
}

/// Whether `e` holds a value of `name` that's the project parameter's, not its family's parameter of that name.
fn holds_value(m: &Model, e: &Element, name: &str) -> bool {
    let theirs = e.family().and_then(|(f, _)| m.families.get(f)).is_some_and(|f| f.parameters.iter().any(|q| q.name == name));
    !theirs && e.params().is_some_and(|ps| ps.contains_key(name))
}

/// The JSON a dry run or an import answers with.
fn report(rows: &[Planned], read: &table::Read, dry_run: bool) -> Value {
    let count = |s: &str| rows.iter().filter(|r| r.status == s).count();
    let collect = |f: &dyn Fn(&Planned) -> &Vec<String>| -> Vec<String> {
        let all: BTreeSet<&String> = rows.iter().flat_map(|r| f(r).iter()).collect();
        all.into_iter().cloned().collect()
    };
    json!({
        "dry_run": dry_run,
        "dictionary": read.dictionary.as_ref().map(|d| json!({"name": d.name, "version": d.version, "revision": d.revision})),
        "rows": rows,
        "create": count("create"),
        "modify": count("modify"),
        "existing": count("existing"),
        "conflict": count("conflict"),
        "skip": count("skip"),
        "unmapped": collect(&|r| &r.unmapped),
        "broad": collect(&|r| &r.broad),
        "left_out": read.left_out,
    })
}

/// The `parameter_import` command.
pub(crate) fn import(s: &mut Session, p: &Value) -> R {
    let c = "parameter_import";
    only(c, p, &["path", "names", "map", "resolve", "scopes", "categories", "replace_categories", "skip_conflicts", "dry_run"])?;
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is required"))?;
    let read = table::read_file(&path).map_err(|e| bad(c, e))?;
    let mut choices = Choices { replace_categories: flag(c, p, "replace_categories")?, ..Choices::default() };
    if let Some(v) = p.get("map").filter(|v| !v.is_null()) {
        choices.map =
            serde_json::from_value(v.clone()).map_err(|_| bad(c, "`map` gives each of the file's categories one of ours, or null"))?;
        if let Some(bad_one) = choices.map.values().flatten().find(|o| !PARAMETER_CATEGORIES.contains(&category(o).as_str())) {
            return Err(bad(c, format!("`{bad_one}` isn't a category it can be for; there are {}", PARAMETER_CATEGORIES.join(", "))));
        }
    }
    if let Some(v) = p.get("resolve").filter(|v| !v.is_null()) {
        choices.resolve = serde_json::from_value(v.clone()).map_err(|_| bad(c, "`resolve` gives a name `same`, `separate` or `skip`"))?;
        if let Some((n, what)) = choices.resolve.iter().find(|(_, w)| !matches!(w.as_str(), "same" | "separate" | "skip")) {
            return Err(bad(c, format!("`{n}`: `{what}` isn't `same`, `separate` or `skip`")));
        }
    }
    if let Some(v) = p.get("scopes").filter(|v| !v.is_null()) {
        choices.scopes = serde_json::from_value(v.clone()).map_err(|_| bad(c, "`scopes` gives a name `instance` or `type`"))?;
    }
    if let Some(v) = p.get("categories").filter(|v| !v.is_null()) {
        choices.categories = serde_json::from_value(v.clone()).map_err(|_| bad(c, "`categories` is a list of categories"))?;
        if let Some(k) = choices.categories.iter().find(|k| !PARAMETER_CATEGORIES.contains(&category(k).as_str())) {
            return Err(bad(c, format!("`{k}` isn't a category it can be for; there are {}", PARAMETER_CATEGORIES.join(", "))));
        }
    }
    if let Some(v) = p.get("names").filter(|v| !v.is_null()) {
        choices.names = Some(serde_json::from_value(v.clone()).map_err(|_| bad(c, "`names` is a list of the file's names"))?);
    }
    let dry_run = flag(c, p, "dry_run")?;
    let mut rows = plan(&s.model, &read.definitions, &choices);
    if dry_run {
        return Ok(report(&rows, &read, true));
    }
    let conflicts: Vec<&Planned> = rows.iter().filter(|r| r.status == "conflict").collect();
    if !conflicts.is_empty() && !flag(c, p, "skip_conflicts")? {
        let named: Vec<String> = conflicts.iter().take(NAMED).map(|r| format!("`{}`: {}", r.name, r.notes.join("; "))).collect();
        return Err(bad(c, format!("{} rows need a decision: {}", conflicts.len(), named.join(" | "))));
    }
    rows.retain(|r| r.status != "conflict");
    apply(s, &rows, read.dictionary.as_ref(), &path)?;
    // The mapping stays with the project, for the next file.
    if !choices.map.is_empty() {
        let m = s.model_mut();
        for (raw, ours) in &choices.map {
            m.parameter_map.insert(raw.clone(), ours.as_ref().map(|o| category(o)));
        }
    }
    Ok(report(&rows, &read, false))
}

/// `rows` done to the session's model.
fn apply(s: &mut Session, rows: &[Planned], dictionary: Option<&table::Source>, path: &str) -> Result<(), String> {
    let c = "parameter_import";
    let dict_name = dictionary.map(|d| d.name.clone());
    if let Some(d) = dictionary {
        let m = s.model_mut();
        m.dictionaries.insert(d.name.clone(), DictionaryRef { path: path.to_string(), version: d.version.clone(), revision: d.revision });
        // Its GUID, when the file has one, is what the library knows it by.
        if let Some(g) = d.guid
            && m.library.id(&LibKind::Dictionary, &d.name).is_none()
            && m.library.by_guid(g).is_none()
            && !m.elements.contains_key(g)
        {
            let id = m.fresh_library_id();
            m.library.insert(id, Entry { kind: LibKind::Dictionary, name: d.name.clone(), guid: g })?;
        }
    }
    for r in rows {
        match r.status {
            "create" => {
                let m = s.model_mut();
                if m.parameters.len() >= crate::model::MAX_PROJECT_PARAMETERS {
                    return Err(bad(c, format!("a project holds up to {} parameters", crate::model::MAX_PROJECT_PARAMETERS)));
                }
                let d = &r.definition;
                m.parameters.insert(
                    r.here.clone(),
                    ProjectParameter {
                        kind: d.kind,
                        categories: r.categories.clone(),
                        description: clip(&d.description),
                        group: clip(&d.group),
                        read_only: d.read_only,
                        scope: r.scope,
                        type_values: BTreeMap::new(),
                        dictionary: dict_name.clone(),
                        same_as: Vec::new(),
                    },
                );
                if let Some(g) = d.guid
                    && m.library.by_guid(g).is_none()
                    && !m.elements.contains_key(g)
                    && m.library.id(&LibKind::Parameter, &r.here).is_none()
                {
                    let id = m.fresh_library_id();
                    m.library.insert(id, Entry { kind: LibKind::Parameter, name: r.here.clone(), guid: g })?;
                }
            }
            "modify" => {
                let Some(old) = r.existing.clone() else { continue };
                let d = &r.definition;
                let mut edit = json!({"name": old, "kind": d.kind, "categories": r.categories, "scope": r.scope});
                if r.here != old {
                    edit["rename"] = json!(r.here);
                }
                parameter(s, &edit).map_err(|e| bad(c, e))?;
                let m = s.model_mut();
                if let Some(q) = m.parameters.get_mut(&r.here) {
                    if !d.description.is_empty() {
                        q.description = clip(&d.description);
                    }
                    if !d.group.is_empty() {
                        q.group = clip(&d.group);
                    }
                    q.read_only = d.read_only;
                    if dict_name.is_some() {
                        q.dictionary = dict_name.clone();
                    }
                    if let Some(g) = r.same_as
                        && !q.same_as.contains(&g)
                    {
                        q.same_as.push(g);
                    }
                }
            }
            _ => {
                // Nothing to change but where it comes from, and who else it is.
                let Some(old) = r.existing.as_ref() else { continue };
                if dict_name.is_some() || r.same_as.is_some() {
                    let m = s.model_mut();
                    if let Some(q) = m.parameters.get_mut(old) {
                        if dict_name.is_some() {
                            q.dictionary = dict_name.clone();
                        }
                        if let Some(g) = r.same_as
                            && !q.same_as.contains(&g)
                        {
                            q.same_as.push(g);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn clip(t: &str) -> String {
    t.chars().take(MAX_TEXT).collect()
}

/// A parameter of the project as a dictionary or a table holds it.
pub fn definition(m: &Model, name: &str) -> Option<Definition> {
    let p = m.parameters.get(name)?;
    Some(Definition {
        guid: m.library.guid(&LibKind::Parameter, name),
        name: name.to_string(),
        kind: p.kind,
        description: p.description.clone(),
        group: p.group.clone(),
        read_only: p.read_only,
        scope: Some(p.scope),
        categories: p.categories.clone(),
    })
}

/// The project's parameters `names` names, or all of them; an error for a name it hasn't.
fn chosen(c: &str, m: &Model, p: &Value) -> Result<Vec<Definition>, String> {
    let names: Vec<String> = match p.get("names").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad(c, "`names` is a list of parameters' names"))?,
        None => m.parameters.keys().cloned().collect(),
    };
    names.iter().map(|n| definition(m, n).ok_or_else(|| bad(c, format!("there's no parameter `{n}`")))).collect()
}

/// The `parameter_export` command.
pub(crate) fn export(s: &mut Session, p: &Value) -> R {
    let c = "parameter_export";
    only(c, p, &["path", "names", "definitions", "name", "version"])?;
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is required"))?;
    let mut m = (*s.model).clone();
    m.sync_library();
    let defs = match p.get("definitions").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad(c, format!("`definitions`: {e}")))?,
        None => chosen(c, &m, p)?,
    };
    let lower = path.to_lowercase();
    let bytes = if lower.ends_with(".txt") {
        return Err(bad(c, "BIMCraft reads Revit's shared parameter files and doesn't write them; export .xlsx, .csv or .bimparams"));
    } else if lower.ends_with(".xlsx") {
        table::to_xlsx(&defs)
    } else if lower.ends_with(".csv") {
        table::to_csv(&defs).into_bytes()
    } else if lower.ends_with(&format!(".{DICTIONARY_EXTENSION}")) {
        let name = string(c, p, "name")?.unwrap_or_else(|| table::stem(&path));
        let version = string(c, p, "version")?.unwrap_or_default();
        let d = Dictionary {
            format: DICTIONARY_FORMAT,
            guid: Some(Guid::new()),
            name,
            version,
            revision: 1,
            changed: today(),
            parameters: defs.clone(),
        };
        serde_json::to_vec_pretty(&d).map_err(|e| bad(c, e))?
    } else {
        return Err(bad(c, "a path ending .xlsx, .csv or .bimparams"));
    };
    std::fs::write(&path, bytes).map_err(|e| bad(c, format!("{path}: {e}")))?;
    Ok(json!({"path": path, "parameters": defs.len()}))
}

/// The `dictionary` command: makes or changes a dictionary's file. The project isn't touched.
pub(crate) fn dictionary(s: &mut Session, p: &Value) -> R {
    let c = "dictionary";
    only(c, p, &["path", "name", "version", "add", "names", "remove", "merge", "skip_conflicts"])?;
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is required"))?;
    if !path.to_lowercase().ends_with(&format!(".{DICTIONARY_EXTENSION}")) {
        return Err(bad(c, format!("a dictionary's path ends .{DICTIONARY_EXTENSION}")));
    }
    let fresh = !std::path::Path::new(&path).exists();
    let mut d = if !fresh {
        table::read_dictionary_file(&path).map_err(|e| bad(c, e))?
    } else {
        Dictionary { format: DICTIONARY_FORMAT, guid: Some(Guid::new()), name: table::stem(&path), ..Dictionary::default() }
    };
    let was = d.clone();
    if let Some(n) = string(c, p, "name")? {
        d.name = clip(&n);
    }
    if let Some(v) = string(c, p, "version")? {
        d.version = clip(&v);
    }
    let mut conflicts = Vec::new();
    let put = |d: &mut Dictionary, def: Definition, conflicts: &mut Vec<String>| {
        let at = def.guid.and_then(|g| d.parameters.iter().position(|q| q.guid == Some(g)));
        if let Some(i) = at {
            if let Some(q) = d.parameters.get_mut(i) {
                *q = def;
            }
        } else if d.parameters.iter().any(|q| q.name == def.name) {
            conflicts.push(def.name);
        } else {
            let mut def = def;
            def.guid = def.guid.or_else(|| Some(Guid::new()));
            d.parameters.push(def);
        }
    };
    // Whole files first.
    if let Some(v) = p.get("merge").filter(|v| !v.is_null()) {
        let files: Vec<String> = serde_json::from_value(v.clone()).map_err(|_| bad(c, "`merge` is a list of paths"))?;
        for f in files {
            let read = table::read_file(&f).map_err(|e| bad(c, e))?;
            for def in read.definitions {
                put(&mut d, def, &mut conflicts);
            }
        }
    }
    if p.get("names").is_some_and(|v| !v.is_null()) {
        let mut m = (*s.model).clone();
        m.sync_library();
        for def in chosen(c, &m, p)? {
            put(&mut d, def, &mut conflicts);
        }
    }
    if let Some(v) = p.get("add").filter(|v| !v.is_null()) {
        let list: Vec<Definition> = serde_json::from_value(v.clone()).map_err(|e| bad(c, format!("`add`: {e}")))?;
        for def in list {
            if crate::import::parameter_name(&def.name).as_deref() != Some(def.name.as_str()) {
                return Err(bad(c, format!("`{}`: a name starts with a letter and holds letters, digits, `_` and spaces", def.name)));
            }
            put(&mut d, def, &mut conflicts);
        }
    }
    if let Some(v) = p.get("remove").filter(|v| !v.is_null()) {
        let list: Vec<String> = serde_json::from_value(v.clone()).map_err(|_| bad(c, "`remove` is a list of names or GUIDs"))?;
        d.parameters.retain(|q| !list.contains(&q.name) && !q.guid.is_some_and(|g| list.contains(&g.to_string())));
    }
    if !conflicts.is_empty() && !flag(c, p, "skip_conflicts")? {
        conflicts.truncate(NAMED);
        return Err(bad(c, format!("the dictionary has another parameter of each of these names: {}", conflicts.join(", "))));
    }
    if d.parameters.len() > table::MAX_ROWS {
        return Err(bad(c, format!("a dictionary holds up to {} parameters", table::MAX_ROWS)));
    }
    d.parameters.sort_by(|a, b| a.name.cmp(&b.name));
    if let Some(w) = d.parameters.windows(2).find(|w| w[0].name == w[1].name) {
        return Err(bad(c, format!("two definitions would be called `{}`", w[0].name)));
    }
    // Only a change counts.
    let mut before = was;
    before.parameters.sort_by(|a, b| a.name.cmp(&b.name));
    if fresh || d != before {
        d.revision += 1;
        d.changed = today();
    }
    if fresh || d != before {
        let text = serde_json::to_vec_pretty(&d).map_err(|e| bad(c, e))?;
        std::fs::write(&path, text).map_err(|e| bad(c, format!("{path}: {e}")))?;
    }
    Ok(
        json!({"path": path, "name": d.name, "version": d.version, "revision": d.revision, "changed": d.changed, "guid": d.guid, "parameters": d.parameters}),
    )
}

/// The `parameter_batch` command: several changes at once, all or none.
pub(crate) fn batch(s: &mut Session, p: &Value) -> R {
    let c = "parameter_batch";
    only(c, p, &["ops", "dry_run"])?;
    let ops = p.get("ops").and_then(Value::as_array).ok_or_else(|| bad(c, "`ops` is a list of changes"))?;
    if ops.len() > MAX_OPS {
        return Err(bad(c, format!("up to {MAX_OPS} changes at once")));
    }
    let dry_run = flag(c, p, "dry_run")?;
    let before = std::sync::Arc::clone(&s.model);
    let mut done = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        let what = op.get("op").and_then(Value::as_str).unwrap_or("set");
        let mut body = op.clone();
        if let Some(o) = body.as_object_mut() {
            o.remove("op");
        }
        let at = |e: String| bad(c, format!("change {}: {e}", i + 1));
        match what {
            "set" => {
                parameter(s, &body).map_err(at)?;
                done.push(json!({"op": "set", "name": body.get("rename").or(body.get("name"))}));
            }
            "delete" => {
                let name = body.get("name").and_then(Value::as_str).ok_or_else(|| at("`name` is required".into()))?.to_string();
                parameter(s, &json!({"name": name, "delete": true})).map_err(at)?;
                done.push(json!({"op": "delete", "name": name}));
            }
            "duplicate" => {
                let name = body.get("name").and_then(Value::as_str).ok_or_else(|| at("`name` is required".into()))?.to_string();
                let m = s.model_mut();
                let was = m.parameters.get(&name).cloned().ok_or_else(|| at(format!("there's no parameter `{name}`")))?;
                let base = body.get("as").and_then(Value::as_str).map_or_else(|| format!("{name} Copy"), str::to_string);
                let new = free_name(m, &base, &BTreeSet::new());
                // A duplicate is a parameter of its own: its own GUID (made by the sync), and from no dictionary.
                m.parameters
                    .insert(new.clone(), ProjectParameter { dictionary: None, same_as: Vec::new(), type_values: BTreeMap::new(), ..was });
                done.push(json!({"op": "duplicate", "name": name, "as": new}));
            }
            "rename" => {
                let names: Vec<String> = body
                    .get("names")
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .ok_or_else(|| at("`names` is a list of parameters".into()))?;
                let text_of = |k: &str| body.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let (prefix, suffix, find, replace) = (text_of("prefix"), text_of("suffix"), text_of("find"), text_of("replace"));
                for name in names {
                    let mid = if find.is_empty() { name.clone() } else { name.replace(&find, &replace) };
                    let new = format!("{prefix}{mid}{suffix}");
                    if new != name {
                        parameter(s, &json!({"name": name, "rename": new})).map_err(at)?;
                        done.push(json!({"op": "rename", "name": name, "as": new}));
                    }
                }
            }
            other => return Err(at(format!("`{other}` isn't `set`, `delete`, `duplicate` or `rename`"))),
        }
    }
    // The whole result has to hold before anything is said about it.
    let mut m = (*s.model).clone();
    m.sync_library();
    m.check().map_err(|e| bad(c, e))?;
    if dry_run {
        s.model = before;
    }
    Ok(json!({"dry_run": dry_run, "done": done}))
}

/// What names each parameter: its values, schedules, view filters, tags and calculated fields.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Usage {
    pub values: usize,
    pub schedules: usize,
    pub filters: usize,
    pub tags: usize,
    pub formulas: usize,
}

impl Usage {
    pub fn used(&self) -> bool {
        self.values + self.schedules + self.filters + self.tags + self.formulas > 0
    }
}

/// [`Usage`] of parameter `name`.
pub fn usage(m: &Model, name: &str) -> Usage {
    let mut u = Usage { filters: m.view_filters.values().filter(|f| f.rules.iter().any(|r| r.field == name)).count(), ..Usage::default() };
    for e in m.elements.values() {
        match e {
            Element::Schedule(sc) => {
                let plain = sc.fields.iter().any(|f| f.formula.is_none() && f.name == name)
                    || sc.filters.iter().any(|f| f.field == name)
                    || sc.sort.iter().any(|x| x.field == name);
                u.schedules += usize::from(plain);
                u.formulas += sc.fields.iter().filter(|f| f.formula.as_deref().is_some_and(|t| formula_names(t, name))).count();
            }
            Element::Tag(t) if t.label.as_deref() == Some(name) => u.tags += 1,
            _ => u.values += usize::from(holds_value(m, e, name)),
        }
    }
    u.values += m.parameters.get(name).map_or(0, |p| p.type_values.len());
    u
}

/// The `parameter_list` command: the table the Project Parameters window shows, for agents too.
pub(crate) fn list(s: &mut Session, p: &Value) -> R {
    only("parameter_list", p, &[])?;
    let mut m = (*s.model).clone();
    m.sync_library();
    let rows: Vec<Value> = m
        .parameters
        .iter()
        .map(|(name, q)| {
            json!({
                "name": name,
                "id": m.library.id(&LibKind::Parameter, name),
                "guid": m.library.guid(&LibKind::Parameter, name),
                "shared": q.dictionary.is_some(),
                "parameter": q,
                "usage": usage(&m, name),
            })
        })
        .collect();
    Ok(json!({"parameters": rows, "dictionaries": m.dictionaries, "map": m.parameter_map}))
}

/// The `parameter_purge` command: takes out the parameters nothing uses.
pub(crate) fn purge(s: &mut Session, p: &Value) -> R {
    let c = "parameter_purge";
    only(c, p, &["dry_run"])?;
    let unused: Vec<String> = s.model.parameters.keys().filter(|n| !usage(&s.model, n).used()).cloned().collect();
    if !flag(c, p, "dry_run")? {
        for n in &unused {
            parameter(s, &json!({"name": n, "delete": true}))?;
        }
    }
    Ok(json!({"purged": unused}))
}

/// The `parameter_merge` command: `from`'s values, and what names it, moved to `into`, and `from` gone; a file holding
/// `from`'s GUID is taken for `into` from then on.
pub(crate) fn merge(s: &mut Session, p: &Value) -> R {
    let c = "parameter_merge";
    only(c, p, &["from", "into", "overwrite"])?;
    let from = string(c, p, "from")?.ok_or_else(|| bad(c, "`from` is required"))?;
    let into = string(c, p, "into")?.ok_or_else(|| bad(c, "`into` is required"))?;
    if from == into {
        return Err(bad(c, "`from` and `into` are the same parameter"));
    }
    let overwrite = flag(c, p, "overwrite")?;
    let mut model = (*s.model).clone();
    model.sync_library();
    let a = model.parameters.get(&from).cloned().ok_or_else(|| bad(c, format!("there's no parameter `{from}`")))?;
    let b = model.parameters.get(&into).cloned().ok_or_else(|| bad(c, format!("there's no parameter `{into}`")))?;
    if a.kind != b.kind {
        return Err(bad(c, format!("`{from}` holds {} and `{into}` holds {}", kind_word(a.kind), kind_word(b.kind))));
    }
    if a.scope != b.scope {
        return Err(bad(
            c,
            format!("one of `{from}` and `{into}` is a type parameter and the other an instance one; make them the same first"),
        ));
    }
    if let Some((g, _)) = a.type_values.iter().find(|(g, v)| !overwrite && b.type_values.get(*g).is_some_and(|w| w != *v)) {
        let ty = model.library.by_guid(*g).map_or_else(|| g.to_string(), |(_, e)| e.name.clone());
        return Err(bad(c, format!("type `{ty}` holds both, with different values; `overwrite` takes `{from}`'s, or change it first")));
    }
    let guid = model.library.guid(&LibKind::Parameter, &from);
    // Values that differ stop it, unless told.
    let mut differ = 0;
    for e in model.elements.values() {
        if holds_value(&model, e, &from)
            && holds_value(&model, e, &into)
            && let Some(ps) = e.params()
            && ps.get(&from) != ps.get(&into)
        {
            differ += 1;
        }
    }
    if differ > 0 && !overwrite {
        return Err(bad(
            c,
            format!("{differ} elements hold both, with different values; `overwrite` takes `{from}`'s, or change them first"),
        ));
    }
    let families = model.families.clone();
    let m = s.model_mut();
    let mut moved = 0;
    for e in m.elements.values_mut() {
        let theirs = e.family().and_then(|(f, _)| families.get(f)).is_some_and(|f| f.parameters.iter().any(|q| q.name == from));
        match e {
            Element::Schedule(sc) => {
                for f in &mut sc.fields {
                    match &mut f.formula {
                        Some(text) => *text = rename_in_formula(text, &from, &into),
                        None if f.name == from => f.name = into.clone(),
                        None => {}
                    }
                }
                let mut seen = BTreeSet::new();
                sc.fields.retain(|f| f.formula.is_some() || seen.insert(f.name.clone()));
                sc.filters.iter_mut().filter(|f| f.field == from).for_each(|f| f.field = into.clone());
                sc.sort.iter_mut().filter(|x| x.field == from).for_each(|x| x.field = into.clone());
            }
            Element::Tag(t) if t.label.as_deref() == Some(from.as_str()) => t.label = Some(into.clone()),
            _ => {}
        }
        if !theirs
            && let Some(ps) = e.params_mut()
            && let Some(v) = ps.remove(&from)
        {
            if overwrite || !ps.contains_key(&into) {
                ps.insert(into.clone(), v);
            }
            moved += 1;
        }
    }
    for f in m.view_filters.values_mut() {
        f.rules.iter_mut().filter(|r| r.field == from).for_each(|r| r.field = into.clone());
    }
    m.parameters.remove(&from);
    if let Some(q) = m.parameters.get_mut(&into) {
        for (g, v) in a.type_values {
            if overwrite {
                q.type_values.insert(g, v);
            } else {
                q.type_values.entry(g).or_insert(v);
            }
        }
        q.categories.extend(a.categories);
        q.categories.sort();
        q.categories.dedup();
        q.same_as.extend(a.same_as);
        q.same_as.extend(guid);
        q.same_as.sort_by_key(ToString::to_string);
        q.same_as.dedup();
    }
    Ok(json!({"into": into, "values": moved, "overwritten": if overwrite { differ } else { 0 }}))
}

/// The `family_parameter_transfer` command: parameters copied from one family to others, with what their formulas read.
pub(crate) fn family_transfer(s: &mut Session, p: &Value) -> R {
    let c = "family_parameter_transfer";
    only(c, p, &["from", "to", "names", "prefix", "suffix", "find", "replace"])?;
    let from = string(c, p, "from")?.ok_or_else(|| bad(c, "`from` is required"))?;
    let to: Vec<String> = p
        .get("to")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .filter(|l: &Vec<String>| !l.is_empty())
        .ok_or_else(|| bad(c, "`to` is a list of families"))?;
    let names: Vec<String> = p
        .get("names")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .filter(|l: &Vec<String>| !l.is_empty())
        .ok_or_else(|| bad(c, "`names` is a list of its parameters"))?;
    let source = s.model.families.get(&from).cloned().ok_or_else(|| bad(c, format!("there's no family `{from}`")))?;
    if let Some(n) = names.iter().find(|n| !source.parameters.iter().any(|q| q.name == **n)) {
        return Err(bad(c, format!("`{from}` has no parameter `{n}`")));
    }
    if let Some(f) = to.iter().find(|f| !s.model.families.contains_key(*f)) {
        return Err(bad(c, format!("there's no family `{f}`")));
    }
    // What's asked, and what its formulas read.
    let mut take: Vec<String> = names.clone();
    let mut i = 0;
    while let Some(n) = take.get(i).cloned() {
        i += 1;
        if let Some(text) = source.parameters.iter().find(|q| q.name == n).and_then(|q| q.formula.as_ref()) {
            for q in &source.parameters {
                if !take.contains(&q.name) && formula_names(text, &q.name) {
                    take.push(q.name.clone());
                }
            }
        }
    }
    let (prefix, suffix) = (string(c, p, "prefix")?.unwrap_or_default(), string(c, p, "suffix")?.unwrap_or_default());
    let (find, replace) = (string(c, p, "find")?.unwrap_or_default(), string(c, p, "replace")?.unwrap_or_default());
    let new_name = |n: &str| {
        let mid = if find.is_empty() { n.to_string() } else { n.replace(&find, &replace) };
        format!("{prefix}{mid}{suffix}")
    };
    let mut report = Vec::new();
    let m = s.model_mut();
    for family in &to {
        if *family == from && prefix.is_empty() && suffix.is_empty() && find.is_empty() {
            continue;
        }
        let Some(target) = m.families.get_mut(family) else { continue };
        let (mut added, mut skipped) = (Vec::new(), Vec::new());
        for n in &take {
            let Some(q) = source.parameters.iter().find(|q| q.name == *n) else { continue };
            let name = new_name(n);
            if target.parameters.iter().any(|t| t.name == name) {
                skipped.push(name);
                continue;
            }
            let mut copy = q.clone();
            copy.name = name.clone();
            if let Some(text) = &mut copy.formula {
                for other in &take {
                    *text = rename_in_formula(text, other, &new_name(other));
                }
            }
            target.parameters.push(copy);
            added.push(name);
        }
        report.push(json!({"family": family, "added": added, "skipped": skipped}));
    }
    Ok(json!({"families": report}))
}

/// Today, `YYYY-MM-DD`, in UTC.
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    // Days to a civil date, after Howard Hinnant's `civil_from_days`.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The type a command names: `{category, type}`, or `{family, type}`.
fn named_type(c: &str, m: &Model, p: &Value, name: &str) -> Result<(LibKind, String, Guid), String> {
    let (kind, entry) = match string(c, p, "family")? {
        Some(f) => {
            let fam = m.families.get(&f).ok_or_else(|| bad(c, format!("there's no family `{f}`")))?;
            if !fam.types.contains_key(name) {
                return Err(bad(c, format!("family `{f}` has no type `{name}`")));
            }
            (LibKind::FamilyType, format!("{f}: {name}"))
        }
        None => {
            let cat = string(c, p, "category")?.ok_or_else(|| bad(c, "`category` or `family` is required"))?;
            let cat = category(&cat);
            if !m.type_names(&cat).contains(&name) {
                return Err(bad(c, format!("{cat} has no type `{name}`")));
            }
            (LibKind::Type(cat), name.to_string())
        }
    };
    let guid = m.library.guid(&kind, &entry).ok_or_else(|| bad(c, format!("`{entry}` has no GUID yet")))?;
    Ok((kind, entry, guid))
}

/// The `type_parameter` command: a type's values of the project's type parameters.
pub(crate) fn type_parameter(s: &mut Session, p: &Value) -> R {
    let c = "type_parameter";
    only(c, p, &["category", "family", "type", "values", "copy_to"])?;
    let name = string(c, p, "type")?.ok_or_else(|| bad(c, "`type` is required"))?;
    let values = p.get("values").filter(|v| !v.is_null());
    let copy_to = string(c, p, "copy_to")?;
    let mut synced = (*s.model).clone();
    synced.sync_library();
    let (kind, entry, guid) = named_type(c, &synced, p, &name)?;
    let cat = category_of_type(&synced, &kind, &entry).unwrap_or_default();
    let held = |m: &Model, g: Guid| -> BTreeMap<String, Value> {
        m.parameters.iter().filter_map(|(n, q)| Some((n.clone(), q.type_values.get(&g)?.clone()))).collect()
    };
    if values.is_none() && copy_to.is_none() {
        return Ok(json!({"type": entry, "values": held(&synced, guid)}));
    }
    let mut set: Vec<(String, Guid, Value)> = Vec::new();
    if let Some(v) = values {
        let map = v.as_object().ok_or_else(|| bad(c, "`values` gives each parameter a value, or null"))?;
        for (n, value) in map {
            let q = synced.parameters.get(n).ok_or_else(|| bad(c, format!("there's no parameter `{n}`")))?;
            if q.scope != Scope::Type {
                return Err(bad(c, format!("`{n}` is an instance parameter: its value is each element's, set with `modify`")));
            }
            if !q.categories.contains(&cat) {
                return Err(bad(c, format!("`{n}` isn't for {cat}s")));
            }
            if !value.is_null() && !q.holds(value) {
                return Err(bad(c, format!("`{n}` holds {}, not {value}", kind_word(q.kind))));
            }
            set.push((n.clone(), guid, value.clone()));
        }
    }
    if let Some(to) = &copy_to {
        let (_, _, there) = named_type(c, &synced, p, to)?;
        for (n, v) in held(&synced, guid) {
            set.push((n, there, v));
        }
    }
    *s.model_mut() = synced;
    let m = s.model_mut();
    for (n, g, v) in set {
        if let Some(q) = m.parameters.get_mut(&n) {
            if v.is_null() {
                q.type_values.remove(&g);
            } else {
                q.type_values.insert(g, v);
            }
        }
    }
    Ok(json!({"type": entry, "values": held(m, guid)}))
}

/// A parameter a family is to have, as `family_parameter_add` takes it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewFamilyParameter {
    name: String,
    #[serde(default)]
    kind: family::Kind,
    #[serde(default)]
    instance: bool,
    #[serde(default)]
    value: Value,
    #[serde(default)]
    formula: Option<String>,
    #[serde(default)]
    group: String,
    #[serde(default)]
    definition: Option<Guid>,
}

/// The `family_parameter_add` command: new parameters for several families at once. All or none: [`Model::check`] goes over each
/// family after, and the engine takes the lot back when one can't hold them.
pub(crate) fn family_add(s: &mut Session, p: &Value) -> R {
    let c = "family_parameter_add";
    only(c, p, &["families", "parameters"])?;
    let families: Vec<String> = p
        .get("families")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .filter(|l: &Vec<String>| !l.is_empty())
        .ok_or_else(|| bad(c, "`families` is a list of families"))?;
    let new: Vec<NewFamilyParameter> = p
        .get("parameters")
        .cloned()
        .ok_or_else(|| bad(c, "`parameters` is a list of parameters"))
        .and_then(|v| serde_json::from_value(v).map_err(|e| bad(c, format!("`parameters`: {e}"))))?;
    if new.len() > MAX_OPS {
        return Err(bad(c, format!("up to {MAX_OPS} parameters at once")));
    }
    if let Some(f) = families.iter().find(|f| !s.model.families.contains_key(*f)) {
        return Err(bad(c, format!("there's no family `{f}`")));
    }
    for q in &new {
        if !family::nameable(&q.name) {
            return Err(bad(c, format!("`{}`: a name starts with a letter and holds letters, digits, `_` and spaces", q.name)));
        }
    }
    let m = s.model_mut();
    let mut report = Vec::new();
    for name in &families {
        let Some(f) = m.families.get_mut(name) else { continue };
        let (mut added, mut skipped) = (Vec::new(), Vec::new());
        for q in &new {
            if f.parameters.iter().any(|t| t.name == q.name) {
                skipped.push(q.name.clone());
                continue;
            }
            f.parameters.push(family::Parameter {
                name: q.name.clone(),
                kind: q.kind,
                instance: q.instance,
                value: q.value.clone(),
                formula: q.formula.clone().filter(|t| !t.trim().is_empty()),
                group: clip(&q.group),
                definition: q.definition,
            });
            added.push(q.name.clone());
        }
        report.push(json!({"family": name, "added": added, "skipped": skipped}));
    }
    Ok(json!({"families": report}))
}

/// The most dictionaries a project keeps loaded.
const MAX_DICTIONARIES: usize = 256;

/// The `dictionary_mount` command: a dictionary's file kept with the project, or taken off it.
pub(crate) fn dictionary_mount(s: &mut Session, p: &Value) -> R {
    let c = "dictionary_mount";
    only(c, p, &["path", "name", "remove"])?;
    if flag(c, p, "remove")? {
        let name = string(c, p, "name")?.ok_or_else(|| bad(c, "`name` is required"))?;
        if !s.model.dictionaries.contains_key(&name) {
            return Err(bad(c, format!("no dictionary `{name}` is loaded")));
        }
        let using: Vec<&str> = s
            .model
            .parameters
            .iter()
            .filter(|(_, q)| q.dictionary.as_deref() == Some(&name))
            .map(|(n, _)| n.as_str())
            .take(NAMED)
            .collect();
        if !using.is_empty() {
            return Err(bad(c, format!("these come from `{name}`; make them project parameters first: {}", using.join(", "))));
        }
        s.model_mut().dictionaries.remove(&name);
        return Ok(json!({"removed": name}));
    }
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is required"))?;
    let d = table::read_dictionary_file(&path).map_err(|e| bad(c, e))?;
    // The same dictionary by its GUID, whatever the file is called now and wherever it is.
    let known = d
        .guid
        .and_then(|g| s.model.library.by_guid(g))
        .filter(|(_, e)| e.kind == LibKind::Dictionary && s.model.dictionaries.contains_key(&e.name))
        .map(|(_, e)| e.name.clone());
    if known.is_none()
        && let Some(r) = s.model.dictionaries.get(&d.name).filter(|r| r.path != path)
    {
        return Err(bad(c, format!("a dictionary `{}` is loaded already, from {}: name this one otherwise", d.name, r.path)));
    }
    let name = known.unwrap_or_else(|| d.name.clone());
    if !s.model.dictionaries.contains_key(&name) && s.model.dictionaries.len() >= MAX_DICTIONARIES {
        return Err(bad(c, format!("a project keeps up to {MAX_DICTIONARIES} dictionaries")));
    }
    let m = s.model_mut();
    let was = m.dictionaries.get(&name).map_or(0, |r| r.revision);
    m.dictionaries.insert(name.clone(), DictionaryRef { path: path.clone(), version: d.version.clone(), revision: was });
    if let Some(g) = d.guid
        && m.library.id(&LibKind::Dictionary, &name).is_none()
        && m.library.by_guid(g).is_none()
        && !m.elements.contains_key(g)
    {
        let id = m.fresh_library_id();
        m.library.insert(id, Entry { kind: LibKind::Dictionary, name: name.clone(), guid: g })?;
    }
    Ok(json!({"name": name, "path": path, "revision": d.revision}))
}
