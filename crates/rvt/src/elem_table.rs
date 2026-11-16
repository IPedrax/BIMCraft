// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/elem_table.rs (the record layouts of
// `Global/ElemTable` and the rule that picks one, which rvt-rs credits to STE1200's frame measured by puzzbobb).
// Changed: rewritten for BIMCraft; only the ElementIds are read (not the owners), 12/28/40-byte layouts are supported,
// either column may be the one that rises and both are listed, and every count and offset is checked before it is used.
//! `Global/ElemTable`: every ElementId in the file, in ascending order.
//!
//! After a `u16` (the tag of the class `ElemTable` in the file's schema) and a `u32` record count at byte 2, the records
//! start at byte 6. A record is 28 bytes up to Revit 2023 (`[u32 id][u32 id][16 bytes][u32 owner]`) and 40 bytes from 2024
//! (`[u64 id][12 bytes][u64 id][u64 owner][u32]`). The two ids are equal in a file that is not workshared; in a workshared one
//! they differ in many records, and an element's own record carries either, so both are listed. The size is found, not assumed:
//! the one at which the stated number of records fits and the ElementIds of one of the two columns rise strictly all the way,
//! and if both fit, the one that leaves the shorter tail.

use crate::error::{Error, Result};

const ORIGIN: usize = 6;
/// `(record size, offsets of an ElementId)`, longest first: the second id, then the first. The layout is the one in which either
/// column rises strictly, and both columns are listed: they are equal in a file that is not workshared, and in a workshared one the
/// second ids may repeat, fall and differ from the first in many records, an element's record carrying either.
const LAYOUTS: [(usize, [usize; 2]); 3] = [(40, [20, 0]), (28, [4, 0]), (12, [4, 0])];

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Whether the ids at `id_at` of the `count` records of `size` bytes rise strictly all the way. The first may be 0 (the Snowdon
/// Towers structural sample's is); every one after it is then above 0.
fn rises(data: &[u8], count: usize, size: usize, id_at: usize) -> bool {
    let mut previous: Option<u32> = None;
    (0..count).all(|k| match u32_at(data, ORIGIN + k * size + id_at) {
        Some(id) if id < u32::MAX && previous.is_none_or(|p| id > p) => {
            previous = Some(id);
            true
        }
        _ => false,
    })
}

/// The ElementIds the inflated `Global/ElemTable` declares, ascending, of both columns.
pub fn parse(data: &[u8]) -> Result<Vec<u32>> {
    let bad = |why: &str| Error::Format(format!("Global/ElemTable {why}"));
    let count = u32_at(data, 2).ok_or_else(|| bad("is too short to hold its record count"))? as usize;
    if count == 0 {
        return Err(bad("lists no elements"));
    }
    let mut best: Option<(usize, usize, [usize; 2])> = None; // (tail, size, id offsets)
    for (size, id_ats) in LAYOUTS {
        let Some(end) = count.checked_mul(size).and_then(|n| n.checked_add(ORIGIN)).filter(|&end| end <= data.len()) else {
            continue;
        };
        if !id_ats.into_iter().any(|id_at| rises(data, count, size, id_at)) {
            continue;
        }
        if best.is_none_or(|(tail, ..)| data.len() - end < tail) {
            best = Some((data.len() - end, size, id_ats));
        }
    }
    let (_, size, id_ats) = best.ok_or_else(|| {
        bad(&format!("has no record layout (12, 28 or 40 bytes) in which its {count} records fit with ElementIds that rise"))
    })?;
    let mut ids: Vec<u32> = (0..count)
        .flat_map(|k| id_ats.map(|id_at| u32_at(data, ORIGIN + k * size + id_at)))
        .flatten()
        .filter(|&id| id > 0 && id < u32::MAX)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

impl crate::Revit {
    /// The ElementIds `Global/ElemTable` declares.
    pub fn element_ids(&mut self) -> Result<Vec<u32>> {
        parse(&self.inflate("Global/ElemTable")?.bytes)
    }
}
