//! Elements of another project brought into this one, as Revit's Insert from File: those asked for, and what they point at
//! that this project hasn't. An element keeps its id where this project has none of it, else gets the next; it keeps its
//! GUID, unless this project has an element of that GUID already (the same project inserted into itself, or a copy of it),
//! when it is a copy, and a copy is a new element with a new GUID. What the inserted elements point at among themselves
//! follows them; what they point at that this project has (by GUID, or a level of the same name) they point at here.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::graphics;
use crate::model::{Element, Guid, Id, Model};
use crate::phasing;

/// What an insert made.
#[derive(Debug, Default, PartialEq)]
pub struct Inserted {
    /// Each element brought in: its id in the other project, and its id here.
    pub ids: BTreeMap<Id, Id>,
    /// The elements here that are copies, with a GUID of their own, because this project had theirs.
    pub copies: Vec<Id>,
    /// The types, families, materials, pictures and parameters brought in by name, as `category: name`.
    pub library: Vec<String>,
    /// What was left off an element, and why.
    pub left_out: Vec<String>,
}

/// Elements `ids` of `from` (all of them when none), and what they point at that `to` hasn't, brought into `to`. Unchecked:
/// the engine checks the model after the command.
pub fn insert(to: &mut Model, from: &Model, ids: &[Id]) -> Result<Inserted, String> {
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
    bring_library(to, from, &take, &mut out);
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
        let left = phasing_here(to, &mut e);
        let new = if to.elements.contains_key(id) || id < crate::model::FIRST_ID { to.fresh_id() } else { id };
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

/// The types, families, materials, pictures and project parameters `from` has that `to` hasn't, by name: those the elements
/// `take` need come along. (Types and the rest are named, not numbered, so far: a name this project has is its own.)
fn bring_library(to: &mut Model, from: &Model, take: &BTreeSet<Id>, out: &mut Inserted) {
    fn missing<T: Clone>(to: &mut BTreeMap<String, T>, from: &BTreeMap<String, T>, what: &str, out: &mut Inserted) {
        for (n, v) in from {
            if !to.contains_key(n) {
                to.insert(n.clone(), v.clone());
                out.library.push(format!("{what}: {n}"));
            }
        }
    }
    if take.is_empty() {
        return;
    }
    let t = &mut to.types;
    let f = &from.types;
    missing(&mut t.wall, &f.wall, "wall type", out);
    missing(&mut t.floor, &f.floor, "floor type", out);
    missing(&mut t.ceiling, &f.ceiling, "ceiling type", out);
    missing(&mut t.roof, &f.roof, "roof type", out);
    missing(&mut t.door, &f.door, "door type", out);
    missing(&mut t.window, &f.window, "window type", out);
    missing(&mut t.column, &f.column, "column type", out);
    missing(&mut t.structural_column, &f.structural_column, "structural column type", out);
    missing(&mut t.footing, &f.footing, "footing type", out);
    missing(&mut t.railing, &f.railing, "railing type", out);
    for (cat, types) in &f.runs {
        missing(t.runs.entry(cat.clone()).or_default(), types, &format!("{cat} type"), out);
    }
    missing(&mut to.materials, &from.materials, "material", out);
    missing(&mut to.images, &from.images, "picture", out);
    missing(&mut to.families, &from.families, "family", out);
    missing(&mut to.parameters, &from.parameters, "parameter", out);
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
}
