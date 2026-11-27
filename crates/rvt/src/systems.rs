//! MEP systems: the system each duct, pipe, flexible run, fitting and accessory is in, and the colour its system's type gives it.
//!
//! Revit keeps a system type (`RbsHvacSystemType`, `RbsPipingSystemType`: Supply Air, Domestic Hot Water) and each system of it
//! (`RbsHvacSystem`, `RbsPipingSystem`: Mechanical Supply Air 1) as an element's own object: its id as a `u64` sixteen bytes before
//! the tag of its class, then the object, read by the file's own schema ([`crate::graphics::read_as`]) and taken only where its
//! `m_id` is that id. A type gives its name (`m_symbolInfo`'s `m_name`), the material it names (`m_renderStyleId`, the Material
//! of its properties) and its graphic overrides (`m_oSystemOverrides`, an `OverrideGraphicSettings` whose `m_oProjPenColor` is the
//! colour of its lines, `0xBBGGRR`, none where the 25th bit is set); a system gives its name (`m_strName`) and its type
//! (`m_typeId`).
//!
//! An element names the systems it is in in its own object (`RbsDuctCurve`, `RbsPipeCurve`, the flexible ones, `FamilyInstance`):
//! its analytical cell's `m_systemId` and each of its connectors' `m_idSystem`. A fitting's object holds data its schema does not
//! describe whole further on (its analytical data is written four bytes longer than its fields), so what reads of it before that is
//! taken ([`crate::graphics::read_some`]), and a value only where it is the id of a system the file holds.
//!
//! An element in systems of one type is drawn in that type's material, or, where it names none, in a material named for the type
//! in the colour its graphic overrides give lines (Revit's colour for the system: the samples' system types name no material): a
//! run's shape and the forms of a component's family that have no material of their own, through the family's material parameter
//! `System Material`. A run takes its system's name. One in systems of two types (a fitting between supply and return) takes none.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bimcraft_core::family::{Kind, Parameter};
use bimcraft_core::model::{Element, Id};

use crate::graphics::{Layouts, Obj, Tree, Value};
use crate::import::{Facts, Import, clip, origin};
use crate::inventory::Gathered;

/// The categories whose elements are in systems: ducts, duct fittings, duct accessories, flexible ducts, pipes, pipe fittings,
/// flexible pipes and pipe accessories.
const CATEGORIES: [i64; 8] = [-2_008_000, -2_008_010, -2_008_016, -2_008_020, -2_008_044, -2_008_049, -2_008_050, -2_008_055];

/// The classes of system types, of systems and of the elements in them.
const TYPES: [&str; 2] = ["RbsHvacSystemType", "RbsPipingSystemType"];
const SYSTEMS: [&str; 2] = ["RbsHvacSystem", "RbsPipingSystem"];
const ELEMENTS: [&str; 5] = ["FamilyInstance", "RbsDuctCurve", "RbsPipeCurve", "RbsFlexDuctCurve", "RbsFlexPipeCurve"];

/// The family material parameter a component's forms with no material of their own name, set to its system's material.
pub(crate) const MATERIAL: &str = "System Material";

/// The most systems one element may name.
const MAX_NAMED: usize = 64;

/// A system type: its name, the material it names, and the colour its graphic overrides give lines.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SystemType {
    pub name: String,
    pub material: Option<u32>,
    pub colour: Option<[u8; 3]>,
}

/// A system: its name and its type.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct System {
    pub name: String,
    pub type_id: u32,
}

/// What the partitions hold of the systems: the types and systems by id, and the systems each wanted element names.
#[derive(Clone, Debug, Default)]
pub(crate) struct Systems {
    pub types: BTreeMap<u32, SystemType>,
    pub systems: BTreeMap<u32, System>,
    pub named: BTreeMap<u32, BTreeSet<u32>>,
}

/// [`Systems`] as the partitions are read: each one's newest copy, by the partition it is in.
#[derive(Default)]
pub(crate) struct Scan {
    types: BTreeMap<u32, (u32, SystemType)>,
    systems: BTreeMap<u32, (u32, System)>,
    named: BTreeMap<u32, (u32, BTreeSet<u32>)>,
}

/// The elements whose systems are read.
pub(crate) fn wanted(f: &Facts, out: &mut BTreeSet<u32>) {
    for category in CATEGORIES {
        out.extend(f.of_category(category).map(|r| r.id));
    }
}

/// Keeps `value` for `id` where `number` is its newest partition yet.
fn offer<T>(map: &mut BTreeMap<u32, (u32, T)>, id: u32, number: u32, value: T) {
    if map.get(&id).is_none_or(|(held, _)| *held <= number) {
        map.insert(id, (number, value));
    }
}

impl Scan {
    /// Reads partition `number`'s bytes `buf`: its system types and systems, and what the elements `wanted` name.
    pub(crate) fn partition(&mut self, buf: &[u8], number: u32, layouts: &Layouts, wanted: &BTreeSet<u32>) {
        if wanted.is_empty() {
            return;
        }
        for (class, tag) in TYPES.iter().chain(&SYSTEMS).chain(&ELEMENTS).filter_map(|c| Some((*c, layouts.tag(c)?))) {
            for p in memchr::memmem::find_iter(buf, &tag.to_le_bytes()) {
                let Some(id) = p.checked_sub(16).and_then(|a| buf.get(a..a + 8)).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes)
                else {
                    continue;
                };
                let Ok(id) = u32::try_from(id) else { continue };
                let element = ELEMENTS.contains(&class);
                if element && !wanted.contains(&id) || id == 0 || id == u32::MAX {
                    continue;
                }
                let (tree, stopped) = crate::graphics::read_some(buf, p + 2, layouts, tag);
                let Some(root) = tree.root() else { continue };
                if layouts.get(root, "m_id").and_then(crate::graphics::element_id) != Some(u64::from(id)) {
                    continue;
                }
                if element {
                    offer(&mut self.named, id, number, named(&tree, layouts));
                } else if stopped.is_some() {
                    // A type or a system is read whole, or not at all.
                } else if TYPES.contains(&class) {
                    if let Some(t) = system_type(&tree, root, layouts) {
                        offer(&mut self.types, id, number, t);
                    }
                } else if let Some(s) = system(root, layouts) {
                    offer(&mut self.systems, id, number, s);
                }
            }
        }
    }

    pub(crate) fn done(self) -> Systems {
        Systems {
            types: self.types.into_iter().map(|(id, (_, t))| (id, t)).collect(),
            systems: self.systems.into_iter().map(|(id, (_, s))| (id, s)).collect(),
            named: self.named.into_iter().map(|(id, (_, n))| (id, n)).collect(),
        }
    }
}

/// The object an object's field `field` owns, inline or in its turn.
fn owned<'t>(tree: &'t Tree, o: &'t Obj, layouts: &Layouts, field: &str) -> Option<&'t Obj> {
    match layouts.get(o, field)? {
        Value::Inline(k) => o.inline.get(*k),
        Value::Owned(i, _) => tree.get(*i),
        _ => None,
    }
}

/// A text value: its UTF-16 units, clipped.
fn text(v: &Value) -> Option<String> {
    let units: Vec<u16> =
        v.list()?.iter().map(|u| if let Value::Int(u) = u { u16::try_from(*u).ok() } else { None }).collect::<Option<_>>()?;
    let s = String::from_utf16(&units).ok()?;
    let s = clip(s.trim());
    (!s.is_empty()).then_some(s)
}

fn id_of(v: Option<&Value>) -> Option<u32> {
    v.and_then(crate::graphics::element_id).and_then(|id| u32::try_from(id).ok()).filter(|id| *id != u32::MAX && *id != 0)
}

fn system_type(tree: &Tree, root: &Obj, layouts: &Layouts) -> Option<SystemType> {
    let name = owned(tree, root, layouts, "m_symbolInfo").and_then(|i| text(layouts.get(i, "m_name")?))?;
    let material = id_of(layouts.get(root, "m_renderStyleId"));
    let pen = owned(tree, root, layouts, "m_oSystemOverrides")
        .and_then(|o| owned(tree, o, layouts, "m_oOverrideGraphicSettings"))
        .and_then(|o| owned(tree, o, layouts, "m_oProjPenColor"));
    let colour = match pen.and_then(|c| layouts.get(c, "m_color")) {
        Some(Value::Int(c)) if *c < 1 << 24 => u32::try_from(*c).ok().map(|c| {
            let [r, g, b, _] = c.to_le_bytes();
            [r, g, b]
        }),
        _ => None,
    };
    Some(SystemType { name, material, colour })
}

fn system(root: &Obj, layouts: &Layouts) -> Option<System> {
    Some(System { name: text(layouts.get(root, "m_strName")?)?, type_id: id_of(layouts.get(root, "m_typeId"))? })
}

/// The ids an element's analytical cell and connectors name as its systems.
fn named(tree: &Tree, layouts: &Layouts) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    for o in crate::graphics::every(tree) {
        for field in ["m_systemId", "m_idSystem"] {
            if let Some(id) = id_of(layouts.get(o, field)) {
                out.insert(id);
            }
        }
        if out.len() >= MAX_NAMED {
            break;
        }
    }
    out
}

/// What an element in systems is drawn in: its system type's material, or one named for the type in its colour.
enum Drawn {
    In(String),
    /// Its system type gives no colour.
    Plain,
}

/// Draws each element made of a Revit element in systems in its system type's colour, and gives a run its system's name; notes
/// what was not read.
pub(crate) fn colour(cx: &mut Import, f: &Facts, g: &Gathered) {
    let s = &g.systems;
    let mut made: HashMap<u32, (String, Vec<Id>)> = HashMap::new();
    for (id, from) in &cx.from {
        if let Some((kind, Ok(n))) = origin(from) {
            made.entry(n).or_insert_with(|| (kind, Vec::new())).1.push(*id);
        }
    }
    let file_materials: BTreeSet<&str> = g.materials.values().map(String::as_str).collect();
    for r in CATEGORIES.iter().flat_map(|c| f.of_category(*c)) {
        let Some((kind, ids)) = made.get(&r.id) else { continue };
        let systems: Vec<&System> = s.named.get(&r.id).into_iter().flatten().filter_map(|id| s.systems.get(id)).collect();
        let types: BTreeSet<u32> = systems.iter().map(|s| s.type_id).collect();
        let drawn = match (types.len(), types.first().and_then(|t| s.types.get(t))) {
            (0, _) => Err("its system is not read".to_string()),
            (1, None) => Err("its system's type is not read".to_string()),
            (1, Some(t)) => match (t.material.and_then(|m| g.materials.get(&m)), t.colour) {
                (Some(m), _) => Ok(Drawn::In(m.clone())),
                (None, Some(colour)) => {
                    if !file_materials.contains(t.name.as_str()) {
                        cx.looks.insert(t.name.clone(), crate::data::Look { color: colour, transparency: 0.0 });
                    }
                    Ok(Drawn::In(t.name.clone()))
                }
                (None, None) => Ok(Drawn::Plain),
            },
            (n, _) => Err(format!("it joins systems of {n} types, so it takes the colour of none")),
        };
        let system = match systems.as_slice() {
            [one] => Some(one.name.as_str()),
            _ => None,
        };
        for id in ids {
            apply(cx, *id, if let Ok(Drawn::In(m)) = &drawn { Some(m) } else { None }, system);
        }
        let note = match &drawn {
            Ok(Drawn::In(_)) => "it is drawn in the colour its system type's graphic overrides give its lines",
            Ok(Drawn::Plain) => "its system type gives no colour",
            Err(why) => why,
        };
        cx.note(kind, r.id, note);
    }
}

/// Gives element `id` material `material` (where it has a shape or forms of no material of their own) and its system's name.
fn apply(cx: &mut Import, id: Id, material: Option<&str>, system: Option<&str>) {
    let Some(e) = cx.model.elements.get_mut(id) else { return };
    if let Some(run) = e.run_mut() {
        if let Some(name) = system {
            run.system = name.to_string();
        }
        if let (Some(m), Some(shape)) = (material, run.imported.as_mut()) {
            for part in shape.parts.iter_mut().filter(|p| p.material.is_empty()) {
                part.material = m.to_string();
            }
        }
        return;
    }
    let (Some(m), Element::Component(c)) = (material, e) else { return };
    let Some(family) = cx.model.families.get_mut(&c.family) else { return };
    if !family.forms.iter().any(|f| f.material.is_empty() || f.material == MATERIAL) {
        return;
    }
    if !family.parameters.iter().any(|p| p.name == MATERIAL) {
        family.parameters.push(Parameter {
            name: MATERIAL.to_string(),
            kind: Kind::Material,
            instance: true,
            value: "".into(),
            ..Parameter::default()
        });
    }
    for form in family.forms.iter_mut().filter(|f| f.material.is_empty()) {
        form.material = MATERIAL.to_string();
    }
    c.params.insert(MATERIAL.to_string(), m.into());
}
