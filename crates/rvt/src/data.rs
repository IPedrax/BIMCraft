// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/partition_names.rs (the element-data
// header and its id), src/partition_beam_axes.rs (`scan_first_records`: an element's data runs to the next header) and
// src/partition_materials.rs (a material object and its name, RE-58, RE-116).
// Changed: rewritten for BIMCraft; the header tag and the tags that frame a material's name come from the file's own schema,
// every copy of an element's data is kept with its partition (the caller picks), and a material's name is read by the rules
// the samples confirm (its own name field, the name that ends at `PhysicalParamSet`, the first string framed by
// `PatternHelper`, the first `-1001203` entry).
//! The serialised data of elements, and the names of materials.
//!
//! Beside its record (see [`crate::records`]) an element has data: its parameters, its location line, the layers of a type.
//! It opens with `ff ff ff ff`, the tag of `CellList`, `01 00 00 00` and the element's id (a `u64` from 2024, a `u32` before,
//! and four more bytes), and runs to the next such header. An element edited after an earlier save is written again in a
//! later partition and the earlier copy stays, so a reader gets every copy, with the number of its partition.
//!
//! A material is an object keyed by its id (`01 00 00 00` and the id) with the schema's tag of `Material` a fixed distance
//! on (0x47 bytes past a `u64` id, 0x27 past a `u32` one). Its name is, in this order: its own name field after the class
//! tag (two to four zero `u32`s, then a counted string, unless a built-in parameter id follows); the one counted string that
//! ends right before `ff ff ff ff` and the tag of `PhysicalParamSet`; the first string framed by the tag of `PatternHelper`;
//! the first built-in parameter `-1001203` entry; and, from 2024, the first string closed by eight zero bytes, an id and
//! `ff` ×8. An id whose copies disagree has no name, and nor has a name that two
//! materials would share: Revit keeps material names unique, so one of them is misread.
//!
//! A room's number and name are parameter entries in the room's object (`01 00 00 00` and its id, a `u32` up to 2023 and a `u64`
//! after): the first `ROOM_NUMBER` (-1006901), a counted UTF-16 string and, right after it, `ROOM_NAME` (-1006900) and another
//! (the parameter ids as wide as the release's ElementIds), within 0x400 bytes of the id. An empty text is a count of 0. The
//! newest partition's copy decides, even where it is empty.
//!
//! A column's record box can start at 0.0 while the column starts higher: the column's geometry node holds the truer box. The node
//! is the column's `u64` id, `u64 3` and `u64 -1`, preceded by two boxes of six `f64` 96 and 48 bytes before the id; the second
//! holds Revit's z-range, and the plan extents of both equal the record's.
//!
//! A layer with no material of its own takes its category's, which the document's object styles hold: the category's id, an
//! unset id, `01 00 00 00`, another unset id and the material (all `u64`) up to 2024, and from the id's `i32` an unset word,
//! a 1 or 2, `01 00 00 00`, `ff ff ff ff` and the tag of `PatternHelper`, eight bytes, `i32 -3000010` and the material
//! (`u32`) before. Read only where it is the same everywhere it is written.

use std::collections::{BTreeMap, BTreeSet};

use crate::names::Latest;
use crate::records::{Tags, declared, f64_at, u32_at, u64_at};

/// One copy of an element's data: the number of the partition it is in, and the bytes from just past its id to the next
/// element's header (or the window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub partition: u32,
    pub bytes: Vec<u8>,
}

/// The copies of each wanted element's data.
pub type Data = BTreeMap<u32, Vec<Copy>>;

/// How far an element's data is read.
pub const WINDOW: usize = 0x4_0000;

/// The data of the `wanted` elements in a partition.
pub(crate) fn element_data(buf: &[u8], tags: &Tags, wide: bool, wanted: &BTreeSet<u32>, partition: u32, out: &mut Data) {
    if wanted.is_empty() {
        return;
    }
    let header = tags.data_header();
    let hits: Vec<usize> = memchr::memmem::find_iter(buf, &header).collect();
    for (index, &hit) in hits.iter().enumerate() {
        let at = hit + header.len();
        let id = if wide { u64_at(buf, at).and_then(|v| u32::try_from(v).ok()) } else { u32_at(buf, at) };
        let Some(id) = id.filter(|id| wanted.contains(id)) else { continue };
        let end = hits.get(index + 1).copied().unwrap_or(buf.len()).min(hit.saturating_add(WINDOW)).min(buf.len());
        if let Some(bytes) = buf.get(at + 8..end) {
            out.entry(id).or_default().push(Copy { partition, bytes: bytes.to_vec() });
        }
    }
}

/// The newest copy of an element's data that `read` makes something of, and whether an older copy read differently.
pub fn newest<T: PartialEq>(copies: &[Copy], read: impl Fn(&[u8]) -> Option<T>) -> Option<(T, bool)> {
    let mut read_all: Vec<(u32, T)> = copies.iter().filter_map(|c| read(&c.bytes).map(|v| (c.partition, v))).collect();
    read_all.sort_by_key(|(partition, _)| *partition);
    let (_, last) = read_all.pop()?;
    let differs = read_all.iter().any(|(_, v)| *v != last);
    Some((last, differs))
}

/// What follows a category's id in the object styles up to 2024, before its material.
const CATEGORY_FRAME_64: [u8; 20] =
    [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
/// `i32 -3000010`: right before the material in the 32-bit form.
const CATEGORY_MARK_32: [u8; 4] = [0x36, 0x39, 0xd2, 0xff];

/// The materials the object styles give `categories`, as written in a partition: every value found for each.
pub(crate) fn category_materials(buf: &[u8], tags: &Tags, release: u32, categories: &[i64], out: &mut BTreeMap<i64, BTreeSet<u64>>) {
    for &category in categories {
        if release >= 2024 {
            // Measured on 2024; Revit 2025's files hold no such entry.
            if release != 2024 {
                continue;
            }
            let mut pattern = category.to_le_bytes().to_vec();
            pattern.extend_from_slice(&CATEGORY_FRAME_64);
            for at in memchr::memmem::find_iter(buf, &pattern) {
                if let Some(material) = u64_at(buf, at + pattern.len()) {
                    out.entry(category).or_default().insert(material);
                }
            }
        } else if let (Some(tag), Ok(id)) = (tags.pattern_helper, i32::try_from(category)) {
            let mut frame = [0xff, 0xff, 0xff, 0xff, 0, 0];
            frame[4..].copy_from_slice(&tag.to_le_bytes());
            let mut pattern = id.to_le_bytes().to_vec();
            pattern.extend_from_slice(&[0xff; 4]);
            for at in memchr::memmem::find_iter(buf, &pattern) {
                let word = u32_at(buf, at + 8);
                if !matches!(word, Some(1 | 2))
                    || u32_at(buf, at + 12) != Some(1)
                    || buf.get(at + 16..at + 22) != Some(&frame[..])
                    || buf.get(at + 30..at + 34) != Some(&CATEGORY_MARK_32[..])
                {
                    continue;
                }
                if let Some(material) = u32_at(buf, at + 34) {
                    out.entry(category).or_default().insert(u64::from(material));
                }
            }
        }
    }
}

/// What follows a column's `u64` id in its geometry node: `u64 3 · u64 -1`.
const NODE_AFTER_ID: [u8; 16] = [3, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255];
const NODE_TOLERANCE: f64 = 0.01;
const PLAN_TOLERANCE: f64 = 1e-6;

fn node_box(buf: &[u8], at: usize) -> Option<[f64; 6]> {
    let mut out = [0.0; 6];
    for (i, v) in out.iter_mut().enumerate() {
        *v = f64_at(buf, at.checked_add(i * 8)?).filter(|v| v.is_finite() && v.abs() <= crate::records::MAX_FEET)?;
    }
    (out[3] > out[0] && out[4] > out[1] && out[5] > out[2]).then_some(out)
}

/// The two boxes of a column's geometry node.
pub(crate) type Node = ([f64; 6], [f64; 6]);

/// Every geometry node of a column in `targets` in a partition.
pub(crate) fn column_nodes(buf: &[u8], targets: &BTreeMap<u32, [f64; 6]>, out: &mut BTreeMap<u32, Vec<Node>>) {
    if targets.is_empty() {
        return;
    }
    for at in memchr::memmem::find_iter(buf, &NODE_AFTER_ID) {
        let Some(id_at) = at.checked_sub(8).filter(|&a| a >= 96) else { continue };
        let Some(id) = u64_at(buf, id_at).and_then(|v| u32::try_from(v).ok()).filter(|id| targets.contains_key(id)) else { continue };
        if let (Some(first), Some(second)) = (node_box(buf, id_at - 96), node_box(buf, id_at - 48)) {
            out.entry(id).or_default().push((first, second));
        }
    }
}

fn plan_equal(a: &[f64; 6], b: &[f64; 6], tolerance: f64) -> bool {
    [0, 1, 3, 4].iter().all(|&i| (a[i] - b[i]).abs() < tolerance)
}

/// The bottom, in feet, of each column that a geometry node gives and agrees on: only for a column whose record box begins
/// where the node's does not, with the same plan and the same top.
pub(crate) fn column_bottoms(nodes: BTreeMap<u32, Vec<Node>>, targets: &BTreeMap<u32, [f64; 6]>) -> BTreeMap<u32, f64> {
    nodes
        .into_iter()
        .filter_map(|(id, nodes)| {
            let record = targets.get(&id)?;
            let bottoms: Vec<f64> = nodes
                .iter()
                .filter(|(first, second)| {
                    plan_equal(first, second, 1e-9)
                        && plan_equal(second, record, PLAN_TOLERANCE)
                        && (second[5] - record[5]).abs() < NODE_TOLERANCE
                        && (second[2] - record[2]).abs() > NODE_TOLERANCE
                })
                .map(|(_, second)| second[2])
                .collect();
            let first = *bottoms.first()?;
            bottoms.iter().all(|b| (b - first).abs() < NODE_TOLERANCE).then_some((id, first))
        })
        .collect()
}

const ROOM_NUMBER: i64 = -1_006_901;
const ROOM_NAME: i64 = -1_006_900;
const ROOM_WINDOW: usize = 0x400;

fn counted(buf: &[u8], at: usize, limit: usize) -> Option<(String, usize)> {
    let n = usize::try_from(u32_at(buf, at)?).ok().filter(|&n| n <= limit)?;
    let bytes = buf.get(at + 4..at + 4 + 2 * n)?;
    let text = String::from_utf16(&bytes.as_chunks::<2>().0.iter().map(|u| u16::from_le_bytes(*u)).collect::<Vec<_>>()).ok()?;
    (!text.chars().any(char::is_control)).then_some((text, at + 4 + 2 * n))
}

/// The number and name of each room of `rooms` in a partition.
pub(crate) fn room_entries(buf: &[u8], wide: bool, rooms: &BTreeSet<u32>, partition: u32, out: &mut Latest<(String, String)>) {
    if rooms.is_empty() {
        return;
    }
    let (id_len, number, name) = if wide {
        (8, ROOM_NUMBER.to_le_bytes().to_vec(), ROOM_NAME.to_le_bytes().to_vec())
    } else {
        (4, (ROOM_NUMBER as i32).to_le_bytes().to_vec(), (ROOM_NAME as i32).to_le_bytes().to_vec())
    };
    for hit in memchr::memmem::find_iter(buf, &[1u8, 0, 0, 0]) {
        let id = if wide { u64_at(buf, hit + 4).and_then(|v| u32::try_from(v).ok()) } else { u32_at(buf, hit + 4) };
        let Some(id) = id.filter(|id| rooms.contains(id)) else { continue };
        let start = hit + 4 + id_len;
        let Some(window) = buf.get(start..(start + ROOM_WINDOW).min(buf.len())) else { continue };
        let pair = memchr::memmem::find_iter(window, &number).find_map(|at| {
            let at = start + at;
            let (n, after) = counted(buf, at + number.len(), 64)?;
            (buf.get(after..after + name.len())? == &name[..]).then_some(())?;
            let (m, _) = counted(buf, after + name.len(), 256)?;
            Some((n, m))
        });
        if let Some(pair) = pair {
            out.offer(id, partition, pair);
        }
    }
}

const NAME_WINDOW: usize = 0x1000;
const NAME_PARAMETER: i64 = -1_001_203;
const TAG_AFTER_ID_64: usize = 0x47;
const TAG_AFTER_ID_32: usize = 0x27;

fn utf16_name(buf: &[u8], at: usize, n: u32) -> Option<String> {
    let n = usize::try_from(n).ok().filter(|n| (1..=256).contains(n))?;
    let bytes = buf.get(at..at.checked_add(2 * n)?)?;
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|u| u16::from_le_bytes(*u)).collect();
    if units.iter().any(|u| !(0x20..0xfffe).contains(u)) {
        return None;
    }
    String::from_utf16(&units).ok()
}

fn is_builtin_parameter(id: i64) -> bool {
    (-2_000_000..=-1_000_000).contains(&id)
}

/// The name after two to four zero `u32`s at `from` in `object`, unless a built-in parameter id follows (a parameter entry).
fn own_name(object: &[u8], from: usize) -> Option<String> {
    let (mut at, mut zeros) = (from, 0);
    while zeros < 4 && u32_at(object, at) == Some(0) {
        at += 4;
        zeros += 1;
    }
    if zeros < 2 {
        return None;
    }
    let n = u32_at(object, at)?;
    let param = object.get(at + 4..at + 12).map(|b| i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]));
    let param32 = object.get(at + 4..at + 8).map(|b| i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])));
    if param.is_some_and(is_builtin_parameter) || param32.is_some_and(is_builtin_parameter) {
        return None;
    }
    // Private-use and specials-block units are never a name.
    utf16_name(object, at + 4, n).filter(|name| !name.chars().any(|c| matches!(c, '\u{e000}'..='\u{f8ff}' | '\u{fff0}'..='\u{ffff}')))
}

/// The one counted string that ends right before `ff ff ff ff` and the tag.
fn terminated(object: &[u8], from: usize, tag: u16) -> Option<String> {
    let mut terminator = [0xff, 0xff, 0xff, 0xff, 0, 0];
    terminator[4..].copy_from_slice(&tag.to_le_bytes());
    let end = from + memchr::memmem::find(object.get(from..)?, &terminator)?;
    let mut names = (1..=256u32).filter_map(|n| {
        let at = end.checked_sub(4 + 2 * n as usize).filter(|&at| at >= from)?;
        (u32_at(object, at)? == n).then(|| utf16_name(object, at + 4, n)).flatten()
    });
    let name = names.next()?;
    names.next().is_none().then_some(name)
}

/// Last resort: the first string in the object closed by eight zero bytes, a `u64` ElementId and `ff` ×8. A material whose own
/// name field is a run of shared-parameter entries keeps its name there, after the entries.
fn closed_name(object: &[u8], from: usize) -> Option<String> {
    (from..object.len().saturating_sub(4)).find_map(|at| {
        let n = u32_at(object, at)?;
        let name = utf16_name(object, at + 4, n)?;
        let after = at + 4 + 2 * usize::try_from(n).ok()?;
        let id = u64_at(object, after + 8)?;
        (object.get(after..after + 8) == Some(&[0u8; 8][..])
            && object.get(after + 16..after + 24) == Some(&[0xffu8; 8][..])
            && u32::try_from(id).is_ok_and(|id| id > 0))
        .then_some(name)
    })
}

/// A material's name, from its object `object` (the bytes from its id), whose class tag is at `tag_at`.
fn material_name(object: &[u8], tag_at: usize, tags: &Tags, wide: bool) -> Option<String> {
    let from = tag_at + 2;
    if let Some(name) = own_name(object, from) {
        return Some(name);
    }
    if let Some(name) = tags.physical_param_set.and_then(|t| terminated(object, from, t)) {
        return Some(name);
    }
    if let Some(tag) = tags.pattern_helper.filter(|_| wide) {
        let mut frame = [0xff, 0xff, 0xff, 0xff, 0, 0];
        frame[4..].copy_from_slice(&tag.to_le_bytes());
        let framed = memchr::memmem::find_iter(object, &frame).find_map(|hit| {
            let n = u32_at(object, hit + frame.len())?;
            utf16_name(object, hit + frame.len() + 4, n)
        });
        if framed.is_some() {
            return framed;
        }
    }
    for entry_len in [8usize, 4] {
        let mut entry = if entry_len == 8 { NAME_PARAMETER.to_le_bytes().to_vec() } else { (NAME_PARAMETER as i32).to_le_bytes().to_vec() };
        entry.extend_from_slice(&[0, 0, 0, 0]);
        let found = memchr::memmem::find_iter(object, &entry).find_map(|hit| {
            let n = u32_at(object, hit + entry.len())?;
            utf16_name(object, hit + entry.len() + 4, n)
        });
        if found.is_some() {
            return found;
        }
    }
    if wide { closed_name(object, from) } else { None }
}

/// The names of the materials in a partition, each id with the name its object gives, or none where its copies here disagree.
pub(crate) fn material_names(buf: &[u8], tags: &Tags, wide: bool, ids: &[u32], out: &mut BTreeMap<u32, Option<String>>) {
    let Some(class) = tags.material else { return };
    let mut note = |id: u32, name: Option<String>| {
        let Some(name) = name else { return };
        match out.get_mut(&id) {
            None => {
                out.insert(id, Some(name));
            }
            Some(held) if held.as_deref() != Some(name.as_str()) => *held = None,
            Some(_) => {}
        }
    };
    if wide {
        for at in memchr::memmem::find_iter(buf, &[1u8, 0, 0, 0]) {
            let id_at = at + 4;
            let Some(id) = u64_at(buf, id_at).and_then(|v| u32::try_from(v).ok()).filter(|&id| declared(ids, id)) else { continue };
            let tag_at = id_at + TAG_AFTER_ID_64;
            if buf.get(tag_at..tag_at + 2) != Some(&class.to_le_bytes()[..]) {
                continue;
            }
            let object = buf.get(id_at..(id_at + NAME_WINDOW).min(buf.len())).unwrap_or_default();
            note(id, material_name(object, TAG_AFTER_ID_64, tags, true));
        }
    } else {
        // `01 00 00 00`, the id, ..., `00 00 00 ff ff ff ff` and the class tag.
        const PREFIX: [u8; 7] = [0, 0, 0, 0xff, 0xff, 0xff, 0xff];
        for tag_at in memchr::memmem::find_iter(buf, &class.to_le_bytes()) {
            let Some(id_at) = tag_at.checked_sub(TAG_AFTER_ID_32).filter(|&a| a >= 4) else { continue };
            if buf.get(id_at - 4..id_at) != Some(&[1, 0, 0, 0][..])
                || tag_at < PREFIX.len()
                || buf.get(tag_at - PREFIX.len()..tag_at) != Some(&PREFIX[..])
            {
                continue;
            }
            let Some(id) = u32_at(buf, id_at).filter(|&id| declared(ids, id)) else { continue };
            let object = buf.get(id_at..(id_at + NAME_WINDOW).min(buf.len())).unwrap_or_default();
            note(id, material_name(object, TAG_AFTER_ID_32, tags, false));
        }
    }
}
