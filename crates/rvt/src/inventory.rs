// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/partition_element_records.rs (the
// 64-bit element record, its fixed offsets, the record chain and the second-prologue rule, the BuiltInCategory names),
// src/partition_element_records_2023.rs (the 32-bit form, 2015 to 2023) and src/partition_level_records.rs (Level records and
// their name and elevation blocks).
// Changed: rewritten for BIMCraft; the record marker always comes from the file's own schema, every offset is bounds-checked,
// only the id, category, class and whether the record is a placed instance are kept (no bounding boxes, no reference
// lists), the walk over a partition is one function per record form, and a Level's Building Story setting and the Levels that
// sit inside other elements (RE-51, RE-118) are read from the same blocks.
//! The element inventory: which elements a project holds, by `BuiltInCategory`, and its levels.
//!
//! A partition (`Partitions/NN`) holds one record per element, each opening with a header (the ElementId, the record's size,
//! a constant, a count) and closing with a 16-byte trailer that repeats the size. In 2024 and later a record's
//! frame has fixed offsets from its start: the `BuiltInCategory` (a negative `i64`, Autodesk's published API constant) at
//! `+0x12`, the container reference at `+0x32`, the placement kind at `+0x42`, the class tag at `+0x4a`, a marker at `+0x50`
//! and the bounding box after it. The marker is the schema's tags for `Outline` and `ElementParents` with `ff` ×4 between.
//! Some frames have no ElementId of their own (the second prologue): they sit inside another record's body and take that
//! record's id. Up to 2023 the ElementId is a `u32` 52 bytes before the marker and the category sits 38 bytes before it.
//!
//! A *placed instance* is a record with no container and the placed kind: a family's types and a group's members are
//! records too, and counting only placed instances is what matches the elements Revit's own IFC export writes.
//!
//! A Level has no bounding box, so no `Outline` marker: it is found by its category, and its name and elevation are in a
//! separate block keyed by its ElementId (a run of `ff`, the name as counted UTF-16, then an elevation in feet that appears
//! twice). Past the name the block holds Revit's Building Story setting, which says whether the level is a storey. A Level that
//! sits inside another element has a block whose run of `ff` is broken by that element's id; it is not a project level.

use std::collections::{BTreeMap, BTreeSet};

use crate::data::{Data, category_materials, column_bottoms, column_nodes, element_data, material_names, newest, room_entries};
use crate::error::{Error, Result};
use crate::names::{Latest, entries_32, entries_64};
use crate::records::{
    CATEGORY_AT, CONTAINER_AT, KIND_AT, KIND_PLACED, MARKER_AT, NO_CONTAINER, Record, Tags, TypeRecord, chain, declared, enclosing, f64_at,
    id_before, no_id, records_32, records_64, types_32, types_64, u16_at, u32_at, u64_at,
};
use crate::types::first_name;

/// `OST_Levels`.
pub const OST_LEVELS: i64 = -2_000_240;

/// The kind Revit 2015 to 2018 write on a Level.
const LEVEL_KIND_2015: u32 = 0xffff_ef6f;
/// Level name blocks.
const RUN_64: usize = 56;
const RUN_32: usize = 28;
const MAX_NAME_CHARS: usize = 128;
const ELEVATION_SEARCH: usize = 2048;
const ELEVATION_AFTER_MARKER: usize = 55;
const CONFIRM_LEAD_IN: usize = 24;
const MAX_ELEVATION_FEET: f64 = 1.0e7;
/// Where a level's Building Story setting is looked for past its name, and what frames it: an unset id, `00 01`, a number and
/// the level's own id (`u64` from 2024, `u32` before), then the setting.
const STORY_WINDOW: usize = 0x200;
/// Where the level's name starts from the `u64` ElementId of its block, and the `01 00 00 00` that opens an object keyed by one.
const NAME_AFTER_OWNER: usize = 0x47;
const OBJECT_PREFIX: [u8; 4] = [1, 0, 0, 0];

/// The releases the record forms are known for (rvt-rs's), of which 2023 to 2026 are checked here on sample files.
pub const RELEASES: std::ops::RangeInclusive<u32> = 2015..=2027;

/// One element: its ElementId, its `BuiltInCategory`, its class (the tag in the file's schema) and whether it is a placed
/// instance. An element seen in several records is one element, placed if any of its records is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Element {
    pub category: i64,
    pub id: u32,
    pub class_tag: u16,
    pub placed: bool,
}

/// A level, with the elevation as stored (feet).
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub id: u32,
    pub name: String,
    pub elevation_feet: f64,
    /// Revit's Building Story setting, where the block holds it: only a building story is a storey in Revit's own IFC export.
    pub building_story: Option<bool>,
}

impl Level {
    /// The elevation in millimetres (1 ft is 304.8 mm exactly).
    pub fn elevation_mm(&self) -> f64 {
        self.elevation_feet * 304.8
    }
}

/// How many elements a category has, and how many of them are placed instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Count {
    pub category: i64,
    pub elements: usize,
    pub placed: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inventory {
    /// ElementIds the element table declares.
    pub declared: usize,
    /// Element records read, before elements seen twice are joined.
    pub records: usize,
    /// One per element and category, ordered by category then id. Levels are here too, under `OST_Levels`.
    pub elements: Vec<Element>,
    /// The levels whose name and elevation could be read, by elevation.
    pub levels: Vec<Level>,
    /// Level elements (placed instances) with no readable name block, not counting those inside other elements.
    pub unread_levels: usize,
    /// Level elements whose block names another element that holds them: not project levels, and not in `levels`.
    pub owned_levels: Vec<u32>,
}

impl Inventory {
    /// Elements per category, ordered by category id.
    pub fn counts(&self) -> Vec<Count> {
        let mut by: BTreeMap<i64, (usize, usize)> = BTreeMap::new();
        for e in &self.elements {
            let c = by.entry(e.category).or_default();
            c.0 += 1;
            c.1 += usize::from(e.placed);
        }
        by.into_iter().map(|(category, (elements, placed))| Count { category, elements, placed }).collect()
    }
}

/// The name of a `BuiltInCategory`, where it is known: the ids are Autodesk's published API constants and the names the ones
/// Revit gives the category (rvt-rs measured them in Revit's own IFC export of sample files).
pub fn category_name(id: i64) -> Option<&'static str> {
    NAMES.binary_search_by_key(&id, |&(known, _)| known).ok().map(|i| NAMES[i].1)
}

/// Sorted by id.
const NAMES: &[(i64, &str)] = &[
    (-2_009_030, "Structural Connections"),
    (-2_009_000, "Structural Rebar"),
    (-2_008_132, "Conduits"),
    (-2_008_130, "Cable Trays"),
    (-2_008_128, "Conduit Fittings"),
    (-2_008_126, "Cable Tray Fittings"),
    (-2_008_099, "Sprinklers"),
    (-2_008_087, "Lighting Devices"),
    (-2_008_049, "Pipe Fittings"),
    (-2_008_044, "Pipes"),
    (-2_008_020, "Flex Ducts"),
    (-2_008_013, "Air Terminals"),
    (-2_008_010, "Duct Fittings"),
    (-2_008_000, "Ducts"),
    (-2_006_114, "Design Options"),
    (-2_006_112, "Design Option Sets"),
    (-2_003_400, "Mass"),
    (-2_003_200, "Areas"),
    (-2_001_392, "Floors: Slab Edges"),
    (-2_001_370, "Entourage"),
    (-2_001_360, "Planting"),
    (-2_001_354, "Structural Stiffeners"),
    (-2_001_350, "Specialty Equipment"),
    (-2_001_336, "Structural Trusses"),
    (-2_001_330, "Structural Columns"),
    (-2_001_327, "Structural Beam Systems"),
    (-2_001_320, "Structural Framing"),
    (-2_001_300, "Structural Foundations"),
    (-2_001_263, "Site: Pads"),
    (-2_001_180, "Parking"),
    (-2_001_160, "Plumbing Fixtures"),
    (-2_001_140, "Mechanical Equipment"),
    (-2_001_120, "Lighting Fixtures"),
    (-2_001_100, "Furniture Systems"),
    (-2_001_060, "Electrical Fixtures"),
    (-2_001_040, "Electrical Equipment"),
    (-2_001_000, "Casework"),
    (-2_000_997, "Walls: Rectangular Straight Wall Opening"),
    (-2_000_996, "Shaft Openings"),
    (-2_000_920, "Stairs: Landings"),
    (-2_000_919, "Stairs: Runs"),
    (-2_000_267, "Assemblies"),
    (-2_000_240, "Levels"),
    (-2_000_182, "Walls: Reveals"),
    (-2_000_181, "Walls: Wall Sweeps"),
    (-2_000_171, "Curtain Wall Mullions"),
    (-2_000_170, "Curtain Panels"),
    (-2_000_160, "Rooms"),
    (-2_000_151, "Generic Models"),
    (-2_000_126, "Railings"),
    (-2_000_123, "Stairs: Supports"),
    (-2_000_120, "Stairs"),
    (-2_000_112, "Phases"),
    (-2_000_100, "Columns"),
    (-2_000_080, "Furniture"),
    (-2_000_051, "Lines"),
    (-2_000_045, "Lines: <Sketch>"),
    (-2_000_038, "Ceilings"),
    (-2_000_035, "Roofs"),
    (-2_000_032, "Floors"),
    (-2_000_023, "Doors"),
    (-2_000_014, "Windows"),
    (-2_000_011, "Walls"),
];

/// What a scan of one partition finds.
#[derive(Default)]
struct Found {
    records: usize,
    elements: BTreeMap<(i64, u32), (u16, bool)>,
    level_records: BTreeMap<u32, bool>,
    blocks: Vec<Block>,
    /// Declared ids with the block of a level inside another element.
    owned: BTreeSet<u32>,
}

struct Block {
    id: u32,
    name: String,
    elevation_feet: f64,
    building_story: Option<bool>,
}

impl Found {
    fn element(&mut self, category: i64, id: u32, class_tag: u16, placed: bool) {
        self.records += 1;
        let e = self.elements.entry((category, id)).or_insert((class_tag, false));
        e.1 |= placed;
    }

    fn level(&mut self, id: u32, placed: bool) {
        *self.level_records.entry(id).or_insert(false) |= placed;
    }
}

/// Level records of a 2024 and later partition: found by their category, with the marker's last six bytes at `+0x50`.
fn levels_64(buf: &[u8], tags: &Tags, release: u32, ids: &[u32], found: &mut Found) {
    let marker = tags.marker();
    let spans = chain(buf, &marker);
    for hit in memchr::memmem::find_iter(buf, &OST_LEVELS.to_le_bytes()) {
        let Some(f) = hit.checked_sub(CATEGORY_AT) else { continue };
        if f + MARKER_AT + 8 > buf.len() || u16_at(buf, f + 0x10) != Some(0) || buf[f + MARKER_AT..f + MARKER_AT + 6] != marker[2..] {
            continue;
        }
        let raw = u64_at(buf, f).unwrap_or(0);
        let id = if no_id(raw) { enclosing(&spans, f).and_then(|s| u32::try_from(s.id).ok()) } else { Some(raw as u32) };
        let Some(id) = id.filter(|&id| declared(ids, id)) else { continue };
        let container = u64_at(buf, f + CONTAINER_AT).unwrap_or(0);
        let kind = u64_at(buf, f + KIND_AT).map_or(0, |v| v as u32);
        found.level(id, container == NO_CONTAINER && is_placed_level(kind, release));
    }
}

/// Level records of a partition up to 2023: the id 14 bytes before the category (and 14 more for each item of a workshared file's
/// list), the marker 38 after it.
fn levels_32(buf: &[u8], tags: &Tags, release: u32, ids: &[u32], found: &mut Found) {
    let marker = tags.marker();
    for hit in memchr::memmem::find_iter(buf, &OST_LEVELS.to_le_bytes()) {
        let Some((id, _)) = id_before(buf, hit, tags.header, ids) else { continue };
        if buf.get(hit + 38..hit + 44) != Some(&marker[2..]) {
            continue;
        }
        let container = u32_at(buf, hit + 16);
        let kind = u32_at(buf, hit + 24).unwrap_or(0);
        found.level(id, container == Some(u32::MAX) && is_placed_level(kind, release));
    }
}

fn is_placed_level(kind: u32, release: u32) -> bool {
    kind == KIND_PLACED || (release < 2019 && kind == LEVEL_KIND_2015)
}

/// Name and elevation blocks: an owner id, a run of `ff`, three bytes of padding, the name's length and its UTF-16, then the
/// elevation marker and, [`ELEVATION_AFTER_MARKER`] bytes on, the elevation in feet, which appears again with the 24 bytes
/// before it.
fn blocks(buf: &[u8], tags: &Tags, ids: &[u32], wide_ids: bool, found: &mut Found) {
    let (id_len, run_len) = if wide_ids { (8, RUN_64) } else { (4, RUN_32) };
    let run = vec![0xffu8; run_len];
    let finder = memchr::memmem::Finder::new(&run);
    let mut index = 0usize;
    while let Some(at) = finder.find(&buf[index..]) {
        let mut start = index + at;
        while start > index && buf[start - 1] == 0xff {
            start -= 1;
        }
        index = start;
        while index < buf.len() && buf[index] == 0xff {
            index += 1;
        }
        if let Some(block) = block_at(buf, tags, ids, start, id_len, run_len) {
            found.blocks.push(block);
        }
    }
}

fn block_at(buf: &[u8], tags: &Tags, ids: &[u32], start: usize, id_len: usize, run_len: usize) -> Option<Block> {
    let owner_at = start.checked_sub(id_len)?;
    let owner = if id_len == 4 { u64::from(u32_at(buf, owner_at)?) } else { u64_at(buf, owner_at)? };
    let id = u32::try_from(owner).ok().filter(|&id| declared(ids, id))?;
    // The run ends here, and three bytes of padding follow it (the first 0 or 1).
    let after = start + run_len;
    let pad = buf.get(after..after + 3)?;
    if !matches!(pad[0], 0 | 1) || pad[1..].iter().any(|&b| b != 0) {
        return None;
    }
    let chars = u32_at(buf, after + 3)? as usize;
    if chars == 0 || chars > MAX_NAME_CHARS {
        return None;
    }
    let value = after + 7;
    let end = value.checked_add(chars * 2)?;
    let units: Vec<u16> = buf.get(value..end)?.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p)).collect();
    let name = String::from_utf16(&units).ok().filter(|n| !n.chars().any(char::is_control))?;
    let marker = tags.elevation_marker();
    let window = buf.get(end..(end + ELEVATION_SEARCH).min(buf.len()))?;
    let at = end + memchr::memmem::find(window, &marker)? + ELEVATION_AFTER_MARKER;
    let elevation_feet = f64_at(buf, at).filter(|e| e.is_finite() && e.abs() <= MAX_ELEVATION_FEET)?;
    let copy = buf.get(at.checked_sub(CONFIRM_LEAD_IN)?..at + 8)?;
    let later = buf.get(at + 8..(at + 8 + ELEVATION_SEARCH).min(buf.len()))?;
    memchr::memmem::find(later, copy)?;
    Some(Block { id, name, elevation_feet, building_story: building_story_after(buf, end, id_len == 8) })
}

/// A level's Building Story setting: the byte after the first unset id and `00 01` past its name, a number (0 on most levels)
/// and the level's own id, 1 for a building story and 0 for a level that is not. None where the frame is not found or the
/// byte is neither.
fn building_story_after(buf: &[u8], name_end: usize, wide: bool) -> Option<bool> {
    let unset: &[u8] = if wide { &[0xff; 8] } else { &[0xff; 4] };
    let anchor = [unset, &[0, 1][..]].concat();
    let window = buf.get(name_end..(name_end + STORY_WINDOW).min(buf.len()))?;
    let after = name_end + memchr::memmem::find(window, &anchor)? + anchor.len();
    let id_at = after + 8;
    let setting = if wide {
        u32::try_from(u64_at(buf, id_at)?).ok()?;
        id_at + 8
    } else {
        id_at + 4
    };
    match buf.get(setting)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// The levels inside other elements: objects keyed by an id (`01 00 00 00` and the `u64`) whose block has the owner's id after
/// eight `ff` where a project level's block has its run of 56.
fn owned_64(buf: &[u8], ids: &[u32], found: &mut Found) {
    for at in memchr::memmem::find_iter(buf, &OBJECT_PREFIX) {
        let id_at = at + OBJECT_PREFIX.len();
        let Some(id) = u64_at(buf, id_at).and_then(|v| u32::try_from(v).ok()).filter(|&id| declared(ids, id)) else { continue };
        let chars = u32_at(buf, id_at + NAME_AFTER_OWNER - 4).unwrap_or(0) as usize;
        let sentinel = buf.get(id_at + 8..id_at + 16);
        let owner = u64_at(buf, id_at + 16);
        if (1..=MAX_NAME_CHARS).contains(&chars)
            && sentinel.is_some_and(|run| run.iter().all(|&b| b == 0xff))
            && owner.is_some_and(|o| o != 0 && o < u64::from(u32::MAX))
        {
            found.owned.insert(id);
        }
    }
}

/// What one pass over a project's partitions finds: the inventory, and the records of the categories asked for.
pub(crate) struct Survey {
    pub release: u32,
    pub tags: Tags,
    /// The ElementIds the element table declares, sorted.
    pub ids: Vec<u32>,
    pub inventory: Inventory,
    /// Every copy of every record of the categories asked for (an edited element is in several partitions).
    pub records: Vec<Record>,
    /// Every type's record, of every category.
    pub types: Vec<TypeRecord>,
}

/// The categories whose object-styles material a layer may take: walls, floors, roofs and ceilings.
const CATEGORY_MATERIAL_CATEGORIES: [i64; 4] = [-2_000_011, -2_000_032, -2_000_035, -2_000_038];

/// What the second pass finds.
pub(crate) struct Gathered {
    pub data: Data,
    /// The name of each material whose name was read once and is its alone.
    pub materials: BTreeMap<u32, String>,
    /// The material the object styles give a category, where they give one: what a layer with no material of its own is made of.
    pub category: BTreeMap<i64, u32>,
    /// The name of each family and family type, with its category (2024 and later), as its newest partition holds it.
    pub names: BTreeMap<u32, (String, Option<i64>)>,
    /// The bottom, in feet, of the columns whose geometry node corrects their record box.
    pub column_bottoms: BTreeMap<u32, f64>,
    /// The number and the name of each room.
    pub rooms: BTreeMap<u32, (String, String)>,
}

/// What the second pass is asked for.
#[derive(Default)]
pub(crate) struct Wants {
    /// The elements whose data is kept.
    pub data: BTreeSet<u32>,
    /// The ids of the materials.
    pub materials: BTreeSet<u32>,
    /// The columns, with their record boxes.
    pub columns: BTreeMap<u32, [f64; 6]>,
    /// The rooms.
    pub rooms: BTreeSet<u32>,
}

impl crate::Revit {
    /// Reads every element of the project and its levels.
    ///
    /// Needs the file's release to be one the record forms are known for ([`RELEASES`]); a release outside it is an error,
    /// not a guess.
    pub fn inventory(&mut self) -> Result<Inventory> {
        Ok(self.survey(&|_| false)?.inventory)
    }

    /// The data of the `wanted` elements, every copy, and the names of the project's materials: a second pass over the
    /// partitions, for what the records name.
    pub(crate) fn gather(&mut self, release: u32, tags: &Tags, ids: &[u32], wants: &Wants) -> Result<Gathered> {
        let names: Vec<String> = self.stream_names().into_iter().filter(|n| n.starts_with("Partitions/")).map(String::from).collect();
        let wide = release >= 2024;
        // Up to 2023 a material the object does not name gives itself a name in its own data.
        let (material_ids, columns) = (&wants.materials, &wants.columns);
        let mut wanted = wants.data.clone();
        if !wide {
            wanted.extend(material_ids);
        }
        let wanted = &wanted;
        let mut data = Data::new();
        let mut materials: BTreeMap<u32, Option<String>> = BTreeMap::new();
        let mut styled: BTreeMap<i64, BTreeSet<u64>> = BTreeMap::new();
        let mut named = Latest::default();
        let mut nodes = BTreeMap::new();
        let mut room_names = Latest::default();
        for name in names {
            let number = name.rsplit('/').next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
            let buf = self.inflate_all(&name)?.bytes;
            element_data(&buf, tags, wide, wanted, number, &mut data);
            material_names(&buf, tags, wide, ids, &mut materials);
            category_materials(&buf, tags, release, &CATEGORY_MATERIAL_CATEGORIES, &mut styled);
            column_nodes(&buf, columns, &mut nodes);
            room_entries(&buf, wide, &wants.rooms, number, &mut room_names);
            let mut entries = Vec::new();
            if wide {
                entries_64(&buf, ids, &mut entries);
            } else {
                entries_32(&buf, ids, &mut entries);
            }
            for e in entries {
                named.offer(e.id, number, (e.name, e.category));
            }
        }
        if !wide {
            let header = tags.data_header();
            for &id in material_ids {
                if materials.get(&id).is_some_and(Option::is_some) {
                    continue;
                }
                if let Some((name, _)) = data.get(&id).and_then(|c| newest(c, |b| first_name(b, &header, release))) {
                    materials.insert(id, Some(name));
                }
            }
        }
        // A name two materials would share is misread for at least one of them.
        let mut uses: BTreeMap<&str, usize> = BTreeMap::new();
        for name in materials.values().flatten() {
            *uses.entry(name.as_str()).or_default() += 1;
        }
        let materials: BTreeMap<u32, String> =
            materials.iter().filter_map(|(id, n)| Some((*id, n.clone()?))).filter(|(_, n)| uses.get(n.as_str()) == Some(&1)).collect();
        // A category's material: the one value written, a named material.
        let category: BTreeMap<i64, u32> = styled
            .into_iter()
            .filter_map(|(c, values)| {
                let [v] = values.into_iter().collect::<Vec<u64>>()[..] else { return None };
                let id = u32::try_from(v).ok().filter(|&id| id != 0 && id != u32::MAX && materials.contains_key(&id))?;
                Some((c, id))
            })
            .collect();
        Ok(Gathered {
            data,
            materials,
            category,
            names: named.into_values(),
            column_bottoms: column_bottoms(nodes, columns),
            rooms: room_names.into_values(),
        })
    }

    /// [`Revit::inventory`], and the records of the categories `keep` says yes to.
    pub(crate) fn survey(&mut self, keep: &dyn Fn(i64) -> bool) -> Result<Survey> {
        let release = self.basic_file_info()?.release;
        if !RELEASES.contains(&release) {
            return Err(Error::Format(format!(
                "Revit {release}: this reader knows the element records of releases {} to {} and does not guess at others",
                RELEASES.start(),
                RELEASES.end()
            )));
        }
        let schema = self.schema()?;
        let tags = Tags::of(&schema)?;
        let ids = self.element_ids()?;
        let names: Vec<String> = self.stream_names().into_iter().filter(|n| n.starts_with("Partitions/")).map(String::from).collect();
        if names.is_empty() {
            return Err(Error::Format("the file has no Partitions stream, so it holds no elements".into()));
        }
        let wide = release >= 2024;
        let mut elements: BTreeMap<(i64, u32), (u16, bool)> = BTreeMap::new();
        let mut level_records: BTreeMap<u32, bool> = BTreeMap::new();
        let mut named: BTreeMap<u32, (u32, Block)> = BTreeMap::new();
        let mut owned: BTreeSet<u32> = BTreeSet::new();
        let mut records = 0;
        let (mut kept, mut types) = (Vec::new(), Vec::new());
        for name in names {
            // One partition at a time: it is inflated, read and dropped.
            let number = name.rsplit('/').next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
            let buf = self.inflate_all(&name)?.bytes;
            let mut found = Found::default();
            let mut seen = |r: Record| {
                found.element(r.category, r.id, r.class_tag, r.placed);
                if keep(r.category) {
                    kept.push(r);
                }
            };
            if wide {
                records_64(&buf, &tags, &ids, number, &mut seen);
                types_64(&buf, &tags, &ids, number, &mut |t| types.push(t));
                levels_64(&buf, &tags, release, &ids, &mut found);
                owned_64(&buf, &ids, &mut found);
            } else {
                records_32(&buf, &tags, &ids, number, &mut seen);
                types_32(&buf, &tags, &ids, number, &mut |t| types.push(t));
                levels_32(&buf, &tags, release, &ids, &mut found);
            }
            blocks(&buf, &tags, &ids, wide, &mut found);
            drop(buf);
            records += found.records;
            owned.extend(found.owned);
            for ((category, id), (class_tag, placed)) in found.elements {
                let e = elements.entry((category, id)).or_insert((class_tag, false));
                e.1 |= placed;
            }
            for (id, placed) in found.level_records {
                *level_records.entry(id).or_insert(false) |= placed;
            }
            // A level edited after an earlier save is written again in a later partition: its newest block is current.
            for block in found.blocks {
                match named.get(&block.id) {
                    Some((held, _)) if *held > number => {}
                    _ => {
                        named.insert(block.id, (number, block));
                    }
                }
            }
        }
        // A level inside another element is no project level, however its block reads.
        owned.retain(|id| level_records.contains_key(id));
        let mut levels: Vec<Level> = level_records
            .iter()
            .filter(|&(id, &placed)| placed && !owned.contains(id))
            .filter_map(|(id, _)| named.get(id))
            .map(|(_, b)| Level { id: b.id, name: b.name.clone(), elevation_feet: b.elevation_feet, building_story: b.building_story })
            .collect();
        levels.sort_by(|a, b| a.elevation_feet.total_cmp(&b.elevation_feet).then(a.id.cmp(&b.id)));
        let placed_levels: BTreeSet<u32> = level_records.iter().filter(|&(id, &p)| p && !owned.contains(id)).map(|(&id, _)| id).collect();
        let unread_levels = placed_levels.len() - levels.len();
        for (&id, &placed) in &level_records {
            let e = elements.entry((OST_LEVELS, id)).or_insert((0, false));
            e.1 |= placed;
        }
        let elements =
            elements.into_iter().map(|((category, id), (class_tag, placed))| Element { category, id, class_tag, placed }).collect();
        let inventory =
            Inventory { declared: ids.len(), records, elements, levels, unread_levels, owned_levels: owned.into_iter().collect() };
        Ok(Survey { release, tags, ids, inventory, records: kept, types })
    }
}
