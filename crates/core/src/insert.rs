//! Elements of another project brought into this one, as Revit's Insert from File: those asked for, and what they point at
//! that this project hasn't. An element keeps its id where this project has none of it, else gets the next; it keeps its
//! GUID, unless this project has an element of that GUID already (the same project inserted into itself, or a copy of it),
//! when it is a copy, and a copy is a new element with a new GUID. What the inserted elements point at among themselves
//! follows them; what they point at that this project has (by GUID, or a level of the same name) they point at here. The
//! library they use comes as they do: a type, a material, a family this project has by GUID is its own, whatever it's
//! called here; one it has only by name is this project's; one it hasn't comes in, with its GUID. A project parameter is
//! the exception: what a parameter means is in its values, so one of a name taken here by another GUID comes in apart, as
//! `name 2`, unless `same_parameters` says it's the same; and only the parameters what comes in uses come along.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::graphics;
use crate::ids::LINKED;
use crate::library::{Entry, Kind};
use crate::model::{Element, Guid, Id, Model};
use crate::phasing;

/// What an insert made.
#[derive(Debug, Default, PartialEq)]
pub struct Inserted {
    /// Each element brought in: its id in the other project, and its id here.
    pub ids: BTreeMap<Id, Id>,
    /// The elements here that are copies, with a GUID of their own, because this project had theirs.
    pub copies: Vec<Id>,
    /// The types, families, materials, pictures and parameters brought in, each with its GUID, as `what: name`.
    pub library: Vec<String>,
    /// What was left off an element, and why.
    pub left_out: Vec<String>,
    /// The parameters that go by another name here: what they're called there, and here.
    pub parameters: BTreeMap<String, String>,
}

/// Elements `ids` of `from` (all of them when none), and what they point at that `to` hasn't, brought into `to`. Unchecked:
/// the engine checks the model after the command.
pub fn insert(to: &mut Model, from: &Model, ids: &[Id]) -> Result<Inserted, String> {
    insert_as(to, from, ids, false, &BTreeSet::new())
}

/// [`insert`], the parameters named in `same` taken for this project's of that name, whatever their GUIDs.
pub fn insert_with(to: &mut Model, from: &Model, ids: &[Id], same: &BTreeSet<String>) -> Result<Inserted, String> {
    insert_as(to, from, ids, false, same)
}

/// [`insert`], where with `linked` every element comes in under an id from [`LINKED`] up, which is how a view tells it from
/// the project's own.
pub(crate) fn insert_as(to: &mut Model, from: &Model, ids: &[Id], linked: bool, same: &BTreeSet<String>) -> Result<Inserted, String> {
    if linked {
        to.next_id = to.next_id.max(LINKED);
    }
    let asked: Vec<Id> = if ids.is_empty() { from.elements.keys().copied().collect() } else { ids.to_vec() };
    if let Some(missing) = asked.iter().find(|id| !from.elements.contains_key(**id)) {
        return Err(format!("the other project has no element {missing}"));
    }
    // A level this project has by name stands in for the other's, when the other's isn't asked for.
    let levels_here: HashMap<&str, Guid> = to.levels().into_iter().map(|(id, l)| (l.name.as_str(), to.guid(id))).collect();
    let mut instead: HashMap<Guid, Guid> = HashMap::new();
    // What's asked, and what it points at that this project hasn't, and so on.
    let mut take: BTreeSet<Id> = asked.iter().copied().collect();
    let mut todo = asked.clone();
    while let Some(id) = todo.pop() {
        for g in from.elements.get(id).map(needs).unwrap_or_default() {
            let Some(rid) = from.elements.id(g) else { continue };
            if take.contains(&rid) || to.elements.contains_key(g) {
                continue;
            }
            if let (Some(l), Some(from_guid)) = (from.level(rid), from.elements.guid(rid))
                && let Some(&here) = levels_here.get(l.name.as_str())
            {
                instead.insert(from_guid, here);
                continue;
            }
            take.insert(rid);
            todo.push(rid);
        }
    }
    let mut out = Inserted::default();
    // An element whose GUID this project has is a copy: a GUID of its own.
    for &id in &take {
        let Some(g) = from.elements.guid(id) else { continue };
        if to.elements.contains_key(g) {
            let mut fresh = to.elements.fresh();
            while instead.values().any(|v| *v == fresh) {
                fresh = to.elements.fresh();
            }
            instead.insert(g, fresh);
        }
    }
    let mut renames = bring_library(to, from, &take, &mut out)?;
    let param_renames = bring_parameters(to, from, &take, ids.is_empty(), same, &mut out)?;
    for (old, new) in &param_renames {
        renames.insert((Kind::Parameter, old.clone()), new.clone());
    }
    bring_type_values(to, from, &renames, &mut out.left_out);
    let mut names = Names::of(to);
    for &id in &take {
        let (Some(e), Some(g)) = (from.elements.get(id), from.elements.guid(id)) else { continue };
        let mut e = e.clone();
        e.refs_mut(&mut |r| {
            if let Some(n) = instead.get(r) {
                *r = *n;
            }
        });
        names.free(&mut e);
        e.lib_refs_mut(&mut |kind, name| {
            if let Some(n) = renames.get(&(kind.clone(), name.clone())) {
                *name = n.clone();
            }
        });
        // A family's own parameter of that name is its family's, and stays as it is.
        let theirs = |n: &str| e_family_holds(from, id, n);
        let moves: Vec<(&String, &String)> = param_renames.iter().filter(|(o, _)| !theirs(o)).collect();
        // Through names no value holds, so that two that swap don't land on one another.
        for (i, (old, _)) in moves.iter().enumerate() {
            crate::parameters::rename_values(&mut e, old, &format!("\u{1}{i}"));
        }
        for (i, (_, new)) in moves.iter().enumerate() {
            crate::parameters::rename_values(&mut e, &format!("\u{1}{i}"), new);
        }
        let mut left = phasing_here(to, &mut e);
        left.extend(values_here(to, from, id, &mut e));
        let taken = to.elements.contains_key(id) || to.library.get(id).is_some();
        let new = if linked || taken || id < crate::model::FIRST_ID { to.fresh_id() } else { id };
        let guid = instead.get(&g).copied().unwrap_or(g);
        to.elements.insert_as(new, guid, e)?;
        to.next_id = to.next_id.max(new + 1);
        out.left_out.extend(left.into_iter().map(|why| format!("element {new}: {why}")));
        if guid != g {
            out.copies.push(new);
        }
        out.ids.insert(id, new);
    }
    tidy_graphics(to);
    Ok(out)
}

/// What `e` can't be without: what it points at, but the elements its view's graphics override, which are left behind.
fn needs(e: &Element) -> Vec<Guid> {
    let overridden: BTreeSet<Guid> = graphics::graphics_of_element(e).map(|g| g.elements.keys().copied().collect()).unwrap_or_default();
    e.refs().into_iter().filter(|g| !overridden.contains(g)).collect()
}

/// The names each library entry `from` names goes by here, where they differ: `(kind, name there)` to the name here.
type Renames = HashMap<(Kind, String), String>;

/// The library entries of `from` that `to` hasn't: types, materials, pictures, families, project parameters, line patterns
/// and line styles come along,
/// each with its GUID. An entry this project has by GUID is its own, under the name it has here (the names in what comes in
/// follow); one it has by name, with another GUID, is this project's. What comes in names entries by the names returned.
fn bring_library(to: &mut Model, from: &Model, take: &BTreeSet<Id>, out: &mut Inserted) -> Result<Renames, String> {
    let mut renames = Renames::new();
    if take.is_empty() {
        return Ok(renames);
    }
    let mut copied = Vec::new();
    for (_, e) in from.library.entries() {
        // Parameters, and the dictionaries they came from, go by what uses them (`bring_parameters`).
        if matches!(e.kind, Kind::Parameter | Kind::Dictionary) {
            continue;
        }
        if let Some((_, here)) = to.library.by_guid(e.guid) {
            if here.kind == e.kind && here.name != e.name {
                renames.insert((e.kind.clone(), e.name.clone()), here.name.clone());
            }
            continue;
        }
        if to.library.id(&e.kind, &e.name).is_some() || !copy_definition(to, from, &e.kind, &e.name) {
            continue;
        }
        let id = to.fresh_library_id();
        to.library.insert(id, Entry { kind: e.kind.clone(), name: e.name.clone(), guid: e.guid })?;
        out.library.push(format!("{}: {}", e.kind.label(), e.name));
        copied.push((e.kind.clone(), e.name.clone()));
    }
    // A family that came in brings its types and parameters, with their GUIDs.
    let came: BTreeSet<String> = copied.iter().filter(|(k, _)| *k == Kind::Family).map(|(_, n)| n.clone()).collect();
    for (_, e) in from.library.entries().filter(|(_, e)| matches!(e.kind, Kind::FamilyType | Kind::FamilyParameter)) {
        let of = came.iter().any(|f| e.name.starts_with(&format!("{f}: ")));
        if of && to.library.id(&e.kind, &e.name).is_none() && to.library.by_guid(e.guid).is_none() {
            let id = to.fresh_library_id();
            to.library.insert(id, Entry { kind: e.kind.clone(), name: e.name.clone(), guid: e.guid })?;
        }
    }
    // What came in names the materials, pictures and line patterns it's made of as this project does.
    let here = |kind: &Kind, name: &mut String| {
        if let Some(n) = renames.get(&(kind.clone(), name.clone())) {
            *name = n.clone();
        }
    };
    for (kind, name) in copied {
        match kind {
            Kind::Family => {
                if let Some(f) = to.families.get_mut(&name) {
                    f.material_refs_mut(&mut |m| here(&Kind::Material, m));
                }
            }
            Kind::Material => {
                if let Some(t) = to.materials.get_mut(&name).and_then(|m| m.texture.as_mut()) {
                    here(&Kind::Picture, &mut t.image);
                }
                for p in to.materials.get_mut(&name).into_iter().flat_map(|x| [&mut x.cut_pattern, &mut x.surface_pattern]).flatten() {
                    here(&Kind::FillPattern, p);
                }
            }
            Kind::LineStyle => {
                if let Some(p) = to.line_styles.get_mut(&name).and_then(|s| s.pattern.as_mut()) {
                    here(&Kind::LinePattern, p);
                }
            }
            Kind::DetailGroup => {
                for m in to.detail_groups.get_mut(&name).into_iter().flat_map(|g| g.members.iter_mut()) {
                    m.element.lib_refs_mut(&mut |k: &Kind, n: &mut String| here(k, n));
                }
            }
            Kind::FilledRegionType => {
                if let Some(t) = to.region_types.get_mut(&name) {
                    for p in [&mut t.foreground, &mut t.background].into_iter().flatten() {
                        here(&Kind::FillPattern, p);
                    }
                }
            }
            Kind::DistributionSystem => {
                for v in to.distribution_systems.get_mut(&name).into_iter().flat_map(|d| d.voltages_mut()) {
                    here(&Kind::Voltage, v);
                }
            }
            Kind::Type(c) => {
                let t = &mut to.types;
                let layers = match c.as_str() {
                    "wall" => t.wall.get_mut(&name).map(|x| &mut x.layers),
                    "floor" => t.floor.get_mut(&name).map(|x| &mut x.layers),
                    "roof" => t.roof.get_mut(&name).map(|x| &mut x.layers),
                    "ceiling" => t.ceiling.get_mut(&name).map(|x| &mut x.layers),
                    _ => None,
                };
                for l in layers.into_iter().flatten() {
                    here(&Kind::Material, &mut l.material);
                }
                if let Some(r) = t.runs.get_mut(&c).and_then(|r| r.get_mut(&name)) {
                    here(&Kind::Material, &mut r.material);
                }
            }
            _ => {}
        }
    }
    Ok(renames)
}

/// Whether the family element `id` of `from` is of has a parameter `name`, which makes its value of `name` the family's.
fn e_family_holds(from: &Model, id: Id, name: &str) -> bool {
    from.elements
        .get(id)
        .and_then(Element::family)
        .and_then(|(f, _)| from.families.get(f))
        .is_some_and(|f| f.parameters.iter().any(|q| q.name == name))
}

/// The project parameters of `from` that the elements `take` use (all of them, for a whole project), brought into `to`: by
/// GUID first, so one renamed here or there is still itself; one of a name taken by another GUID kept apart, unless `same`
/// names it. A parameter that's here takes on the categories what comes in holds it for. What each goes by here, where it
/// differs.
fn bring_parameters(
    to: &mut Model,
    from: &Model,
    take: &BTreeSet<Id>,
    whole: bool,
    same: &BTreeSet<String>,
    out: &mut Inserted,
) -> Result<BTreeMap<String, String>, String> {
    // What's used, and the categories of what holds a value of each.
    let mut used: BTreeSet<String> = if whole { from.parameters.keys().cloned().collect() } else { BTreeSet::new() };
    let mut holders: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for &id in take {
        let Some(e) = from.elements.get(id) else { continue };
        // A type parameter comes along with an element whose type has a value of it.
        if let Some(g) = crate::parameters::type_guid(from, e) {
            for (name, _) in from.parameters.iter().filter(|(_, p)| p.type_values.contains_key(&g)) {
                used.insert(name.clone());
                holders.entry(name.clone()).or_default().insert(e.category().to_string());
            }
        }
        for name in e.params().into_iter().flat_map(|p| p.keys()) {
            if from.parameters.contains_key(name) && !e_family_holds(from, id, name) {
                used.insert(name.clone());
                holders.entry(name.clone()).or_default().insert(e.category().to_string());
            }
        }
        let mut copy = e.clone();
        copy.lib_refs_mut(&mut |kind, name| {
            if *kind == Kind::Parameter && from.parameters.contains_key(name.as_str()) {
                used.insert(name.clone());
            }
        });
        if let Element::Schedule(sc) = e {
            for text in sc.fields.iter().filter_map(|f| f.formula.as_ref()) {
                used.extend(from.parameters.keys().filter(|n| crate::parameters::formula_names(text, n)).cloned());
            }
        }
    }
    let mut renames = BTreeMap::new();
    for name in &used {
        let Some(p) = from.parameters.get(name) else { continue };
        let guid = from.library.guid(&Kind::Parameter, name);
        let known = guid.and_then(|g| match to.library.by_guid(g) {
            Some((_, e)) if e.kind == Kind::Parameter => Some(e.name.clone()),
            _ => to.parameters.iter().find(|(_, q)| q.same_as.contains(&g)).map(|(n, _)| n.clone()),
        });
        let here = match known {
            Some(n) => Some(n),
            None if to.parameters.contains_key(name) && same.contains(name) => {
                if let (Some(q), Some(g)) = (to.parameters.get_mut(name), guid)
                    && !q.same_as.contains(&g)
                {
                    q.same_as.push(g);
                }
                Some(name.clone())
            }
            None => None,
        };
        let here = match here {
            Some(n) => n,
            None => {
                // New here, under its name if that's free.
                let free = std::iter::once(name.clone())
                    .chain((2..).map(|i| format!("{name} {i}")))
                    .find(|n| !to.parameters.contains_key(n))
                    .unwrap_or_else(|| name.clone());
                let mut copy = p.clone();
                copy.same_as.clear();
                // Its values are its types', which come in with what's inserted ([`bring_type_values`]).
                copy.type_values.clear();
                if let Some(d) = copy.dictionary.clone() {
                    match from.dictionaries.get(&d) {
                        Some(r) => {
                            to.dictionaries.entry(d.clone()).or_insert_with(|| r.clone());
                            if let Some(g) = from.library.guid(&Kind::Dictionary, &d)
                                && to.library.id(&Kind::Dictionary, &d).is_none()
                                && to.library.by_guid(g).is_none()
                            {
                                let id = to.fresh_library_id();
                                to.library.insert(id, Entry { kind: Kind::Dictionary, name: d, guid: g })?;
                            }
                        }
                        None => copy.dictionary = None,
                    }
                }
                to.parameters.insert(free.clone(), copy);
                if let Some(g) = guid.filter(|g| to.library.by_guid(*g).is_none()) {
                    let id = to.fresh_library_id();
                    to.library.insert(id, Entry { kind: Kind::Parameter, name: free.clone(), guid: g })?;
                }
                out.library.push(format!("{}: {free}", Kind::Parameter.label()));
                free
            }
        };
        if let Some(q) = to.parameters.get_mut(&here) {
            for c in holders.get(name).into_iter().flatten() {
                if !q.categories.contains(c) {
                    q.categories.push(c.clone());
                }
            }
            q.categories.sort();
        }
        if here != *name {
            out.parameters.insert(name.clone(), here.clone());
            renames.insert(name.clone(), here);
        }
    }
    Ok(renames)
}

/// The values of type parameters of `from`, set on the types they're on as `to` has them. A type's GUID may differ here, so a
/// type is found by its name, after `renames`. A parameter that is an instance one here takes none, and says so.
fn bring_type_values(to: &mut Model, from: &Model, renames: &Renames, why: &mut Vec<String>) {
    to.sync_library();
    for (name, p) in from.parameters.iter().filter(|(_, p)| !p.type_values.is_empty()) {
        let here = renames.get(&(Kind::Parameter, name.clone())).unwrap_or(name).clone();
        for (g, v) in &p.type_values {
            let Some((_, entry)) = from.library.by_guid(*g) else { continue };
            let there = renames.get(&(entry.kind.clone(), entry.name.clone())).unwrap_or(&entry.name);
            let Some(type_here) = to.library.guid(&entry.kind, there) else { continue };
            let Some(q) = to.parameters.get_mut(&here) else { continue };
            if q.scope != crate::model::Scope::Type {
                why.push(format!("`{here}` is an instance parameter here, so type `{there}`'s value {v} was left off"));
            } else if q.holds(v) {
                q.type_values.entry(type_here).or_insert_with(|| v.clone());
            }
        }
    }
}

/// The values `e` holds that its parameter here can't hold, taken off it; why, for each. (The kind there and here can differ
/// even by GUID: each project keeps its copy of the definition.)
fn values_here(to: &Model, from: &Model, id: Id, e: &mut Element) -> Vec<String> {
    let names: Vec<String> = e.params().into_iter().flat_map(|p| p.keys().cloned()).collect();
    let mut why = Vec::new();
    for name in names {
        let Some(q) = to.parameters.get(&name) else { continue };
        if e_family_holds(from, id, &name) {
            continue;
        }
        let Some(params) = e.params_mut() else { continue };
        if q.scope == crate::model::Scope::Type {
            if let Some(v) = params.remove(&name) {
                why.push(format!("`{name}` is a type parameter here, so its value {v} was left off"));
            }
            continue;
        }
        if params.get(&name).is_some_and(|v| !q.holds(v))
            && let Some(v) = params.remove(&name)
        {
            why.push(format!("`{name}` holds {} here, so its value {v} was left off", crate::parameters::kind_word(q.kind)));
        }
    }
    why
}

/// Entry `kind` `name` of `from`, put in `to` as it is there: whether it was one to put.
fn copy_definition(to: &mut Model, from: &Model, kind: &Kind, name: &str) -> bool {
    fn one<T: Clone>(to: &mut BTreeMap<String, T>, from: Option<&BTreeMap<String, T>>, name: &str) -> bool {
        from.and_then(|f| f.get(name)).map(|v| to.insert(name.to_string(), v.clone())).is_some()
    }
    let (t, f) = (&mut to.types, &from.types);
    match kind {
        Kind::Type(c) => match c.as_str() {
            "wall" => one(&mut t.wall, Some(&f.wall), name),
            "floor" => one(&mut t.floor, Some(&f.floor), name),
            "ceiling" => one(&mut t.ceiling, Some(&f.ceiling), name),
            "roof" => one(&mut t.roof, Some(&f.roof), name),
            "door" => one(&mut t.door, Some(&f.door), name),
            "window" => one(&mut t.window, Some(&f.window), name),
            "column" => one(&mut t.column, Some(&f.column), name),
            "structuralcolumn" => one(&mut t.structural_column, Some(&f.structural_column), name),
            "footing" => one(&mut t.footing, Some(&f.footing), name),
            "railing" => one(&mut t.railing, Some(&f.railing), name),
            run => one(t.runs.entry(run.to_string()).or_default(), f.runs.get(run), name),
        },
        Kind::Material => one(&mut to.materials, Some(&from.materials), name),
        Kind::Picture => one(&mut to.images, Some(&from.images), name),
        Kind::Cloud => one(&mut to.clouds, Some(&from.clouds), name),
        Kind::Cad => one(&mut to.cads, Some(&from.cads), name),
        Kind::Family => one(&mut to.families, Some(&from.families), name),
        Kind::Parameter => one(&mut to.parameters, Some(&from.parameters), name),
        Kind::LinePattern => one(&mut to.line_patterns, Some(&from.line_patterns), name),
        Kind::LineStyle => one(&mut to.line_styles, Some(&from.line_styles), name),
        Kind::FillPattern => one(&mut to.fill_patterns, Some(&from.fill_patterns), name),
        Kind::FilledRegionType => one(&mut to.region_types, Some(&from.region_types), name),
        Kind::DetailGroup => one(&mut to.detail_groups, Some(&from.detail_groups), name),
        Kind::TextType => one(&mut to.text_types, Some(&from.text_types), name),
        Kind::ReinforcementPreset => one(&mut to.reinforcement, Some(&from.reinforcement), name),
        Kind::DimensionType => one(&mut to.dimension_types, Some(&from.dimension_types), name),
        Kind::Voltage => one(&mut to.voltages, Some(&from.voltages), name),
        Kind::DistributionSystem => one(&mut to.distribution_systems, Some(&from.distribution_systems), name),
        Kind::WireType => one(&mut to.wire_types, Some(&from.wire_types), name),
        Kind::Dictionary => one(&mut to.dictionaries, Some(&from.dictionaries), name),
        _ => false,
    }
}

/// The phases and the design option `e` holds that this project hasn't, taken off it: it is made in the first phase, in no
/// option, unless this project has them. Why, for each.
fn phasing_here(to: &Model, e: &mut Element) -> Vec<String> {
    let Some(params) = e.params_mut() else { return Vec::new() };
    let mut why = Vec::new();
    for key in [phasing::CREATED, phasing::DEMOLISHED] {
        if let Some(p) = params.get(key).and_then(|v| v.as_str()).filter(|p| !to.phases.iter().any(|q| q == p)).map(String::from) {
            params.remove(key);
            why.push(format!("`{key}` {p} isn't a phase of this project"));
        }
    }
    if let Some(o) = params.get(phasing::OPTION).and_then(|v| v.as_str()).filter(|o| !phasing::option_exists(to, o)).map(String::from) {
        params.remove(phasing::OPTION);
        why.push(format!("`{}` {o} isn't a design option of this project", phasing::OPTION));
    }
    why
}

/// The names and numbers that must differ, taken: a level's or a view's name, a grid's, a sheet's number.
struct Names {
    views: BTreeSet<String>,
    grids: BTreeSet<String>,
    sheets: BTreeSet<String>,
}

impl Names {
    fn of(m: &Model) -> Names {
        let mut n = Names { views: BTreeSet::new(), grids: BTreeSet::new(), sheets: BTreeSet::new() };
        for (id, e) in &m.elements {
            match e {
                Element::Grid(g) => n.grids.insert(g.name.clone()),
                Element::Sheet(sh) => n.sheets.insert(sh.number.clone()),
                _ => m.view_name(*id).is_some_and(|v| n.views.insert(v.to_string())),
            };
        }
        // The standard schedules are views before they're elements.
        n.views.extend(crate::schedule::VIEWS.iter().map(|(_, name)| m.language.word(name).to_string()));
        n
    }

    /// `e` named or numbered apart from what's taken (a ` 2`, ` 3`… after a name that is), and its name taken.
    fn free(&mut self, e: &mut Element) {
        let (name, taken) = match e {
            Element::Level(l) => (&mut l.name, &mut self.views),
            Element::Section(v) | Element::Elevation(v) => (&mut v.name, &mut self.views),
            Element::View3D(v) => (&mut v.name, &mut self.views),
            Element::Schedule(sc) => (&mut sc.name, &mut self.views),
            Element::Grid(g) => (&mut g.name, &mut self.grids),
            Element::Sheet(sh) => (&mut sh.number, &mut self.sheets),
            _ => return,
        };
        if taken.contains(name.as_str()) {
            let base = name.clone();
            *name = (2..).map(|n| format!("{base} {n}")).find(|n| !taken.contains(n)).unwrap_or_default();
        }
        taken.insert(name.clone());
    }
}

/// Each view's overrides of elements the project hasn't, dropped: what an inserted view overrode that wasn't brought in.
fn tidy_graphics(m: &mut Model) {
    let known: BTreeSet<Guid> = m.elements.with_guids().map(|(_, g, _)| g).collect();
    for g in m.elements.values_mut().filter_map(graphics::graphics_mut) {
        g.elements.retain(|k, _| known.contains(k));
        g.linework.retain(|l| known.contains(&l.element));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::engine::Session;
    use crate::model::{Element, FIRST_ID, Guid};

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    /// A box of walls with a door, saved to a file of its own: the session and the path.
    fn saved(name: &str) -> (Session, String) {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
        run(&mut s, "door", json!({"at": [2000, 0]}));
        run(&mut s, "grid", json!({"start": [-1000, 3000], "end": [9000, 3000]}));
        let path = std::env::temp_dir().join(format!("bimcraft-insert-{name}-{}.bimcraft", std::process::id()));
        let path = path.to_string_lossy().into_owned();
        run(&mut s, "save", json!({"path": path}));
        (s, path)
    }

    fn guids(s: &Session) -> Vec<Guid> {
        s.model.elements.with_guids().map(|(_, g, _)| g).collect()
    }

    #[test]
    fn a_file_inserted_into_itself_comes_in_as_copies_with_new_ids_and_new_guids_pointing_at_each_other() {
        let (mut s, path) = saved("self");
        let before = guids(&s);
        let n = s.model.elements.len();
        let r = run(&mut s, "insert", json!({"path": path}));
        let _ = std::fs::remove_file(&path);
        assert_eq!(s.model.elements.len(), 2 * n, "all of it, again");
        let ids = r["ids"].as_object().unwrap();
        assert_eq!(ids.len(), n);
        // Every id was taken, so each came in under a new one past the last; every GUID was here, so each is a copy.
        for (there, here) in ids {
            assert!(here.as_u64().unwrap() > there.parse::<u64>().unwrap() && here.as_u64().unwrap() >= FIRST_ID + n as u64);
        }
        assert_eq!(r["copies"].as_array().unwrap().len(), n);
        let after = guids(&s);
        assert!(before.iter().all(|g| after.contains(g)), "the originals keep theirs");
        // The copied door is in the copied wall, which stands on the copied level, renamed apart.
        let door = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Door(_))).map(|(id, _)| *id).max().unwrap();
        let Some(Element::Door(d)) = s.model.elements.get(door) else { panic!() };
        let wall = s.model.wall(d.host).unwrap();
        assert!(!before.contains(&d.host) && !before.contains(&wall.level));
        assert_eq!(s.model.level(wall.level).unwrap().name, "Level 1 2");
        assert_eq!(s.model.check(), Ok(()));
        run(&mut s, "undo", json!({}));
        assert_eq!(s.model.elements.len(), n, "one undo step");
    }

    #[test]
    fn elements_of_another_project_keep_their_guids_take_new_ids_where_taken_and_stand_on_levels_of_the_same_name() {
        let (other, path) = saved("other");
        let mut s = Session::new();
        let level_here = s.model.guid(FIRST_ID);
        // Both projects number from 100000: this one's ids are taken.
        for x in 0..6 {
            run(&mut s, "grid", json!({"start": [x * 1000, -9000], "end": [x * 1000, -8000]}));
        }
        let door = other.model.elements.iter().find(|(_, e)| matches!(e, Element::Door(_))).map(|(id, _)| *id).unwrap();
        let r = run(&mut s, "insert", json!({"path": path, "ids": [door]}));
        // The door brings its wall, which stands on Level 1: this project's.
        assert_eq!(r["ids"].as_object().unwrap().len(), 2, "{r}");
        assert_eq!(r["copies"], json!([]));
        let here = r["ids"][door.to_string()].as_u64().unwrap();
        assert_ne!(here, door, "its id was taken");
        assert_eq!(s.model.guid(here), other.model.guid(door), "a GUID is the element's for good");
        let Some(Element::Door(d)) = s.model.elements.get(here).cloned() else { panic!() };
        assert_eq!(s.model.wall(d.host).unwrap().level, level_here);
        assert_eq!(s.model.levels().len(), 2, "no level came in");
        assert!(s.model.check().is_ok());
        // Inserted again, the grid is here already: a copy, with a GUID of its own, named apart (as the first was, from
        // this project's grids).
        let grid = other.model.elements.iter().find(|(_, e)| matches!(e, Element::Grid(_))).map(|(id, _)| *id).unwrap();
        let name_there = match other.model.elements.get(grid) {
            Some(Element::Grid(g)) => g.name.clone(),
            _ => panic!(),
        };
        let first = run(&mut s, "insert", json!({"path": path, "ids": [grid]}))["ids"][grid.to_string()].as_u64().unwrap();
        let again = run(&mut s, "insert", json!({"path": path, "ids": [grid]}));
        let _ = std::fs::remove_file(&path);
        let copy = again["ids"][grid.to_string()].as_u64().unwrap();
        assert_eq!(again["copies"], json!([copy]));
        assert_eq!(s.model.guid(first), other.model.guid(grid));
        assert_ne!(s.model.guid(copy), s.model.guid(first));
        let name = |id| match s.model.elements.get(id) {
            Some(Element::Grid(g)) => g.name.clone(),
            _ => panic!(),
        };
        assert!(name(first).starts_with(&name_there) && name(copy).starts_with(&name_there) && name(copy) != name(first));
    }

    #[test]
    fn what_isnt_there_or_isnt_a_project_is_refused() {
        let mut s = Session::new();
        assert!(s.execute("insert", &json!({})).unwrap_err().contains("`path`"));
        assert!(s.execute("insert", &json!({"path": "/nowhere/x.bimcraft"})).is_err());
        let (_, path) = saved("missing");
        let e = s.execute("insert", &json!({"path": path, "ids": [5]})).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(e.contains("no element 5"), "{e}");
    }

    #[test]
    fn a_camera_view_comes_in_with_its_camera_and_its_own_name() {
        let mut other = Session::new();
        run(&mut other, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
        let made = run(&mut other, "camera", json!({"eye": [-5000.0, -4000.0, 1600.0], "target": [4000.0, 3000.0, 1200.0], "lens": 24}));
        let camera_there = made["id"].as_u64().unwrap();
        let path = std::env::temp_dir().join(format!("bimcraft-insert-camera-{}.bimcraft", std::process::id()));
        let path = path.to_string_lossy().into_owned();
        run(&mut other, "save", json!({"path": path}));
        // This project already has a Camera 1, so the inserted one is named apart.
        let mut s = Session::new();
        run(&mut s, "camera", json!({"eye": [0.0, -9000.0, 1700.0], "target": [4000.0, 3000.0, 1000.0]}));
        let r = run(&mut s, "insert", json!({"path": path, "ids": [camera_there]}));
        let _ = std::fs::remove_file(&path);
        let here = r["ids"][camera_there.to_string()].as_u64().unwrap();
        let Some(Element::View3D(v)) = s.model.elements.get(here) else { panic!("the camera view came in") };
        assert_eq!(v.camera, Some(crate::model::ViewCamera { eye: [-5000.0, -4000.0, 1600.0], target: [4000.0, 3000.0, 1200.0] }));
        assert_eq!(v.perspective.filter(|p| p.on).map(|p| p.lens), Some(24.0));
        assert!(v.name.starts_with("Camera 1") && v.name != "Camera 1", "named apart from this project's: {}", v.name);
        assert_eq!(s.model.check(), Ok(()));
    }
}
