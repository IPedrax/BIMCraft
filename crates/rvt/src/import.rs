//! A Revit file read into a BIMCraft model, the way the IFC importer reads an IFC file: through the model's own builders and
//! `Model::check`, with a list of what was left out and why.
//!
//! Lengths in a Revit file are feet and BIMCraft's are millimetres (x east, y north, z up, as Revit's project coordinates).
//! The reader only maps what the file's records say. Where a geometry is not decoded the element is left out and listed:
//! BIMCraft's elements are parametric, so an element is made only where a parameter set honestly describes what the file holds.
//! Two passes read the file: the first finds the records (and so the ids of what to read), the second their data.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Element, Id, Level, MAX_TEXT, Model, culprit};

use crate::inventory::{Inventory, Survey, Wants};
use crate::records::{Record, TypeRecord};
use crate::{Limits, Revit};
use crate::{columns, floors, openings, roofs, rooms, walls};

/// `OST_Materials`.
pub(crate) const OST_MATERIALS: i64 = -2_000_700;

/// Feet to millimetres, to the micron.
pub(crate) fn mm(feet: f64) -> f64 {
    micron(feet * 304.8)
}

/// A length in millimetres, to the micron.
pub(crate) fn micron(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// A point in feet, in the model's millimetres.
pub(crate) fn point(x: f64, y: f64) -> P2 {
    P2(mm(x), mm(y))
}

/// `text` cut to what a name may hold.
pub(crate) fn clip(text: &str) -> String {
    text.trim().chars().take(MAX_TEXT).collect()
}

/// `base`, or `base (2)`, `base (3)`… whichever `taken` does not hold yet; then it does.
pub(crate) fn fresh(taken: &mut BTreeSet<String>, base: &str) -> String {
    let base = clip(base);
    let name = (1..).map(|n| if n == 1 { base.clone() } else { format!("{base} ({n})") }).find(|n| !taken.contains(n)).unwrap_or(base);
    taken.insert(name.clone());
    name
}

/// A type of `map` like `same` says, by `base` or `base (2)`, `base (3)`… made as `new` when there is none.
pub(crate) fn kind<T>(map: &mut BTreeMap<String, T>, base: &str, same: impl Fn(&T) -> bool, new: T) -> String {
    let base = clip(base);
    let mut new = Some(new);
    let mut n = 1;
    loop {
        let name = if n == 1 { base.clone() } else { format!("{base} ({n})") };
        match map.get(&name) {
            Some(t) if same(t) => return name,
            Some(_) => n += 1,
            None => {
                if let Some(t) = new.take() {
                    map.insert(name.clone(), t);
                }
                return name;
            }
        }
    }
}

/// What an entry of `Import::from` says (`Wall 423099`, `Structural column 12`, `Level 'Roof'`): its kind in lower case, and the Revit
/// id or, for a level, which has a name and no id, the name as written.
pub(crate) fn origin(from: &str) -> Option<(String, Result<u32, &str>)> {
    if let Some((kind, n)) = from.rsplit_once(' ')
        && let Ok(n) = n.parse::<u32>()
    {
        return Some((kind.to_lowercase(), Ok(n)));
    }
    from.split_once(' ').map(|(kind, what)| (kind.to_lowercase(), Err(what)))
}

/// What the import holds while it reads: the model, what was left out, and what each element was in the file.
pub(crate) struct Import {
    pub model: Model,
    pub skipped: Vec<String>,
    /// Each element made, and what it was in the file (`Wall 423099`), for the notes that say why one was dropped.
    pub from: HashMap<Id, String>,
    /// The model's level for each Revit level.
    pub levels: HashMap<u32, Guid>,
    /// The model's wall for each Revit wall made, and the walls left out for having no layers (curtain walls): hosts of doors.
    pub walls: HashMap<u32, Id>,
    pub curtain: BTreeSet<u32>,
    /// The model's roof for each Revit roof made: the walls whose tops follow one name it.
    pub roofs: HashMap<u32, Id>,
    /// What was left out or noted, by (left out, kind, reason): the ids it concerned. Told as one line each in the end.
    groups: BTreeMap<(bool, String, String), Vec<u32>>,
}

impl Import {
    /// A `kind` (`wall`) of Revit id `id` is left out, for `reason`.
    pub fn left_out(&mut self, kind: &str, id: u32, reason: &str) {
        self.groups.entry((true, kind.to_string(), reason.to_string())).or_default().push(id);
    }

    /// Something to say about a `kind` (`wall`) that was made nonetheless.
    pub fn note(&mut self, kind: &str, id: u32, text: &str) {
        self.groups.entry((false, kind.to_string(), text.to_string())).or_default().push(id);
    }

    /// Element `id` of the model was made and then taken out again, for `why`: left out, as a Revit element of its kind (`Wall 423099`
    /// was made, so it is `wall`, 423099), or for a level, which has a name and no id, a note of its own.
    fn dropped(&mut self, id: Id, why: &str) {
        let from = self.from.get(&id).cloned().unwrap_or_default();
        match origin(&from) {
            Some((kind, Ok(n))) => self.left_out(&kind, n, why),
            Some((kind, Err(what))) => self.skipped.push(format!("1 {kind} left out, {why} ({what})")),
            None => self.skipped.push(format!("1 element left out, {why}")),
        }
    }

    fn tell(&mut self) {
        for ((left, kind, reason), mut ids) in std::mem::take(&mut self.groups) {
            ids.sort_unstable();
            ids.dedup();
            let list: Vec<String> = ids.iter().take(8).map(u32::to_string).collect();
            let more = if ids.len() > list.len() { ", …" } else { "" };
            let n = ids.len();
            let plural = if n == 1 { "" } else { "s" };
            self.skipped.push(if left {
                format!("{n} {kind}{plural} left out, {reason} (ids {}{more})", list.join(", "))
            } else {
                format!("{n} {kind}{plural}: {reason} (ids {}{more})", list.join(", "))
            });
        }
    }
}

/// What the first pass found, arranged for the readers of each kind of element.
pub(crate) struct Facts {
    pub release: u32,
    pub tags: crate::records::Tags,
    pub ids: Vec<u32>,
    /// The current copy of each placed element of the categories asked for.
    pub current: BTreeMap<u32, Record>,
    /// The ids of the types of each category.
    pub types: BTreeMap<i64, BTreeSet<u32>>,
    /// The ids of the materials.
    pub materials: BTreeSet<u32>,
    /// The ids of the family symbols (types of components, doors and windows) of each category, which have a box but are not placed.
    pub symbols: BTreeMap<i64, BTreeSet<u32>>,
    /// The box of each family symbol (its envelope in the family's own axes), by id, as its newest record has it.
    pub symbol_boxes: BTreeMap<u32, [f64; 6]>,
    /// Each project level's elevation in feet, by Revit id.
    pub elevations: BTreeMap<u32, f64>,
}

impl Facts {
    fn of(s: Survey) -> (Facts, Inventory) {
        let mut current: BTreeMap<u32, Record> = BTreeMap::new();
        let mut symbols: BTreeMap<i64, BTreeSet<u32>> = BTreeMap::new();
        let mut symbol_boxes: BTreeMap<u32, ((u32, usize), [f64; 6])> = BTreeMap::new();
        for r in s.records.iter().filter(|r| !r.placed) {
            symbols.entry(r.category).or_default().insert(r.id);
            if symbol_boxes.get(&r.id).is_none_or(|(held, _)| *held < (r.partition, r.offset)) {
                symbol_boxes.insert(r.id, ((r.partition, r.offset), r.bbox));
            }
        }
        let symbol_boxes = symbol_boxes.into_iter().map(|(id, (_, b))| (id, b)).collect();
        for r in s.records.into_iter().filter(|r| r.placed && (has_volume(r) || r.category == floors::OST_SKETCH_LINES)) {
            // The copy with the greatest height is the element's whole body; a tie goes to the newest.
            let key = |r: &Record| (r.extent()[2].to_bits(), r.partition, r.offset);
            if current.get(&r.id).is_none_or(|held| key(held) < key(&r)) {
                current.insert(r.id, r);
            }
        }
        let mut types: BTreeMap<i64, BTreeSet<u32>> = BTreeMap::new();
        let mut materials = BTreeSet::new();
        for TypeRecord { id, category, kind, .. } in s.types {
            if category == OST_MATERIALS {
                materials.insert(id);
            } else if kind == crate::records::KIND_TYPE {
                types.entry(category).or_default().insert(id);
            }
        }
        let elevations = s.inventory.levels.iter().map(|l| (l.id, l.elevation_feet)).collect();
        (Facts { release: s.release, tags: s.tags, ids: s.ids, current, types, materials, symbols, symbol_boxes, elevations }, s.inventory)
    }

    /// The current records of a category.
    pub fn of_category(&self, category: i64) -> impl Iterator<Item = &Record> {
        self.current.values().filter(move |r| r.category == category)
    }
}

/// A placed instance with no volume is a 2D symbol (a drain drawn in plan): it has no body to import.
fn has_volume(r: &Record) -> bool {
    r.extent().iter().all(|e| e.is_finite() && *e > 1e-6)
}

/// The categories whose records the first pass keeps.
const CATEGORIES: [i64; 10] = [
    walls::OST_WALLS,
    floors::OST_FLOORS,
    floors::OST_CEILINGS,
    floors::OST_SKETCH_LINES,
    openings::OST_DOORS,
    openings::OST_WINDOWS,
    columns::OST_COLUMNS,
    columns::OST_STRUCTURAL_COLUMNS,
    rooms::OST_ROOMS,
    roofs::OST_ROOFS,
];

/// Reads the Revit project in `bytes` into a model named after its title (or `name`).
pub fn import(bytes: &[u8], name: &str) -> Result<Imported, String> {
    let mut revit = Revit::from_bytes(bytes.to_vec(), Limits::default()).map_err(|e| e.to_string())?;
    build(&mut revit, name).map(|(i, _)| i)
}

/// Reads the Revit project at `path`, from disk (a large file is not held in memory twice).
pub fn import_path(path: &Path, name: &str) -> Result<Imported, String> {
    let mut revit = Revit::open(path, Limits::default()).map_err(|e| e.to_string())?;
    build(&mut revit, name).map(|(i, _)| i)
}

/// [`import_path`], and what each element made was in the file (`Wall 423099`): for checking an import against another
/// reader's export of the same file.
#[doc(hidden)]
pub fn import_path_with_origins(path: &Path, name: &str) -> Result<(Imported, HashMap<Id, String>), String> {
    let mut revit = Revit::open(path, Limits::default()).map_err(|e| e.to_string())?;
    build(&mut revit, name)
}

/// The import, and what each element made was in the file.
pub(crate) fn build(revit: &mut Revit, name: &str) -> Result<(Imported, HashMap<Id, String>), String> {
    let e = |e: crate::Error| e.to_string();
    let survey = revit.survey(&|c| CATEGORIES.contains(&c)).map_err(e)?;
    let title = revit.document_atom().ok().flatten().and_then(|a| a.title).filter(|t| !t.trim().is_empty());
    let mut m = Model::new(&clip(title.as_deref().unwrap_or(name)));
    m.elements.clear();
    m.next_id = 1;
    let mut cx = Import {
        model: m,
        skipped: Vec::new(),
        from: HashMap::new(),
        levels: HashMap::new(),
        walls: HashMap::new(),
        curtain: BTreeSet::new(),
        roofs: HashMap::new(),
        groups: BTreeMap::new(),
    };
    let release = survey.release;
    let (facts, inv) = Facts::of(survey);
    levels(&mut cx, &inv);
    let mut wants = Wants { materials: facts.materials.clone(), ..Wants::default() };
    walls::wanted(&facts, &mut wants.data);
    floors::wanted(&facts, &mut wants.data);
    roofs::wanted(&facts, &mut wants.data);
    openings::wanted(&facts, &mut wants.data);
    columns::wanted(&facts, &mut wants);
    rooms::wanted(&facts, &mut wants);
    let gathered = revit.gather(release, &facts.tags, &facts.ids, &wants).map_err(e)?;
    walls::import(&mut cx, &facts, &gathered);
    floors::import(&mut cx, &facts, &gathered);
    roofs::import(&mut cx, &facts, &gathered);
    attached(&mut cx, &facts);
    openings::import(&mut cx, &facts, &gathered);
    columns::import(&mut cx, &facts, &gathered);
    rooms::import(&mut cx, &facts, &gathered);
    let from = cx.from.clone();
    finish(cx, format!("Revit {release}")).map(|i| (i, from))
}

/// Walls whose tops follow a roof: a wall's record names the roof it is attached to with the levels it is constrained to, so its top
/// is cut by the roof's underside instead of running up to the top of its own box, which a wall under a ridge does.
fn attached(cx: &mut Import, facts: &Facts) {
    if cx.roofs.is_empty() {
        return;
    }
    for r in facts.of_category(crate::walls::OST_WALLS) {
        let Some(id) = cx.walls.get(&r.id).copied() else { continue };
        let roof = r.refs.iter().filter_map(|v| u32::try_from(*v).ok()).find_map(|n| cx.roofs.get(&n).copied());
        let Some(roof) = roof else { continue };
        let guid = cx.model.guid(roof);
        if let Some(bimcraft_core::model::Element::Wall(w)) = cx.model.elements.get_mut(id) {
            w.attach_top = Some(guid);
            cx.note("wall", r.id, "its top follows the roof it names: it comes in cut by the roof's underside");
        }
    }
}

/// The project's levels, lowest first. A level of a name another has takes `(2)`; none at all gives the project one at zero.
fn levels(cx: &mut Import, inv: &Inventory) {
    let mut taken = BTreeSet::new();
    for l in &inv.levels {
        let name = fresh(&mut taken, &l.name);
        let id = cx.model.insert(Element::Level(Level::new(name, mm(l.elevation_feet))));
        cx.from.insert(id, format!("Level '{}'", l.name));
        cx.levels.insert(l.id, cx.model.guid(id));
    }
    if inv.levels.is_empty() {
        let name = fresh(&mut taken, "Level 1");
        let id = cx.model.insert(Element::Level(Level::new(name, 0.0)));
        cx.from.insert(id, "Level 'Level 1'".into());
        cx.skipped.push("the file has no levels the reader can find: the project has one, Level 1, at zero".into());
    }
    // A level inside another element (a group's, a family's) is no project level: the project has the levels it shows, and these
    // are not among them. They are named in the report, not left out of it.
    if !inv.owned_levels.is_empty() {
        let ids: Vec<String> = inv.owned_levels.iter().map(u32::to_string).collect();
        let n = ids.len();
        let plural = if n == 1 { "" } else { "s" };
        cx.skipped.push(format!(
            "{n} level{plural}: it sits inside another element (a group's or a family's), so it is no project level (ids {})",
            ids.join(", ")
        ));
    }
    if inv.unread_levels > 0 {
        let n = inv.unread_levels;
        let plural = if n == 1 { "" } else { "s" };
        cx.skipped.push(format!("{n} level{plural} left out, no name or elevation the reader can find (its block is missing or damaged)"));
    }
}

/// What breaks a rule is left out with the rule (a wall takes its doors and windows along), then the model is checked once more.
fn finish(mut cx: Import, schema: String) -> Result<Imported, String> {
    cx.model.adopt_materials();
    // What the import put in the library (types, materials) and its parts get their ids and GUIDs, as `Model::check` asks.
    cx.model.sync_library();
    let m = &mut cx.model;
    let mut dropped: Vec<(Id, String)> = Vec::new();
    let mut checked = false;
    for _ in 0..=m.elements.len() {
        let Err(error) = m.check() else {
            checked = true;
            break;
        };
        let id = culprit(&error).filter(|id| cx.from.contains_key(id)).ok_or_else(|| error.clone())?;
        let why = error.split_once(": ").map_or(error.as_str(), |(_, why)| why);
        dropped.push((id, why.to_string()));
        let gone = m.guid(id);
        m.elements.remove(id);
        let hosted: Vec<Id> = m
            .elements
            .iter()
            .filter_map(|(k, x)| match x {
                Element::Door(o) | Element::Window(o) if o.host == gone => Some(*k),
                _ => None,
            })
            .collect();
        for h in hosted {
            dropped.push((h, "its wall was left out".to_string()));
            m.elements.remove(h);
        }
    }
    if !checked {
        m.check()?;
    }
    for (id, why) in dropped {
        cx.dropped(id, &why);
    }
    cx.tell();
    Ok(Imported { model: cx.model, schema, skipped: cx.skipped, moved: None })
}
