//! The project's library, as elements are, has ids and GUIDs: each type, material, picture, family, project parameter, view
//! filter, view template, phase, phase filter, design option set and design option. Its id comes from the same count as the
//! elements' (an id names one thing in the whole project); its GUID is made once and never changes, and survives renames.
//!
//! What uses one names it in memory, as the code always has (`type_name`, a layer's `material`, a view's `template`…), and
//! [`Library`] knows which name is which GUID. In a file, every such name is the GUID instead ([`Model::to_json`]), and an
//! element's values of project parameters are keyed by theirs: renaming changes nothing a file holds but the name. A name
//! that is a GUID is refused, so the two never mix.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

use crate::ids::{Guid, Id};

/// Where the library's ids start, counting down.
pub const LAST_ID: Id = 999_999;
use crate::model::Model;
use serde_json::Value;

/// What a library entry is.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// A type, of a category: `wall`, `door`, `beam`…
    Type(String),
    Material,
    Picture,
    Family,
    Parameter,
    ViewFilter,
    ViewTemplate,
    Phase,
    PhaseFilter,
    OptionSet,
    /// A design option, named `set: option`.
    DesignOption,
    /// A family's type, named `family: type`.
    FamilyType,
    /// A family's parameter, named `family: parameter`.
    FamilyParameter,
    LinePattern,
    LineStyle,
    FillPattern,
    FilledRegionType,
}

impl Kind {
    /// What it is called, as messages and listings say: `wall type`, `material`, `design option`.
    pub fn label(&self) -> String {
        match self {
            Kind::Type(c) => format!("{c} type"),
            Kind::Material => "material".into(),
            Kind::Picture => "picture".into(),
            Kind::Family => "family".into(),
            Kind::Parameter => "parameter".into(),
            Kind::ViewFilter => "view filter".into(),
            Kind::ViewTemplate => "view template".into(),
            Kind::Phase => "phase".into(),
            Kind::PhaseFilter => "phase filter".into(),
            Kind::OptionSet => "design option set".into(),
            Kind::DesignOption => "design option".into(),
            Kind::FamilyType => "family type".into(),
            Kind::FamilyParameter => "family parameter".into(),
            Kind::LinePattern => "line pattern".into(),
            Kind::LineStyle => "line style".into(),
            Kind::FillPattern => "fill pattern".into(),
            Kind::FilledRegionType => "filled region type".into(),
        }
    }

    fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "material" => Kind::Material,
            "picture" => Kind::Picture,
            "family" => Kind::Family,
            "parameter" => Kind::Parameter,
            "view-filter" => Kind::ViewFilter,
            "view-template" => Kind::ViewTemplate,
            "phase" => Kind::Phase,
            "phase-filter" => Kind::PhaseFilter,
            "option-set" => Kind::OptionSet,
            "design-option" => Kind::DesignOption,
            "family-type" => Kind::FamilyType,
            "family-parameter" => Kind::FamilyParameter,
            "line-pattern" => Kind::LinePattern,
            "line-style" => Kind::LineStyle,
            "fill-pattern" => Kind::FillPattern,
            "filled-region-type" => Kind::FilledRegionType,
            _ => Kind::Type(s.strip_prefix("type:")?.to_string()),
        })
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::Type(c) => write!(f, "type:{c}"),
            Kind::Material => f.write_str("material"),
            Kind::Picture => f.write_str("picture"),
            Kind::Family => f.write_str("family"),
            Kind::Parameter => f.write_str("parameter"),
            Kind::ViewFilter => f.write_str("view-filter"),
            Kind::ViewTemplate => f.write_str("view-template"),
            Kind::Phase => f.write_str("phase"),
            Kind::PhaseFilter => f.write_str("phase-filter"),
            Kind::OptionSet => f.write_str("option-set"),
            Kind::DesignOption => f.write_str("design-option"),
            Kind::FamilyType => f.write_str("family-type"),
            Kind::FamilyParameter => f.write_str("family-parameter"),
            Kind::LinePattern => f.write_str("line-pattern"),
            Kind::LineStyle => f.write_str("line-style"),
            Kind::FillPattern => f.write_str("fill-pattern"),
            Kind::FilledRegionType => f.write_str("filled-region-type"),
        }
    }
}

impl Serialize for Kind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Kind, D::Error> {
        let s = String::deserialize(d)?;
        Kind::parse(&s).ok_or_else(|| de::Error::custom(format!("`{s}` isn't a kind of library entry")))
    }
}

/// A library entry: what it is, its name, its GUID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub kind: Kind,
    pub name: String,
    pub guid: Guid,
}

/// A part of a library entry, with an id and a GUID of its own that it holds itself, kept while it stays where it is
/// in its list: a type's layer, a family's form or plan symbol. `what` it is, what it's `of` and its place there.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub what: &'static str,
    pub of: String,
    /// The name of the type or family it's of.
    pub owner: String,
    pub index: usize,
    pub guid: Guid,
}

impl Part {
    /// As messages and listings name it: `layer 2 of wall type Exterior - 300mm`.
    pub fn name(&self) -> String {
        format!("{} {} of {}", self.what, self.index + 1, self.of)
    }
}

/// Something that holds its own id and GUID: a [`Part`].
pub trait HasPart {
    fn ident(&mut self) -> (&mut Id, &mut Guid);
}

impl HasPart for crate::model::Layer {
    fn ident(&mut self) -> (&mut Id, &mut Guid) {
        (&mut self.id, &mut self.guid)
    }
}

impl HasPart for crate::family::Form {
    fn ident(&mut self) -> (&mut Id, &mut Guid) {
        (&mut self.id, &mut self.guid)
    }
}

impl HasPart for crate::sketch::SketchLine {
    fn ident(&mut self) -> (&mut Id, &mut Guid) {
        (&mut self.id, &mut self.guid)
    }
}

impl HasPart for crate::family::Symbol {
    fn ident(&mut self) -> (&mut Id, &mut Guid) {
        (&mut self.id, &mut self.guid)
    }
}

/// `new`, the list that replaces `old` (a type's layers, a family's forms), each without an id of its own taking the id
/// and the GUID of the one at its place in `old`: what stays where it was keeps them.
pub fn keep_parts<T: HasPart>(old: &mut [T], new: &mut [T]) {
    for (o, n) in old.iter_mut().zip(new.iter_mut()) {
        let ((oid, ogid), (nid, ngid)) = (o.ident(), n.ident());
        if crate::ids::unset(nid) && ngid.is_nil() {
            (*nid, *ngid) = (*oid, *ogid);
        }
    }
}

/// The library's ids and GUIDs, by id; in a file, an object of ids. Its parts' too, which they hold themselves.
#[derive(Clone, Debug, Default)]
pub struct Library {
    by_id: BTreeMap<Id, Entry>,
    by_name: BTreeMap<(Kind, String), Id>,
    by_guid: HashMap<Guid, Id>,
    parts: BTreeMap<Id, Part>,
}

impl PartialEq for Library {
    fn eq(&self, other: &Library) -> bool {
        self.by_id == other.by_id
    }
}

impl Library {
    pub fn entries(&self) -> impl Iterator<Item = (Id, &Entry)> {
        self.by_id.iter().map(|(id, e)| (*id, e))
    }

    pub fn get(&self, id: Id) -> Option<&Entry> {
        self.by_id.get(&id)
    }

    /// Whether an entry or a part has id `id`.
    pub fn taken(&self, id: Id) -> bool {
        self.by_id.contains_key(&id) || self.parts.contains_key(&id)
    }

    pub fn part(&self, id: Id) -> Option<&Part> {
        self.parts.get(&id)
    }

    pub fn parts(&self) -> impl Iterator<Item = (Id, &Part)> {
        self.parts.iter().map(|(id, p)| (*id, p))
    }

    /// The part of GUID `g`, and its id.
    pub fn part_by_guid(&self, g: Guid) -> Option<(Id, &Part)> {
        self.parts.iter().find(|(_, p)| p.guid == g).map(|(id, p)| (*id, p))
    }

    /// The id of `kind` `name`.
    pub fn id(&self, kind: &Kind, name: &str) -> Option<Id> {
        self.by_name.get(&(kind.clone(), name.to_string())).copied()
    }

    /// The GUID of `kind` `name`.
    pub fn guid(&self, kind: &Kind, name: &str) -> Option<Guid> {
        self.id(kind, name).and_then(|id| self.by_id.get(&id)).map(|e| e.guid)
    }

    /// The entry of GUID `g`, and its id.
    pub fn by_guid(&self, g: Guid) -> Option<(Id, &Entry)> {
        let id = *self.by_guid.get(&g)?;
        Some((id, self.by_id.get(&id)?))
    }

    pub fn first_id(&self) -> Option<Id> {
        self.by_id.keys().next().copied()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Puts an entry at `id`; an error if its id, its name or its GUID is another's.
    pub fn insert(&mut self, id: Id, e: Entry) -> Result<(), String> {
        if self.by_id.contains_key(&id) {
            return Err(format!("two library entries have the id {id}"));
        }
        if let Some(other) = self.by_guid.get(&e.guid) {
            return Err(format!("library entries {other} and {id} have the same GUID {}", e.guid));
        }
        if self.by_name.contains_key(&(e.kind.clone(), e.name.clone())) {
            return Err(format!("two {}s are named `{}` in the library", e.kind.label(), e.name));
        }
        self.by_name.insert((e.kind.clone(), e.name.clone()), id);
        self.by_guid.insert(e.guid, id);
        self.by_id.insert(id, e);
        Ok(())
    }

    fn remove(&mut self, id: Id) {
        if let Some(e) = self.by_id.remove(&id) {
            self.by_name.remove(&(e.kind, e.name));
            self.by_guid.remove(&e.guid);
        }
    }

    /// `kind` `old` is called `new` now: the same entry, its id and GUID kept.
    pub fn renamed(&mut self, kind: &Kind, old: &str, new: &str) {
        if old == new {
            return;
        }
        let Some(id) = self.id(kind, old) else { return };
        if let Some(e) = self.by_id.get_mut(&id) {
            self.by_name.remove(&(kind.clone(), old.to_string()));
            e.name = new.to_string();
            self.by_name.insert((kind.clone(), new.to_string()), id);
        }
    }

    /// The design options of set `old` are of set `new` now.
    pub fn set_renamed(&mut self, old: &str, new: &str) {
        self.owner_renamed(&Kind::DesignOption, old, new);
        self.renamed(&Kind::OptionSet, old, new);
    }

    /// The types and parameters of family `old` are of family `new` now.
    pub fn family_renamed(&mut self, old: &str, new: &str) {
        self.owner_renamed(&Kind::FamilyType, old, new);
        self.owner_renamed(&Kind::FamilyParameter, old, new);
        self.renamed(&Kind::Family, old, new);
    }

    /// The entries of `kind` named `old: …` are named `new: …` now.
    fn owner_renamed(&mut self, kind: &Kind, old: &str, new: &str) {
        let prefix = format!("{old}: ");
        let names: Vec<String> = self.by_name.keys().filter(|(k, n)| k == kind && n.starts_with(&prefix)).map(|(_, n)| n.clone()).collect();
        for n in names {
            let rest = n.get(prefix.len()..).unwrap_or_default().to_string();
            self.renamed(kind, &n, &format!("{new}: {rest}"));
        }
    }
}

impl Serialize for Library {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.by_id.iter().map(|(id, e)| (id.to_string(), e)))
    }
}

impl<'de> Deserialize<'de> for Library {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Library, D::Error> {
        let raw = BTreeMap::<String, Entry>::deserialize(d)?;
        let mut out = Library::default();
        for (k, e) in raw {
            let id: Id = k.parse().map_err(|_| de::Error::custom(format!("`{k}` isn't a library id")))?;
            if e.guid.pending().is_some() {
                return Err(de::Error::custom(format!("library entry {id}: its GUID is a number")));
            }
            out.insert(id, e).map_err(de::Error::custom)?;
        }
        Ok(out)
    }
}

/// What Purge Unused may take out, as a command names it: `type` (every kind of type, and a family's types), `family`, `material`,
/// `picture`, `filter` and `template`.
pub const PURGE_KINDS: [&str; 6] = ["type", "family", "material", "picture", "filter", "template"];

impl Model {
    /// How many times something names each library entry: an element its type or family, a type's layer its material, a material
    /// its picture, a view its template and filters. An entry nothing names is unused.
    /// ponytail: counted on a copy of the model (the visitor takes `&mut`); a read-only visitor if purging a huge project is slow.
    pub fn library_uses(&self) -> BTreeMap<(Kind, String), usize> {
        let mut uses = BTreeMap::new();
        self.clone().lib_refs_mut(&mut |kind, name| *uses.entry((kind.clone(), name.clone())).or_insert(0) += 1);
        uses
    }

    /// Takes entry `name` of `kind` out of the project: false where there is none, or where it isn't one a project can lose
    /// (a phase, a parameter, an option: they have commands of their own).
    pub fn remove_entry(&mut self, kind: &Kind, name: &str) -> bool {
        use crate::model::RUN_CATEGORIES;
        match kind {
            Kind::Type(category) => match category.as_str() {
                "wall" => self.types.wall.remove(name).is_some(),
                "floor" => self.types.floor.remove(name).is_some(),
                "door" => self.types.door.remove(name).is_some(),
                "window" => self.types.window.remove(name).is_some(),
                "column" => self.types.column.remove(name).is_some(),
                "roof" => self.types.roof.remove(name).is_some(),
                "ceiling" => self.types.ceiling.remove(name).is_some(),
                "railing" => self.types.railing.remove(name).is_some(),
                "structuralcolumn" => self.types.structural_column.remove(name).is_some(),
                "footing" => self.types.footing.remove(name).is_some(),
                run if RUN_CATEGORIES.contains(&run) => self.types.runs.get_mut(run).is_some_and(|t| t.remove(name).is_some()),
                _ => false,
            },
            Kind::Family => self.families.remove(name).is_some(),
            Kind::FamilyType => {
                name.split_once(": ").is_some_and(|(f, t)| self.families.get_mut(f).is_some_and(|fam| fam.types.remove(t).is_some()))
            }
            Kind::Material => self.materials.remove(name).is_some(),
            Kind::Picture => self.images.remove(name).is_some(),
            Kind::ViewFilter => self.view_filters.remove(name).is_some(),
            Kind::ViewTemplate => self.view_templates.remove(name).is_some(),
            _ => false,
        }
    }

    /// The entries of `kinds` (see [`PURGE_KINDS`]) nothing names, as one pass of Purge Unused would take them out. Each
    /// category keeps a type (its default's, else its first) and each family a type, so that a tool still has one to place; a
    /// family that goes takes its types with it. What a pass takes out may leave more unused, which the next pass finds.
    pub fn unused_entries(&self, kinds: &[&str]) -> Vec<(Kind, String)> {
        let uses = self.library_uses();
        let has = |kind: &str| kinds.contains(&kind);
        let unused = |k: &Kind, n: &str| !uses.contains_key(&(k.clone(), n.to_string()));
        let mut out: Vec<(Kind, String)> = Vec::new();
        let all = self.library_names();
        // Types by category: all of a category's unused ones but one, when none of its types is used.
        let mut categories: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (kind, name) in &all {
            if let Kind::Type(c) = kind {
                categories.entry(c.as_str()).or_default().push(name);
            }
        }
        if has("type") {
            for (category, names) in categories {
                let kind = Kind::Type(category.to_string());
                let mut gone: Vec<&str> = names.iter().copied().filter(|n| unused(&kind, n)).collect();
                if gone.len() == names.len() {
                    let keep = self.default_type(category).filter(|d| gone.contains(d)).or(gone.first().copied());
                    gone.retain(|n| Some(*n) != keep);
                }
                out.extend(gone.into_iter().map(|n| (kind.clone(), n.to_string())));
            }
        }
        let families_gone: Vec<&str> = if has("family") {
            self.families.keys().map(String::as_str).filter(|f| unused(&Kind::Family, f)).collect()
        } else {
            Vec::new()
        };
        out.extend(families_gone.iter().map(|f| (Kind::Family, f.to_string())));
        if has("type") {
            for (family, fam) in self.families.iter().filter(|(f, _)| !families_gone.contains(&f.as_str())) {
                let full = |t: &str| format!("{family}: {t}");
                let mut gone: Vec<&str> = fam.types.keys().map(String::as_str).filter(|t| unused(&Kind::FamilyType, &full(t))).collect();
                if gone.len() == fam.types.len() {
                    gone.pop();
                }
                out.extend(gone.into_iter().map(|t| (Kind::FamilyType, full(t))));
            }
        }
        for (kind, name, wanted) in all.iter().filter_map(|(k, n)| {
            Some((
                k,
                n,
                match k {
                    Kind::Material => "material",
                    Kind::Picture => "picture",
                    Kind::ViewFilter => "filter",
                    Kind::ViewTemplate => "template",
                    _ => return None,
                },
            ))
        }) {
            if has(wanted) && unused(kind, name) {
                out.push((kind.clone(), name.clone()));
            }
        }
        out
    }
}

impl Model {
    /// Every entry the library has now, by what it is and its name.
    pub fn library_names(&self) -> Vec<(Kind, String)> {
        let mut out = Vec::new();
        let mut add = |kind: Kind, names: &mut dyn Iterator<Item = &String>| out.extend(names.map(|n| (kind.clone(), n.clone())));
        let t = &self.types;
        add(Kind::Type("wall".into()), &mut t.wall.keys());
        add(Kind::Type("floor".into()), &mut t.floor.keys());
        add(Kind::Type("door".into()), &mut t.door.keys());
        add(Kind::Type("window".into()), &mut t.window.keys());
        add(Kind::Type("column".into()), &mut t.column.keys());
        add(Kind::Type("roof".into()), &mut t.roof.keys());
        add(Kind::Type("ceiling".into()), &mut t.ceiling.keys());
        add(Kind::Type("railing".into()), &mut t.railing.keys());
        add(Kind::Type("structuralcolumn".into()), &mut t.structural_column.keys());
        add(Kind::Type("footing".into()), &mut t.footing.keys());
        for (cat, types) in &t.runs {
            add(Kind::Type(cat.clone()), &mut types.keys());
        }
        add(Kind::Material, &mut self.materials.keys());
        add(Kind::Picture, &mut self.images.keys());
        add(Kind::Family, &mut self.families.keys());
        add(Kind::Parameter, &mut self.parameters.keys());
        add(Kind::ViewFilter, &mut self.view_filters.keys());
        add(Kind::ViewTemplate, &mut self.view_templates.keys());
        add(Kind::Phase, &mut self.phases.iter());
        add(Kind::PhaseFilter, &mut self.phase_filters.keys());
        add(Kind::OptionSet, &mut self.option_sets.keys());
        let options: Vec<String> =
            self.option_sets.iter().flat_map(|(s, set)| set.options.iter().map(move |o| format!("{s}: {o}"))).collect();
        add(Kind::DesignOption, &mut options.iter());
        let types: Vec<String> = self.families.iter().flat_map(|(f, fam)| fam.types.keys().map(move |t| format!("{f}: {t}"))).collect();
        add(Kind::FamilyType, &mut types.iter());
        let params: Vec<String> =
            self.families.iter().flat_map(|(f, fam)| fam.parameters.iter().map(move |p| format!("{f}: {}", p.name))).collect();
        add(Kind::FamilyParameter, &mut params.iter());
        add(Kind::LinePattern, &mut self.line_patterns.keys());
        add(Kind::LineStyle, &mut self.line_styles.keys());
        add(Kind::FillPattern, &mut self.fill_patterns.keys());
        add(Kind::FilledRegionType, &mut self.region_types.keys());
        out
    }

    /// Whether the library's ids and GUIDs are those of what the library has: none missing, none left over; and each part
    /// has an id and a GUID of its own, which no other thing has.
    pub fn library_in_step(&self) -> bool {
        let now = self.library_names();
        now.len() == self.library.len() && now.iter().all(|(k, n)| self.library.id(k, n).is_some()) && self.parts_in_step()
    }

    /// Each part of the library, with the id and GUID it holds, in order.
    pub fn parts_now(&self) -> Vec<(Id, Part)> {
        let mut out = Vec::new();
        let mut add = |what: &'static str, of: &str, owner: &str, list: Vec<(Id, Guid)>| {
            out.extend(
                list.into_iter()
                    .enumerate()
                    .map(|(index, (id, guid))| (id, Part { what, of: format!("{of} {owner}"), owner: owner.to_string(), index, guid })),
            );
        };
        let t = &self.types;
        for (n, x) in &t.wall {
            add("layer", "wall type", n, x.layers.iter().map(|l| (l.id, l.guid)).collect());
        }
        for (n, x) in &t.floor {
            add("layer", "floor type", n, x.layers.iter().map(|l| (l.id, l.guid)).collect());
        }
        for (n, x) in &t.roof {
            add("layer", "roof type", n, x.layers.iter().map(|l| (l.id, l.guid)).collect());
        }
        for (n, x) in &t.ceiling {
            add("layer", "ceiling type", n, x.layers.iter().map(|l| (l.id, l.guid)).collect());
        }
        for (n, f) in &self.families {
            add("form", "family", n, f.forms.iter().map(|x| (x.id, x.guid)).collect());
            add("symbol", "family", n, f.symbols.iter().map(|x| (x.id, x.guid)).collect());
        }
        // An element's sketch lines, the element named by its GUID, which never changes.
        for (id, e) in self.elements.iter() {
            if let Some(lines) = e.sketch().filter(|l| !l.is_empty()) {
                add("sketch line", e.category(), &self.guid(*id).to_string(), lines.iter().map(|l| (l.id, l.guid)).collect());
            }
        }
        out
    }

    fn parts_in_step(&self) -> bool {
        let now = self.parts_now();
        let mut ids = std::collections::BTreeSet::new();
        let mut guids = std::collections::HashSet::new();
        now.len() == self.library.parts.len()
            && now.iter().all(|(id, p)| {
                !crate::ids::unset(id)
                    && !p.guid.is_nil()
                    && p.guid.pending().is_none()
                    && ids.insert(*id)
                    && guids.insert(p.guid)
                    && self.library.part(*id) == Some(p)
                    && self.library.get(*id).is_none()
                    && self.library.by_guid(p.guid).is_none()
                    && !self.elements.contains_key(*id)
                    && !self.elements.contains_key(p.guid)
            })
    }

    /// Each part given an id and a GUID where it has none, or has another's (a copy, a part brought in); then listed. Of
    /// two holding one, the one that held it before (at the same place in the same list) keeps it, else the first.
    fn sync_parts(&mut self, make: &mut dyn FnMut(&Model, Option<&Kind>, &str) -> Guid) {
        let now = self.parts_now();
        let mut ids: std::collections::BTreeSet<Id> =
            self.elements.keys().copied().chain(self.library.entries().map(|(id, _)| id)).collect();
        let mut guids: std::collections::HashSet<Guid> =
            self.elements.with_guids().map(|(_, g, _)| g).chain(self.library.entries().map(|(_, e)| e.guid)).collect();
        let place = |p: &Part| (p.what, p.of.clone(), p.index);
        // Who keeps what: first, each that held its id and GUID before at the same place; then the first of the rest.
        let mut keep = vec![(false, false); now.len()];
        for (k, (id, p)) in now.iter().enumerate() {
            let before = self.library.part(*id).filter(|q| place(q) == place(p));
            if before.is_some() && !ids.contains(id) {
                ids.insert(*id);
                keep[k].0 = true;
            }
            if before.is_some_and(|q| q.guid == p.guid) && !guids.contains(&p.guid) {
                guids.insert(p.guid);
                keep[k].1 = true;
            }
        }
        for (k, (id, p)) in now.iter().enumerate() {
            if !keep[k].0 && !crate::ids::unset(id) && !ids.contains(id) {
                ids.insert(*id);
                keep[k].0 = true;
            }
            if !keep[k].1 && !p.guid.is_nil() && p.guid.pending().is_none() && !guids.contains(&p.guid) {
                guids.insert(p.guid);
                keep[k].1 = true;
            }
        }
        // New ones: ids down from the library's lowest, GUIDs made in order (a file read the same makes the same).
        let floor = self.elements.keys().next_back().map_or(crate::ids::FIRST_ID, |k| k + 1).max(self.next_id);
        // Below every entry's and every part's there was, so a part taken away leaves its id unused.
        let mut below = self.library.first_id().into_iter().chain(self.library.parts.keys().next().copied()).min().unwrap_or(LAST_ID + 1);
        let given: Vec<(Id, Guid)> = now
            .iter()
            .zip(&keep)
            .map(|((id, p), (keep_id, keep_guid))| {
                let id = if *keep_id {
                    *id
                } else {
                    let mut next = below.saturating_sub(1);
                    while next >= floor && ids.contains(&next) {
                        next -= 1;
                    }
                    if next < floor {
                        next = ids.iter().next_back().map_or(crate::ids::FIRST_ID, |k| k + 1).max(floor);
                    } else {
                        below = next;
                    }
                    ids.insert(next);
                    next
                };
                let guid = if *keep_guid {
                    p.guid
                } else {
                    let mut g = make(self, None, &p.name());
                    while g.is_nil() || g.pending().is_some() || guids.contains(&g) {
                        g = Guid::new();
                    }
                    guids.insert(g);
                    g
                };
                (id, guid)
            })
            .collect();
        // Put back in the same order `parts_now` walks them.
        let mut given = given.into_iter();
        let mut give = |id: &mut Id, guid: &mut Guid| {
            if let Some((i, g)) = given.next() {
                (*id, *guid) = (i, g);
            }
        };
        let t = &mut self.types;
        for x in t.wall.values_mut() {
            x.layers.iter_mut().for_each(|l| give(&mut l.id, &mut l.guid));
        }
        for x in t.floor.values_mut() {
            x.layers.iter_mut().for_each(|l| give(&mut l.id, &mut l.guid));
        }
        for x in t.roof.values_mut() {
            x.layers.iter_mut().for_each(|l| give(&mut l.id, &mut l.guid));
        }
        for x in t.ceiling.values_mut() {
            x.layers.iter_mut().for_each(|l| give(&mut l.id, &mut l.guid));
        }
        for f in self.families.values_mut() {
            f.forms.iter_mut().for_each(|x| give(&mut x.id, &mut x.guid));
            f.symbols.iter_mut().for_each(|x| give(&mut x.id, &mut x.guid));
        }
        for e in self.elements.values_mut() {
            if let Some(lines) = e.sketch_mut() {
                lines.iter_mut().for_each(|l| give(&mut l.id, &mut l.guid));
            }
        }
        self.library.parts = self.parts_now().into_iter().collect();
    }

    /// What the library has that has no id and GUID gets them; what it has no more loses them. A rename that kept its
    /// entry ([`Library::renamed`]) keeps them. Each part with none, or another's, gets its own.
    pub fn sync_library(&mut self) {
        self.sync_library_with(&mut |m: &Model, _: Option<&Kind>, _: &str| m.elements.fresh());
    }

    /// As [`Model::sync_library`], each new entry's GUID what `guid` makes of what it is and its name, and each new part's
    /// what it makes of none and the part's name (a GUID nothing has, or another is made).
    pub fn sync_library_with(&mut self, guid: &mut dyn FnMut(&Model, Option<&Kind>, &str) -> Guid) {
        if self.library_in_step() {
            return;
        }
        let now = self.library_names();
        let keep: std::collections::BTreeSet<&(Kind, String)> = now.iter().collect();
        let gone: Vec<Id> =
            self.library.entries().filter(|(_, e)| !keep.contains(&(e.kind.clone(), e.name.clone()))).map(|(id, _)| id).collect();
        for id in gone {
            self.library.remove(id);
        }
        for (kind, name) in now {
            if self.library.id(&kind, &name).is_none() {
                let id = self.fresh_library_id();
                let mut g = guid(self, Some(&kind), &name);
                while self.library.by_guid(g).is_some() || self.elements.contains_key(g) || g.pending().is_some() {
                    g = self.elements.fresh();
                }
                let guid = g;
                let _ = self.library.insert(id, Entry { kind, name, guid });
            }
        }
        if !self.parts_in_step() {
            self.sync_parts(guid);
        }
    }

    /// Every part's id and GUID taken away, for [`Model::sync_library_with`] to give them anew.
    pub(crate) fn forget_parts(&mut self) {
        let t = &mut self.types;
        let layers = t
            .wall
            .values_mut()
            .flat_map(|x| x.layers.iter_mut())
            .chain(t.floor.values_mut().flat_map(|x| x.layers.iter_mut()))
            .chain(t.roof.values_mut().flat_map(|x| x.layers.iter_mut()))
            .chain(t.ceiling.values_mut().flat_map(|x| x.layers.iter_mut()));
        for l in layers {
            (l.id, l.guid) = (0, Guid::default());
        }
        for f in self.families.values_mut() {
            f.forms.iter_mut().for_each(|x| (x.id, x.guid) = (0, Guid::default()));
            f.symbols.iter_mut().for_each(|x| (x.id, x.guid) = (0, Guid::default()));
        }
        for e in self.elements.values_mut() {
            if let Some(lines) = e.sketch_mut() {
                lines.iter_mut().for_each(|l| (l.id, l.guid) = (0, Guid::default()));
            }
        }
        self.library.parts.clear();
    }

    /// The next id for the library, which counts down from [`LAST_ID`] while elements count up from
    /// [`FIRST_ID`](crate::ids::FIRST_ID): one id space, a new project's library not in its elements' way. Should the two
    /// meet, the library goes on past the elements.
    pub fn fresh_library_id(&mut self) -> Id {
        let lowest = self.library.first_id().into_iter().chain(self.library.parts.keys().next().copied()).min();
        let below = lowest.map_or(LAST_ID, |f| f.saturating_sub(1));
        let floor = self.elements.keys().next_back().map_or(crate::ids::FIRST_ID, |k| k + 1).max(self.next_id);
        let mut id = below;
        while id >= floor && (self.elements.contains_key(id) || self.library.taken(id)) {
            id -= 1;
        }
        if id < floor {
            return self.fresh_id();
        }
        id
    }

    /// What `text` names among `kind`: its name, or its GUID.
    pub fn library_name(&self, kind: &Kind, text: &str) -> Option<String> {
        if self.library.id(kind, text).is_some() {
            return Some(text.to_string());
        }
        let (_, e) = self.library.by_guid(Guid::parse(text)?)?;
        (e.kind == *kind).then(|| e.name.clone())
    }
}

/// Each name `g` holds of a library entry, given to `f` with what it names, which may put another in its place: a view's
/// template, filters, phase, phase filter and design options.
fn graphics_refs(g: &mut crate::graphics::Graphics, f: &mut dyn FnMut(&Kind, &mut String)) {
    if let Some(t) = &mut g.template {
        f(&Kind::ViewTemplate, t);
    }
    for a in &mut g.filters {
        f(&Kind::ViewFilter, &mut a.filter);
    }
    if let Some(p) = &mut g.phase {
        f(&Kind::Phase, p);
    }
    if let Some(p) = &mut g.phase_filter {
        f(&Kind::PhaseFilter, p);
    }
    g.options = std::mem::take(&mut g.options)
        .into_iter()
        .map(|(mut set, option)| {
            let option = option_ref(&set, &option, f);
            f(&Kind::OptionSet, &mut set);
            (set, option)
        })
        .collect();
}

/// A component's (or a family door's) family and its type of it, each given to `f`: the type as `family: type`, or as the
/// GUID it is in a file. The family is given first when it is a GUID (a file read: its name, then the type's), else last
/// (its name, while the type is looked up by it).
fn family_and_type(family: &mut String, type_name: &mut String, f: &mut dyn FnMut(&Kind, &mut String)) {
    let is_guid = |t: &str| Guid::parse(t).is_some_and(|g| g.pending().is_none());
    if is_guid(family) {
        f(&Kind::Family, family);
    }
    let mut full = if is_guid(type_name) { type_name.clone() } else { format!("{family}: {type_name}") };
    f(&Kind::FamilyType, &mut full);
    if !is_guid(family) {
        f(&Kind::Family, family);
    }
    // `family: type` back to the type, under the family's name as it is now; a GUID stays one.
    *type_name = match full.strip_prefix(&format!("{family}: ")) {
        Some(t) => t.to_string(),
        None if is_guid(&full) => full,
        // Renamed with its family (Insert from File): the type's part, after the family's old name.
        None => full.split_once(": ").map_or(full.clone(), |(_, t)| t.to_string()),
    };
}

/// Option `option` of set `set`, given to `f` by its full name (`set: option`), or as the GUID it is in a file; what `f`
/// makes of it, as the option's own name, or the GUID it is in a file.
fn option_ref(set: &str, option: &str, f: &mut dyn FnMut(&Kind, &mut String)) -> String {
    let mut full = if Guid::parse(option).is_some_and(|g| g.pending().is_none()) { option.to_string() } else { format!("{set}: {option}") };
    f(&Kind::DesignOption, &mut full);
    full.rsplit_once(": ").map_or(full.clone(), |(_, o)| o.to_string())
}

/// The fields, and the phases and design options compared, a schedule's or a view filter's rules name.
fn rules_refs(fields: &mut [&mut String], rules: &mut [crate::schedule::Filter], f: &mut dyn FnMut(&Kind, &mut String)) {
    for name in fields.iter_mut() {
        f(&Kind::Parameter, name);
    }
    for r in rules {
        let kind = match r.field.as_str() {
            crate::phasing::CREATED | crate::phasing::DEMOLISHED => Some(Kind::Phase),
            crate::phasing::OPTION => Some(Kind::DesignOption),
            _ => None,
        };
        f(&Kind::Parameter, &mut r.field);
        if let (Some(kind), serde_json::Value::String(v)) = (kind, &mut r.value) {
            f(&kind, v);
        }
    }
}

impl crate::model::Element {
    /// Each name it holds of a library entry, given to `f` with what it names, which may put another in its place: its
    /// type, its family, the phases, the design option and the materials painted its `params` hold, a view's template,
    /// filters, phases and options, a schedule's parameter fields and the phases its filters compare. (A family's types
    /// are its own: a component's or a family door's type is the family's.)
    pub fn lib_refs_mut(&mut self, f: &mut dyn FnMut(&Kind, &mut String)) {
        use crate::model::{Element, run_arm};
        let category = self.category().to_string();
        match self {
            Element::Wall(w) => f(&Kind::Type(category), &mut w.type_name),
            Element::Floor(x) => f(&Kind::Type(category), &mut x.type_name),
            Element::Ceiling(x) => f(&Kind::Type(category), &mut x.type_name),
            Element::Roof(x) => f(&Kind::Type(category), &mut x.type_name),
            Element::Railing(x) => f(&Kind::Type(category), &mut x.type_name),
            Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => f(&Kind::Type(category), &mut c.type_name),
            run_arm!(r) => f(&Kind::Type(category), &mut r.type_name),
            Element::Door(o) | Element::Window(o) => match &mut o.family {
                Some(family) => family_and_type(family, &mut o.type_name, f),
                None => f(&Kind::Type(category), &mut o.type_name),
            },
            Element::Component(c) => family_and_type(&mut c.family, &mut c.type_name, f),
            Element::DetailLine(l) => f(&Kind::LineStyle, &mut l.style),
            Element::FilledRegion(r) => {
                f(&Kind::FilledRegionType, &mut r.type_name);
                if let Some(st) = &mut r.line_style {
                    f(&Kind::LineStyle, st);
                }
            }
            Element::Schedule(sc) => {
                let mut fields: Vec<&mut String> =
                    sc.fields.iter_mut().map(|x| &mut x.name).chain(sc.sort.iter_mut().map(|x| &mut x.field)).collect();
                rules_refs(&mut fields, &mut sc.filters, f);
            }
            _ => {}
        }
        if let Some(g) = crate::graphics::graphics_mut(self) {
            graphics_refs(g, f);
        }
        if let Some(params) = self.params_mut() {
            for (key, kind) in [
                (crate::phasing::CREATED, Kind::Phase),
                (crate::phasing::DEMOLISHED, Kind::Phase),
                (crate::phasing::OPTION, Kind::DesignOption),
            ] {
                if let Some(serde_json::Value::String(v)) = params.get_mut(key) {
                    f(&kind, v);
                }
            }
            if let Some(serde_json::Value::Object(paint)) = params.get_mut(crate::paint::KEY) {
                for v in paint.values_mut() {
                    if let serde_json::Value::String(m) = v {
                        f(&Kind::Material, m);
                    }
                }
            }
        }
    }
}

impl Model {
    /// Each name the model holds of a library entry, given to `f` with what it names, which may put another in its place:
    /// its elements' ([`Element::lib_refs_mut`](crate::model::Element::lib_refs_mut)), its types' layers' and runs'
    /// materials, its materials' pictures, its view filters' rules, its view templates' graphics, its option sets'
    /// primaries. (A family's forms name their materials themselves.)
    pub fn lib_refs_mut(&mut self, f: &mut dyn FnMut(&Kind, &mut String)) {
        for e in self.elements.values_mut() {
            e.lib_refs_mut(f);
        }
        let t = &mut self.types;
        let layers = t
            .wall
            .values_mut()
            .flat_map(|x| x.layers.iter_mut())
            .chain(t.floor.values_mut().flat_map(|x| x.layers.iter_mut()))
            .chain(t.roof.values_mut().flat_map(|x| x.layers.iter_mut()))
            .chain(t.ceiling.values_mut().flat_map(|x| x.layers.iter_mut()));
        for l in layers.filter(|l| !l.material.is_empty()) {
            f(&Kind::Material, &mut l.material);
        }
        for r in t.runs.values_mut().flat_map(|x| x.values_mut()).filter(|r| !r.material.is_empty()) {
            f(&Kind::Material, &mut r.material);
        }
        for m in self.materials.values_mut() {
            if let Some(tx) = m.texture.as_mut().filter(|tx| !tx.image.is_empty()) {
                f(&Kind::Picture, &mut tx.image);
            }
        }
        for vf in self.view_filters.values_mut() {
            rules_refs(&mut [], &mut vf.rules, f);
        }
        for vt in self.view_templates.values_mut() {
            graphics_refs(&mut vt.graphics, f);
        }
        for (set, s) in self.option_sets.iter_mut() {
            s.primary = option_ref(set, &s.primary, f);
        }
        for fam in self.families.values_mut() {
            fam.material_refs_mut(&mut |m| f(&Kind::Material, m));
        }
        for t in self.region_types.values_mut() {
            for p in [&mut t.foreground, &mut t.background].into_iter().flatten() {
                f(&Kind::FillPattern, p);
            }
        }
        for p in self.line_styles.values_mut().filter_map(|s| s.pattern.as_mut()) {
            f(&Kind::LinePattern, p);
        }
    }

    /// Each component's and family door's or window's values of its family's instance parameters, keyed by what `key`
    /// makes of `family: parameter` (a GUID in a file); a material parameter's value, by what `material` makes of it.
    fn rekey_family_params(&mut self, key: &dyn Fn(&str, &str) -> Option<String>, material: &dyn Fn(&str) -> Option<String>) {
        let families = &self.families;
        for e in self.elements.values_mut() {
            let Some((family, _)) = e.family() else { continue };
            let Some(fam) = families.get(family) else { continue };
            let family = family.to_string();
            let Some(params) = e.params_mut() else { continue };
            let names: Vec<String> = params.keys().cloned().collect();
            for n in names {
                let Some(k) = key(&family, &n) else { continue };
                let Some(mut v) = params.remove(&n) else { continue };
                // The parameter, by its name: the one given, or the one the key is.
                let named = fam.parameters.iter().find(|p| p.name == n || p.name == k);
                if let (Some(p), Value::String(m)) = (named, &mut v)
                    && p.kind == crate::family::Kind::Material
                    && let Some(to) = material(m)
                {
                    *m = to;
                }
                params.insert(k, v);
            }
        }
    }

    /// Each element's values of project parameters, keyed by what `key` makes of each parameter's name.
    fn rekey_params(&mut self, key: &dyn Fn(&str) -> Option<String>) {
        for params in self.elements.values_mut().filter_map(crate::model::Element::params_mut) {
            let names: Vec<String> = params.keys().filter(|k| key(k).is_some()).cloned().collect();
            for n in names {
                if let (Some(v), Some(k)) = (params.remove(&n), key(&n)) {
                    params.insert(k, v);
                }
            }
        }
    }

    /// The project as a file holds it: every name of a library entry its GUID, and the values of project parameters
    /// keyed by theirs.
    pub fn to_json(&self) -> Result<String, String> {
        let mut m = self.clone();
        m.sync_library();
        let lib = m.library.clone();
        // While the families have their names: their instance parameters' values, by the parameters' GUIDs.
        m.rekey_family_params(&|family, n| lib.guid(&Kind::FamilyParameter, &format!("{family}: {n}")).map(|g| g.to_string()), &|name| {
            lib.guid(&Kind::Material, name).map(|g| g.to_string())
        });
        m.lib_refs_mut(&mut |kind, name| {
            if let Some(g) = lib.guid(kind, name) {
                *name = g.to_string();
            }
        });
        m.rekey_params(&|n| lib.guid(&Kind::Parameter, n).map(|g| g.to_string()));
        serde_json::to_string_pretty(&m).map_err(|e| e.to_string())
    }

    /// What a file's GUIDs name, named again, and what has no id and GUID given them: what [`Model::to_json`] did, undone.
    pub(crate) fn names_from_file(&mut self) {
        let lib = self.library.clone();
        let name = |kind: &Kind, text: &str| {
            let (_, e) = lib.by_guid(Guid::parse(text).filter(|g| g.pending().is_none())?)?;
            (e.kind == *kind).then(|| e.name.clone())
        };
        self.lib_refs_mut(&mut |kind, text| {
            if let Some(n) = name(kind, text) {
                *text = n;
            }
        });
        self.rekey_params(&|k| name(&Kind::Parameter, k));
        self.rekey_family_params(
            &|family, k| name(&Kind::FamilyParameter, k).and_then(|n| n.strip_prefix(&format!("{family}: ")).map(String::from)),
            &|m| name(&Kind::Material, m),
        );
        self.sync_library();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::engine::Session;
    use crate::model::{Element, FIRST_ID};

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn temp(name: &str) -> String {
        std::env::temp_dir().join(format!("bimcraft-library-{name}-{}.bimcraft", std::process::id())).to_string_lossy().into_owned()
    }

    /// A project with something of every kind its elements name: a wall of a layered type, a project parameter it holds, a
    /// phase it's made in, a design option, a material painted on it, a view with a template, a filter and a phase filter.
    fn project() -> (Session, Id) {
        let mut s = Session::new();
        s.script(
            r#"phase {"name": "Survey", "before": "Existing"}
option {"set": "Roofs", "options": ["Flat", "Pitched"]}
parameter {"name": "Fire Rating", "kind": "text", "categories": ["wall"]}
filter {"name": "Long", "categories": ["wall"]}
template {"name": "Plans", "from": 100000}"#,
        )
        .unwrap();
        let w = run(&mut s, "wall", json!({"points": [[0, 0], [5000, 0]], "type": "Exterior - 300mm"}))["ids"][0].as_u64().unwrap();
        run(
            &mut s,
            "modify",
            json!({"id": w, "params": {"Fire Rating": "EI 60", "Phase Created": "Survey", "Design Option": "Roofs: Flat"}}),
        );
        run(&mut s, "paint", json!({"id": w, "face": "exterior", "material": "Timber"}));
        run(&mut s, "graphics", json!({"view": 100000, "template": "Plans"}));
        run(
            &mut s,
            "graphics",
            json!({"view": 100001, "filters": [{"filter": "Long", "hidden": true}], "phase_filter": "Show New", "options": {"Roofs": "Pitched"}}),
        );
        (s, w)
    }

    #[test]
    fn every_library_entry_has_an_id_counting_down_and_a_guid_and_elements_still_count_up() {
        let s = Session::new();
        let m = &s.model;
        assert!(m.library_in_step() && m.library.len() > 50, "{}", m.library.len());
        let ids: Vec<Id> = m.library.entries().map(|(id, _)| id).collect();
        assert_eq!(ids.last(), Some(&LAST_ID), "from the top down");
        assert!(ids.iter().all(|id| (FIRST_ID..=LAST_ID).contains(id) && !m.elements.contains_key(*id)));
        assert_eq!(m.elements.keys().copied().collect::<Vec<_>>(), [100_000, 100_001], "the levels: elements count up");
        let brick = m.library.id(&Kind::Material, "Brick").unwrap();
        assert_eq!(m.library.get(brick).map(|e| e.name.as_str()), Some("Brick"));
        assert!(m.library.guid(&Kind::Type("wall".into()), "Generic - 200mm").is_some());
        assert!(m.library.id(&Kind::Phase, "New Construction").is_some());
        assert_eq!(m.check(), Ok(()));
    }

    #[test]
    fn a_file_holds_the_guid_of_whatever_it_names_and_opens_as_it_was() {
        let (s, w) = project();
        let m = &s.model;
        let g = |kind: Kind, name: &str| json!(m.library.guid(&kind, name).unwrap().to_string());
        let file: Value = serde_json::from_str(&m.to_json().unwrap()).unwrap();
        assert_eq!(file["format"], crate::model::FORMAT);
        let wall = &file["elements"][w.to_string()];
        assert_eq!(wall["type"], g(Kind::Type("wall".into()), "Exterior - 300mm"));
        let params = wall["params"].as_object().unwrap();
        let rating = m.library.guid(&Kind::Parameter, "Fire Rating").unwrap().to_string();
        assert_eq!(params[&rating], "EI 60", "keyed by the parameter's GUID: {params:?}");
        assert_eq!(params["Phase Created"], g(Kind::Phase, "Survey"));
        assert_eq!(params["Design Option"], g(Kind::DesignOption, "Roofs: Flat"));
        assert_eq!(params["Paint"]["exterior"], g(Kind::Material, "Timber"));
        assert_eq!(file["types"]["wall"]["Exterior - 300mm"]["layers"][0]["material"], g(Kind::Material, "Brick"));
        assert_eq!(file["elements"]["100000"]["graphics"]["template"], g(Kind::ViewTemplate, "Plans"));
        let up = &file["elements"]["100001"]["graphics"];
        assert_eq!(up["filters"][0]["filter"], g(Kind::ViewFilter, "Long"));
        assert_eq!(up["phase_filter"], g(Kind::PhaseFilter, "Show New"));
        let roofs = m.library.guid(&Kind::OptionSet, "Roofs").unwrap().to_string();
        assert_eq!(up["options"][&roofs], g(Kind::DesignOption, "Roofs: Pitched"));
        assert_eq!(file["option_sets"]["Roofs"]["primary"], g(Kind::DesignOption, "Roofs: Flat"));
        assert!(file["library"].as_object().is_some_and(|l| l.len() == m.library.len()));
        // Read back: the same project, names and all.
        let back = Model::from_json(&m.to_json().unwrap()).unwrap();
        assert_eq!(back, **m);
    }

    #[test]
    fn a_rename_keeps_an_entrys_id_and_guid_and_changes_nothing_a_file_holds_but_its_name() {
        let (mut s, _) = project();
        let before: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
        let key = |m: &Model, kind: Kind, name: &str| (m.library.id(&kind, name), m.library.guid(&kind, name));
        let was = [
            key(&s.model, Kind::Material, "Brick"),
            key(&s.model, Kind::Parameter, "Fire Rating"),
            key(&s.model, Kind::Phase, "Survey"),
            key(&s.model, Kind::OptionSet, "Roofs"),
            key(&s.model, Kind::DesignOption, "Roofs: Flat"),
            key(&s.model, Kind::Family, "Sofa"),
        ];
        s.script(
            r#"material {"name": "Brick", "rename": "Clay brick"}
parameter {"name": "Fire Rating", "rename": "Fire Resistance"}
phase {"name": "Survey", "rename": "As Found"}
option {"set": "Roofs", "option": "Flat", "rename": "Level"}
option {"set": "Roofs", "rename": "Roof Studies"}
family {"name": "Sofa", "rename": "Couch"}"#,
        )
        .unwrap();
        let now = [
            key(&s.model, Kind::Material, "Clay brick"),
            key(&s.model, Kind::Parameter, "Fire Resistance"),
            key(&s.model, Kind::Phase, "As Found"),
            key(&s.model, Kind::OptionSet, "Roof Studies"),
            key(&s.model, Kind::DesignOption, "Roof Studies: Level"),
            key(&s.model, Kind::Family, "Couch"),
        ];
        assert_eq!(now, was);
        // What the elements, the types and the views hold is as it was: only the names, and the definitions keyed by them,
        // say otherwise.
        let after: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
        assert_eq!(after["elements"], before["elements"]);
        assert_eq!(after["types"], before["types"]);
    }

    #[test]
    fn a_command_takes_a_library_entry_by_its_guid_wherever_it_takes_its_name_and_elements_show_their_types() {
        let mut s = Session::new();
        let exterior = s.model.library.guid(&Kind::Type("wall".into()), "Exterior - 300mm").unwrap();
        let w = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": exterior.to_string()}))["ids"][0].as_u64().unwrap();
        let Some(Element::Wall(wall)) = s.model.elements.get(w) else { panic!() };
        assert_eq!(wall.type_name, "Exterior - 300mm");
        let e = crate::element_json(&s.model, w);
        assert_eq!((e["type"].clone(), e["type_guid"].clone()), (json!("Exterior - 300mm"), json!(exterior.to_string())));
        let r = run(&mut s, "library", json!({"kind": "material", "name": "Brick"}));
        assert_eq!(r["count"], 1);
        assert_eq!(r["library"][0]["guid"], json!(s.model.library.guid(&Kind::Material, "Brick").unwrap().to_string()));
        // Select by ID finds the library too, which isn't selected; IDs of Selection names it.
        let brick = s.model.library.id(&Kind::Material, "Brick").unwrap();
        let r = run(&mut s, "select_by_id", json!({"text": format!("{brick} {w}")}));
        assert_eq!((r["ids"].clone(), r["library"].clone()), (json!([w]), json!([brick])));
        let r = run(&mut s, "ids_of", json!({"ids": [brick]}));
        assert_eq!((r["elements"][0]["category"].clone(), r["elements"][0]["name"].clone()), (json!("material"), json!("Brick")));
        // A name that is a GUID can't be told from one: refused.
        let e = s.execute("material", &json!({"name": "3f2a9c1e-0000-4000-8000-000000000001", "color": [1, 2, 3]})).unwrap_err();
        assert!(e.contains("a name isn't a GUID"), "{e}");
    }

    #[test]
    fn a_file_from_before_library_guids_opens_with_them() {
        let (s, w) = project();
        // As format 1 wrote it: names where GUIDs are now, no library.
        let mut old = serde_json::to_value(&*s.model).unwrap();
        old["format"] = json!(1);
        old.as_object_mut().unwrap().remove("library");
        let m = Model::from_json(&old.to_string()).unwrap();
        assert!(m.library_in_step());
        let Some(Element::Wall(wall)) = m.elements.get(w) else { panic!() };
        assert_eq!((wall.type_name.as_str(), wall.params["Fire Rating"].as_str()), ("Exterior - 300mm", Some("EI 60")));
        assert_eq!(m.check(), Ok(()));
    }

    #[test]
    fn insert_from_file_matches_the_library_by_guid_then_by_name_and_brings_the_rest_with_its_guids() {
        // Two projects from one: this one, and the other, which renames a material, makes a type of its own made of it,
        // and a type of the name of one this project makes apart.
        let (base, _) = project();
        let mut here = Session::new();
        here.reset(Model::from_json(&base.model.to_json().unwrap()).unwrap(), None);
        let mut other = Session::new();
        other.reset(Model::from_json(&base.model.to_json().unwrap()).unwrap(), None);
        other
            .script(
                r#"material {"name": "Brick", "rename": "Red brick"}
type {"category": "wall", "name": "Thin", "layers": [{"material": "Red brick", "thickness": 90}]}
type {"category": "wall", "name": "Partition", "thickness": 110}"#,
            )
            .unwrap();
        let thin = run(&mut other, "wall", json!({"points": [[0, 2000], [4000, 2000]], "type": "Thin"}))["ids"][0].as_u64().unwrap();
        let inner = run(&mut other, "wall", json!({"points": [[0, 4000], [4000, 4000]], "type": "Partition"}))["ids"][0].as_u64().unwrap();
        // Here, a type of that name too, made apart: another GUID.
        run(&mut here, "type", json!({"category": "wall", "name": "Partition", "thickness": 100}));
        let path = temp("other");
        run(&mut other, "save", json!({"path": path}));
        let r = run(&mut here, "insert", json!({"path": path, "ids": [thin, inner]}));
        let _ = std::fs::remove_file(&path);
        let (m, o) = (&here.model, &other.model);
        let wall = Kind::Type("wall".into());
        // A material this project has by GUID is its own, under the name it has here: the type that came in is made of it.
        assert_eq!(m.library.guid(&Kind::Material, "Brick"), o.library.guid(&Kind::Material, "Red brick"));
        assert!(!m.materials.contains_key("Red brick"));
        assert_eq!(m.types.wall["Thin"].layers[0].material, "Brick");
        // A type it hasn't comes in, with its GUID.
        assert_eq!(m.library.guid(&wall, "Thin"), o.library.guid(&wall, "Thin"));
        assert!(r["library"].as_array().unwrap().contains(&json!("wall type: Thin")), "{r}");
        // One it has by name, though not by GUID, is this project's, as it is here.
        assert_ne!(m.library.guid(&wall, "Partition"), o.library.guid(&wall, "Partition"));
        assert_eq!(m.types.wall["Partition"].thickness, 100.0);
        let id = r["ids"][inner.to_string()].as_u64().unwrap();
        assert_eq!(crate::element_json(m, id)["thickness"], 100.0);
        assert_eq!(m.check(), Ok(()));
    }

    /// A family of a box, a type of it and a component of the type, its instance parameter of a material set, with a
    /// plan symbol: the parts and the references a family makes.
    fn boxed() -> (Session, Id) {
        let mut s = Session::new();
        run(
            &mut s,
            "family",
            json!({
                "name": "Box",
                "parameters": [{"name": "Side", "value": 600}, {"name": "Finish", "kind": "material", "instance": true, "value": "Timber"}],
                "types": {"Small": {"Side": 400}, "Large": {}},
                "forms": [
                    {"profile": [[0, 0], ["Side", 0], ["Side", "Side"], [0, "Side"]], "to": "Side", "material": "Finish"},
                    {"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "to": 50, "material": "Metal"}
                ],
            }),
        );
        let c = run(&mut s, "component", json!({"family": "Box", "type": "Small", "at": [0, 0], "params": {"Finish": "Brick"}}))["id"]
            .as_u64()
            .unwrap();
        (s, c)
    }

    #[test]
    fn every_layer_form_and_symbol_and_every_family_type_and_parameter_has_an_id_and_a_guid() {
        let (s, _) = boxed();
        let m = &s.model;
        let parts: Vec<(Id, Part)> = m.parts_now();
        assert!(parts.iter().any(|(_, p)| p.what == "layer" && p.of == "wall type Exterior - 300mm" && p.index == 3));
        assert!(parts.iter().any(|(_, p)| p.what == "form" && p.of == "family Box" && p.index == 1));
        assert!(parts.iter().any(|(_, p)| p.what == "symbol"), "the door families' swings");
        let mut ids: Vec<Id> = parts.iter().map(|(id, _)| *id).collect();
        ids.extend(m.library.entries().map(|(id, _)| id));
        ids.extend(m.elements.keys());
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "one id space: no id twice");
        assert!(parts.iter().all(|(id, p)| *id >= FIRST_ID && !p.guid.is_nil() && m.library.part(*id) == Some(p)));
        for (kind, name) in [(Kind::FamilyType, "Box: Small"), (Kind::FamilyParameter, "Box: Finish")] {
            assert!(m.library.guid(&kind, name).is_some(), "{name}");
        }
        assert_eq!(m.elements.keys().next_back(), Some(&100_002), "elements still count up: the levels, then the component");
        assert_eq!(m.check(), Ok(()));
        let l = s.model.library.part(parts[0].0).unwrap().name();
        assert!(l.starts_with("layer 1 of "), "{l}");
    }

    #[test]
    fn a_file_names_a_family_type_parameter_and_material_by_guid_and_its_parts_hold_theirs() {
        let (s, c) = boxed();
        let m = &s.model;
        let g = |kind: Kind, name: &str| json!(m.library.guid(&kind, name).unwrap().to_string());
        let file: Value = serde_json::from_str(&m.to_json().unwrap()).unwrap();
        let comp = &file["elements"][c.to_string()];
        assert_eq!((comp["family"].clone(), comp["type"].clone()), (g(Kind::Family, "Box"), g(Kind::FamilyType, "Box: Small")));
        let finish = m.library.guid(&Kind::FamilyParameter, "Box: Finish").unwrap().to_string();
        assert_eq!(comp["params"][&finish], g(Kind::Material, "Brick"), "its instance value, keyed by the parameter's GUID: {comp}");
        let family = &file["families"]["Box"];
        assert_eq!(family["forms"][1]["material"], g(Kind::Material, "Metal"));
        assert_eq!(family["forms"][0]["material"], "Finish", "a family's own parameter, by name, as its formulas name it");
        assert_eq!(family["parameters"][1]["value"], g(Kind::Material, "Timber"));
        let form = m.families["Box"].forms[0].clone();
        assert_eq!((family["forms"][0]["id"].clone(), family["forms"][0]["guid"].clone()), (json!(form.id), json!(form.guid.to_string())));
        let back = Model::from_json(&m.to_json().unwrap()).unwrap();
        assert_eq!(back, **m);
    }

    #[test]
    fn a_part_keeps_its_id_and_guid_where_it_stays_in_its_list_and_a_new_one_gets_its_own() {
        let (mut s, _) = boxed();
        let ident = |m: &Model| m.types.wall["Exterior - 300mm"].layers.iter().map(|l| (l.id, l.guid)).collect::<Vec<_>>();
        let before = ident(&s.model);
        // The layers given again, as an agent or the type dialog gives them: no ids. The plaster thicker, a layer more.
        let layers = json!([
            {"function": "finish", "material": "Brick", "thickness": 100},
            {"function": "thermal", "material": "Insulation", "thickness": 50},
            {"function": "structure", "material": "Concrete block", "thickness": 140},
            {"function": "finish", "material": "Plaster", "thickness": 15},
            {"function": "finish", "material": "Plaster", "thickness": 5}
        ]);
        run(&mut s, "type", json!({"category": "wall", "name": "Exterior - 300mm", "layers": layers}));
        let after = ident(&s.model);
        assert_eq!(after[..4], before[..], "each at its place keeps them");
        assert!(!before.contains(&after[4]) && after[4].0 >= FIRST_ID);
        // A family's forms, the same.
        let forms = |m: &Model| m.families["Box"].forms.iter().map(|f| (f.id, f.guid)).collect::<Vec<_>>();
        let was = forms(&s.model);
        let mut given = serde_json::to_value(&s.model.families["Box"].forms).unwrap();
        for f in given.as_array_mut().unwrap() {
            f.as_object_mut().unwrap().retain(|k, _| k != "id" && k != "guid");
        }
        given[1]["to"] = json!(80);
        run(&mut s, "family", json!({"name": "Box", "forms": given}));
        assert_eq!(forms(&s.model), was);
        // A type made from another's layers as `inspect` shows them, ids and all: a copy, with ids and GUIDs of its own.
        let copied = serde_json::to_value(&s.model.types.wall["Exterior - 300mm"].layers).unwrap();
        run(&mut s, "type", json!({"category": "wall", "name": "Copy", "layers": copied}));
        let copy: Vec<(Id, Guid)> = s.model.types.wall["Copy"].layers.iter().map(|l| (l.id, l.guid)).collect();
        assert!(copy.iter().all(|c| !after.contains(c) && !after.iter().any(|a| a.0 == c.0 || a.1 == c.1)), "{copy:?}");
        assert_eq!(ident(&s.model), after, "the original keeps its own");
        assert_eq!(s.model.check(), Ok(()));
    }

    #[test]
    fn a_family_renamed_keeps_its_types_and_parameters_guids_and_what_uses_them_is_unchanged_in_a_file() {
        let (mut s, c) = boxed();
        let before: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
        let ty = s.model.library.guid(&Kind::FamilyType, "Box: Small");
        let param = s.model.library.guid(&Kind::FamilyParameter, "Box: Finish");
        run(&mut s, "family", json!({"name": "Box", "rename": "Crate"}));
        assert_eq!(s.model.library.guid(&Kind::FamilyType, "Crate: Small"), ty);
        assert_eq!(s.model.library.guid(&Kind::FamilyParameter, "Crate: Finish"), param);
        let after: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
        assert_eq!(after["elements"][c.to_string()], before["elements"][c.to_string()]);
    }

    #[test]
    fn insert_from_file_brings_a_family_with_its_types_parameters_and_forms_guids() {
        let (other, c) = boxed();
        let path = temp("family");
        let mut o = Session::new();
        o.reset((*other.model).clone(), None);
        run(&mut o, "save", json!({"path": path}));
        let mut here = Session::new();
        let r = run(&mut here, "insert", json!({"path": path, "ids": [c]}));
        let _ = std::fs::remove_file(&path);
        let (m, from) = (&here.model, &other.model);
        for (kind, name) in [(Kind::Family, "Box"), (Kind::FamilyType, "Box: Small"), (Kind::FamilyParameter, "Box: Finish")] {
            assert_eq!(m.library.guid(&kind, name), from.library.guid(&kind, name), "{name}: {r}");
        }
        let guids = |m: &Model| m.families["Box"].forms.iter().map(|f| f.guid).collect::<Vec<_>>();
        assert_eq!(guids(m), guids(from), "the forms keep theirs");
        let id = r["ids"][c.to_string()].as_u64().unwrap();
        let Some(Element::Component(comp)) = m.elements.get(id) else { panic!() };
        assert_eq!((comp.family.as_str(), comp.type_name.as_str(), comp.params["Finish"].as_str()), ("Box", "Small", Some("Brick")));
        assert_eq!(m.check(), Ok(()));
    }

    #[test]
    fn select_by_id_finds_a_layer_and_ids_of_names_it() {
        let (mut s, _) = boxed();
        let layer = s.model.types.wall["Exterior - 300mm"].layers[1].clone();
        let r = run(&mut s, "select_by_id", json!({"text": layer.guid.to_string()}));
        assert_eq!(r["library"], json!([layer.id]));
        let r = run(&mut s, "ids_of", json!({"ids": [layer.id]}));
        assert_eq!(
            (r["elements"][0]["category"].clone(), r["elements"][0]["name"].clone()),
            (json!("layer"), json!("layer 2 of wall type Exterior - 300mm"))
        );
        let r = run(&mut s, "library", json!({"kind": "form", "name": "Box"}));
        assert_eq!(r["parts"].as_array().map(Vec::len), Some(2));
    }
}
