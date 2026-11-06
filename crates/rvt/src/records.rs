// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/partition_element_records.rs (the
// 64-bit element record, its fixed offsets, the counted reference lists after the box, the record chain and the
// second-prologue rule), src/partition_element_records_2023.rs (the 32-bit form, 2015 to 2023) and
// src/partition_type_records.rs (the bounding-box-less record of a type).
// Changed: rewritten for BIMCraft; the record marker always comes from the file's own schema, every offset is
// bounds-checked, records are handed to a callback instead of collected whole, and one function reads either form.
//! The records of a partition: one for every element, found by their marker.
//!
//! A partition (`Partitions/NN`) holds a record per element, each opening with a header (the ElementId, the record's size,
//! a constant, a count) and closing with a 16-byte trailer that repeats the size. In 2024 and later a record's frame has
//! fixed offsets from its start: the `BuiltInCategory` (a negative `i64`, Autodesk's published API constant) at `+0x12`, the
//! container reference at `+0x32`, the placement kind at `+0x42`, the class tag at `+0x4a`, a marker at `+0x50` (the tags of
//! `Outline` and `ElementParents` with `ff` ×4 between), the bounding box of six `f64` (feet) at `+0x58`, and then two counted
//! lists of ElementIds. The first names, among others, the element's type, the levels it is constrained to and the walls it
//! joins; an opening's host is the slot before its own id; the last slot of the second names the owner of a sketch line.
//! Some frames have no ElementId of their own (the second prologue): they sit inside another record's body and take that
//! record's id. Up to 2023 the ElementId is a `u32` 52 bytes before the marker, the category sits 38 bytes before it, and
//! the lists hold `u32`s.
//!
//! A *placed instance* is a record with no container and the placed kind. A type has a record too, with no bounding box:
//! its marker is only the last six bytes, and its placement kind is `0xffff8080`. Its slot list holds, among others, its
//! own id.

use crate::error::{Error, Result};
use crate::schema::Schema;

/// The band `BuiltInCategory` values fall in.
const CATEGORY_MIN: i64 = -2_100_000;
const CATEGORY_MAX: i64 = -1_990_000;

/// Offsets of a 2024 and later frame from its start.
pub(crate) const CATEGORY_AT: usize = 0x12;
pub(crate) const CONTAINER_AT: usize = 0x32;
pub(crate) const KIND_AT: usize = 0x42;
const CLASS_AT: usize = 0x4a;
pub(crate) const MARKER_AT: usize = 0x50;
const BBOX_AT: usize = 0x58;
const FRAME_LEN: usize = BBOX_AT + 48;
const LIST_AT: usize = FRAME_LEN;
/// The prologue constant sits at `+0x0c` of a type record, and its slot list at `+0x56`.
const PROLOGUE_AT: usize = 0x0c;
const TYPE_SLOTS_AT: usize = 0x56;
/// The kind word of a placed instance, and "no container".
pub(crate) const KIND_PLACED: u32 = 0xffff_ef7f;
/// The kind word of a type's record.
pub(crate) const KIND_TYPE: u32 = 0xffff_8080;
pub(crate) const NO_CONTAINER: u64 = u64::MAX;
/// The most slots a reference list may hold.
const MAX_REFERENCES: usize = 1024;

/// The furthest a length may be from zero, in feet, for the reader to take it as a length. Revit's working limit is 20 miles
/// (105,600 ft) from the origin, so a finite value beyond this is damage; it is also far inside the model's own limit once in millimetres, which
/// keeps a hostile number out of the geometry the importer asks of the model before it is checked.
pub(crate) const MAX_FEET: f64 = 3.0e5;

/// A record chain's header and trailer lengths.
const CHAIN_HEADER: usize = 16;
const CHAIN_TRAILER: usize = 16;

/// 32-bit form (up to 2023), as offsets back from the marker.
pub(crate) const CATEGORY_BEFORE_MARKER: usize = 38;
const CONTAINER_BEFORE_MARKER: usize = 22;
const KIND_BEFORE_MARKER: usize = 14;
const CLASS_BEFORE_MARKER: usize = 6;
pub(crate) const HEADER_TAG_AFTER_ID: usize = 8;
/// The most items the list after a 32-bit record's header tag may hold, and the bytes of one.
const MAX_ITEMS: usize = 1024;
const ITEM_LEN: usize = 14;

pub(crate) fn u16_at(buf: &[u8], at: usize) -> Option<u16> {
    let b = buf.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

pub(crate) fn u32_at(buf: &[u8], at: usize) -> Option<u32> {
    let b = buf.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub(crate) fn u64_at(buf: &[u8], at: usize) -> Option<u64> {
    let b = buf.get(at..at.checked_add(8)?)?;
    Some(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

pub(crate) fn f64_at(buf: &[u8], at: usize) -> Option<f64> {
    u64_at(buf, at).map(f64::from_bits)
}

pub(crate) fn in_band(category: i64) -> bool {
    (CATEGORY_MIN..=CATEGORY_MAX).contains(&category)
}

/// The tags of the schema classes the scans read the file's layout from. A class's tag is its place in the schema and moves
/// from one release to the next, so each is looked up by name.
pub(crate) struct Tags {
    pub outline: u16,
    pub parents: u16,
    pub header: u16,
    pub plane: u16,
    /// `CellList`: opens an element's serialised data.
    pub cell_list: u16,
    /// `VerticalRegionsStructure`: frames a type's layer count.
    pub vertical_regions: Option<u16>,
    /// `Material`: a material object's class.
    pub material: Option<u16>,
    /// `PhysicalParamSet`: ends a material's own name.
    pub physical_param_set: Option<u16>,
    /// `PatternHelper`: frames a string that names a material.
    pub pattern_helper: Option<u16>,
    /// `ProfileRoof`: a roof by footprint.
    pub profile_roof: Option<u16>,
    /// `Roof`: a roof by extrusion.
    pub roof: Option<u16>,
}

impl Tags {
    pub fn of(schema: &Schema) -> Result<Self> {
        let tag = |name: &str| {
            schema
                .tag(name)
                .ok_or_else(|| Error::Format(format!("Formats/Latest has no class {name}, so the element records cannot be found")))
        };
        Ok(Self {
            outline: tag("Outline")?,
            parents: tag("ElementParents")?,
            header: tag("ElementHeader")?,
            plane: tag("Plane")?,
            cell_list: schema.tag("CellList").unwrap_or(0),
            vertical_regions: schema.tag("VerticalRegionsStructure"),
            material: schema.tag("Material"),
            physical_param_set: schema.tag("PhysicalParamSet"),
            pattern_helper: schema.tag("PatternHelper"),
            profile_roof: schema.tag("ProfileRoof"),
            roof: schema.tag("Roof"),
        })
    }

    /// `[Outline tag] ff ff ff ff [ElementParents tag]`, the marker of an element record.
    pub fn marker(&self) -> [u8; 8] {
        let mut m = [0xff; 8];
        m[..2].copy_from_slice(&self.outline.to_le_bytes());
        m[6..].copy_from_slice(&self.parents.to_le_bytes());
        m
    }

    /// `05 00 00 00` and the tag of `Plane`: what precedes a Level's elevation.
    pub fn elevation_marker(&self) -> [u8; 6] {
        let mut m = [5, 0, 0, 0, 0, 0];
        m[4..].copy_from_slice(&self.plane.to_le_bytes());
        m
    }

    /// The header that opens an element's serialised data: `ff ff ff ff`, the tag of `CellList` and `01 00 00 00`. The
    /// element's id follows it, a `u64` from 2024 and a `u32` before.
    pub fn data_header(&self) -> [u8; 10] {
        let mut h = [0xff, 0xff, 0xff, 0xff, 0, 0, 1, 0, 0, 0];
        h[4..6].copy_from_slice(&self.cell_list.to_le_bytes());
        h
    }
}

/// One record of an element: where it is, what it is, its box and its reference lists.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// The number of the partition it is in (`Partitions/21` is 21): a later partition holds a newer copy of an edited element.
    pub partition: u32,
    /// Where its frame starts in the partition.
    pub offset: usize,
    pub id: u32,
    pub category: i64,
    pub class_tag: u16,
    /// No container and the placed kind: a standalone instance in the model.
    pub placed: bool,
    /// The model box in feet: min x, y, z then max x, y, z.
    pub bbox: [f64; 6],
    /// The first reference list, verbatim.
    pub refs: Vec<u64>,
    /// The second reference list, verbatim.
    pub refs2: Vec<u64>,
    /// The frame has no id of its own: it sits in another record's body (the second prologue) and takes that record's id.
    pub inside: bool,
}

impl Record {
    /// The slot before this record's own id in its first list: an opening's host wall.
    pub fn preceding(&self) -> Option<u32> {
        let at = self.refs.iter().rposition(|&v| v == u64::from(self.id))?;
        self.refs.get(at.checked_sub(1)?).and_then(|&v| u32::try_from(v).ok()).filter(|&v| v != 0)
    }

    /// The box's extent along each axis.
    pub fn extent(&self) -> [f64; 3] {
        [self.bbox[3] - self.bbox[0], self.bbox[4] - self.bbox[1], self.bbox[5] - self.bbox[2]]
    }
}

/// A record with no box: a type (of a wall, a floor, a roof, a ceiling or whatever a category has, kind `0xffff8080`), a
/// material (`OST_Materials`, `0xffff8020`) and the like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeRecord {
    pub partition: u32,
    pub id: u32,
    pub category: i64,
    pub kind: u32,
}

impl TypeRecord {
    /// Whether it is a type, as a wall's or a floor's.
    pub fn is_type(&self) -> bool {
        self.kind == KIND_TYPE
    }
}

pub(crate) fn declared(ids: &[u32], id: u32) -> bool {
    id != 0 && ids.binary_search(&id).is_ok()
}

/// The ElementId of the 32-bit record whose category starts at `category_at`, and where the record starts. A record is a `u32`
/// id, a `u32` size, the header tag, a `u32` count and that many items of 14 bytes, and then the category: the count is 0 in a
/// file that is not workshared, so the id sits 14 bytes before the category, and 14 more for each item.
pub(crate) fn id_before(buf: &[u8], category_at: usize, header: u16, ids: &[u32]) -> Option<(u32, usize)> {
    (0..=MAX_ITEMS).find_map(|n| {
        let id_at = category_at.checked_sub(14 + ITEM_LEN * n)?;
        let framed = u16_at(buf, id_at + HEADER_TAG_AFTER_ID) == Some(header) && u32_at(buf, id_at + 10) == Some(n as u32);
        u32_at(buf, id_at).filter(|&id| framed && declared(ids, id)).map(|id| (id, id_at))
    })
}

/// A record in a partition's chain: where it starts and ends, and its ElementId.
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
    pub id: u64,
}

/// The leading chain of records of a 2024 and later partition: header (`u64` id, `u32` size, `u16` constant, `u16`
/// count), body, and a 16-byte trailer that repeats the size. The constant is the marker's last tag less 12.
pub(crate) fn chain(buf: &[u8], marker: &[u8; 8]) -> Vec<Span> {
    let constant = u16::from_le_bytes([marker[6], marker[7]]).wrapping_sub(12);
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(id) = u64_at(buf, at) {
        let Some(size) = u32_at(buf, at + 8).map(|v| v as usize) else { break };
        if size < CHAIN_HEADER || u16_at(buf, at + 0x0c) != Some(constant) {
            break;
        }
        let Some(end) = at.checked_add(size) else { break };
        let flags = end.checked_add(8).and_then(|t| u32_at(buf, t));
        let echo = end.checked_add(12).and_then(|t| u32_at(buf, t));
        if !matches!(flags, Some(0 | 0x0100_0000)) || echo.map(|v| v as usize) != Some(size) {
            break;
        }
        out.push(Span { start: at, end, id });
        at = end + CHAIN_TRAILER;
    }
    out
}

/// How many records a partition's chain has (for tests).
#[cfg(test)]
pub(crate) fn chain_len(buf: &[u8], schema: &Schema) -> Option<(usize, usize)> {
    let marker = Tags::of(schema).ok()?.marker();
    let spans = chain(buf, &marker);
    Some((spans.len(), spans.last().map_or(0, |s| s.end + CHAIN_TRAILER)))
}

pub(crate) fn enclosing(chain: &[Span], at: usize) -> Option<&Span> {
    let i = chain.partition_point(|s| s.start <= at);
    let span = chain.get(i.checked_sub(1)?)?;
    (at < span.end).then_some(span)
}

/// A frame with no ElementId of its own: 0, or a value that is not a `u32` ElementId.
pub(crate) fn no_id(raw: u64) -> bool {
    raw == 0 || raw >= u64::from(u32::MAX)
}

fn finite_box(buf: &[u8], at: usize) -> Option<[f64; 6]> {
    let mut b = [0.0f64; 6];
    for (i, v) in b.iter_mut().enumerate() {
        match f64_at(buf, at + i * 8) {
            Some(x) if x.is_finite() && x.abs() <= MAX_FEET => *v = x,
            _ => return None,
        }
    }
    (0..3).all(|a| b[a + 3] >= b[a]).then_some(b)
}

/// A counted list of `u64` slots (`u32` slots when `wide` is false) at `at`, and where it ends. None when the count is over
/// the limit or the list runs past the buffer; a count of zero reads as an empty list only in the 32-bit form (2024 and later
/// read a zero count as no list, so no second list follows it).
fn list_at(buf: &[u8], at: usize, wide: bool) -> Option<(Vec<u64>, usize)> {
    let count = u32_at(buf, at)? as usize;
    if count > MAX_REFERENCES || (wide && count == 0) {
        return None;
    }
    let width = if wide { 8 } else { 4 };
    let start = at.checked_add(4)?;
    let end = start.checked_add(count * width)?;
    let slots = buf.get(start..end)?;
    let list = slots
        .chunks_exact(width)
        .map(|c| {
            if wide {
                u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
            } else {
                u64::from(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            }
        })
        .collect();
    Some((list, end))
}

fn lists(buf: &[u8], at: usize, wide: bool) -> (Vec<u64>, Vec<u64>) {
    match list_at(buf, at, wide) {
        Some((first, end)) => (first, list_at(buf, end, wide).map(|(second, _)| second).unwrap_or_default()),
        None => (Vec::new(), Vec::new()),
    }
}

/// The records of a 2024 and later partition, each handed to `keep`.
pub(crate) fn records_64(buf: &[u8], tags: &Tags, ids: &[u32], partition: u32, keep: &mut dyn FnMut(Record)) {
    let marker = tags.marker();
    let spans = chain(buf, &marker);
    for hit in memchr::memmem::find_iter(buf, &marker) {
        let Some(f) = hit.checked_sub(MARKER_AT) else { continue };
        if f + FRAME_LEN > buf.len() || u16_at(buf, f + 0x10) != Some(0) {
            continue;
        }
        let Some(category) = u64_at(buf, f + CATEGORY_AT).map(|v| v as i64).filter(|&c| in_band(c)) else { continue };
        let container = u64_at(buf, f + CONTAINER_AT).unwrap_or(0);
        let kind = u64_at(buf, f + KIND_AT).map_or(0, |v| v as u32);
        let placed = container == NO_CONTAINER && kind == KIND_PLACED;
        let raw = u64_at(buf, f).unwrap_or(0);
        let inside = no_id(raw);
        let id = if inside {
            // The second prologue: a placed instance inside another record, which gives it its id.
            let host = placed.then(|| enclosing(&spans, f)).flatten().and_then(|s| u32::try_from(s.id).ok());
            match host {
                Some(id) => id,
                None => continue,
            }
        } else {
            raw as u32
        };
        let Some(bbox) = finite_box(buf, f + BBOX_AT).filter(|_| declared(ids, id)) else { continue };
        let (refs, refs2) = lists(buf, f + LIST_AT, true);
        keep(Record {
            partition,
            offset: f,
            id,
            category,
            class_tag: u16_at(buf, f + CLASS_AT).unwrap_or(0),
            placed,
            bbox,
            refs,
            refs2,
            inside,
        });
    }
}

/// The records of a partition up to 2023.
pub(crate) fn records_32(buf: &[u8], tags: &Tags, ids: &[u32], partition: u32, keep: &mut dyn FnMut(Record)) {
    for hit in memchr::memmem::find_iter(buf, &tags.marker()) {
        let Some(category_at) = hit.checked_sub(CATEGORY_BEFORE_MARKER) else { continue };
        let Some((id, id_at)) = id_before(buf, category_at, tags.header, ids) else { continue };
        let Some(category) = u64_at(buf, category_at).map(|v| v as i64).filter(|&c| in_band(c)) else {
            continue;
        };
        let Some(bbox) = finite_box(buf, hit + 8) else { continue };
        let container = u32_at(buf, hit - CONTAINER_BEFORE_MARKER);
        let kind = u32_at(buf, hit - KIND_BEFORE_MARKER);
        let placed = container == Some(u32::MAX) && kind == Some(KIND_PLACED);
        let (refs, refs2) = match list_at(buf, hit + 8 + 48, false) {
            Some((first, end)) => (first, list_at(buf, end, false).map(|(second, _)| second).unwrap_or_default()),
            None => (Vec::new(), Vec::new()),
        };
        keep(Record {
            partition,
            offset: id_at,
            id,
            category,
            class_tag: u16_at(buf, hit - CLASS_BEFORE_MARKER).unwrap_or(0),
            placed,
            bbox,
            refs,
            refs2,
            inside: false,
        });
    }
}

/// The box-less records of a 2024 and later partition: the last six bytes of the marker at `+0x50`, the prologue constant at
/// `+0x0c`, and a slot list at `+0x56` that holds the record's own id. A frame with no id of its own (the second prologue) sits
/// inside another record, takes its id, carries no prologue constant, and its slot list must name that id.
pub(crate) fn types_64(buf: &[u8], tags: &Tags, ids: &[u32], partition: u32, keep: &mut dyn FnMut(TypeRecord)) {
    let marker = tags.marker();
    let spans = chain(buf, &marker);
    let constant = u32::from(tags.parents.wrapping_sub(12));
    for hit in memchr::memmem::find_iter(buf, &marker[2..]) {
        let Some(f) = hit.checked_sub(MARKER_AT) else { continue };
        if u16_at(buf, f + 0x10) != Some(0) {
            continue;
        }
        let Some(category) = u64_at(buf, f + CATEGORY_AT).map(|v| v as i64).filter(|&c| in_band(c)) else { continue };
        let kind = u64_at(buf, f + KIND_AT).map_or(0, |v| v as u32);
        let raw = u64_at(buf, f).unwrap_or(0);
        let id = if no_id(raw) {
            enclosing(&spans, f).and_then(|s| u32::try_from(s.id).ok())
        } else if u32_at(buf, f + PROLOGUE_AT) == Some(constant) {
            Some(raw as u32)
        } else {
            None
        };
        let Some(id) = id.filter(|&id| declared(ids, id)) else { continue };
        let count = u32_at(buf, f + TYPE_SLOTS_AT).map_or(0, |c| c as usize);
        let slots = (1..=MAX_REFERENCES)
            .contains(&count)
            .then(|| (0..count).map(|i| u64_at(buf, f + TYPE_SLOTS_AT + 4 + 8 * i)).collect::<Option<Vec<u64>>>())
            .flatten();
        if slots.is_some_and(|s| s.contains(&u64::from(id))) {
            keep(TypeRecord { partition, id, category, kind });
        }
    }
}

/// The box-less records of a partition up to 2023: laid out back from the marker, as a level's.
pub(crate) fn types_32(buf: &[u8], tags: &Tags, ids: &[u32], partition: u32, keep: &mut dyn FnMut(TypeRecord)) {
    let marker = tags.marker();
    for hit in memchr::memmem::find_iter(buf, &marker[2..]) {
        let Some(category_at) = hit.checked_sub(CATEGORY_BEFORE_MARKER) else { continue };
        let Some((id, _)) = id_before(buf, category_at, tags.header, ids) else { continue };
        let Some(category) = u64_at(buf, category_at).map(|v| v as i64).filter(|&c| in_band(c)) else { continue };
        let container = u32_at(buf, category_at + 16);
        let Some(kind) = u32_at(buf, category_at + 24) else { continue };
        if container == Some(u32::MAX) {
            keep(TypeRecord { partition, id, category, kind });
        }
    }
}
