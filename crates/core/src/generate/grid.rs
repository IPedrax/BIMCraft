//! The gridlines of an outline: a line at each corner's x and y, numbered and lettered, as both engines draw them.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

pub(crate) fn grid_name(mut i: usize, letters: bool) -> String {
    if !letters {
        return (i + 1).to_string();
    }
    let mut name = String::new();
    loop {
        let c = (b'A' + (i % 26) as u8) as char;
        name.insert(0, c);
        if i < 26 {
            return name;
        }
        i = i / 26 - 1;
    }
}

/// How far a plan's gridlines run past its bounding box, mm.
const GRID_PAST: i64 = 1_500;

/// The corners of an outline given as a closed ring of points: the vertices where the direction changes.
pub(crate) fn ring_corners(ring: &[[i64; 2]]) -> Vec<[i64; 2]> {
    let n = ring.len();
    (0..n)
        .filter(|&k| {
            let (p, q, r) = (ring[(k + n - 1) % n], ring[k], ring[(k + 1) % n]);
            (q[0] - p[0]) * (r[1] - q[1]) - (q[1] - p[1]) * (r[0] - q[0]) != 0
        })
        .map(|k| ring[k])
        .collect()
}

/// The corners of an outline drawn as separate wall segments: a point where a horizontal and a vertical segment meet. A point
/// where a segment ends on a straight run (a collinear join) is not a corner.
pub(crate) fn segment_corners(segments: &[([i64; 2], [i64; 2])]) -> Vec<[i64; 2]> {
    let mut seen: BTreeMap<[i64; 2], (bool, bool)> = BTreeMap::new();
    for (a, b) in segments {
        let horizontal = a[1] == b[1];
        for p in [*a, *b] {
            let e = seen.entry(p).or_default();
            if horizontal {
                e.0 = true;
            } else {
                e.1 = true;
            }
        }
    }
    seen.into_iter().filter(|(_, (h, v))| *h && *v).map(|(p, _)| p).collect()
}

/// The params of the `grid` lines of an outline's corners: a vertical line at each distinct x, numbered from 1 left to right,
/// and a horizontal one at each distinct y, lettered from A up, each running 1,500 mm past the corners' bounding box.
pub(crate) fn outline_grids(corners: &[[i64; 2]]) -> Vec<Value> {
    let xs: BTreeSet<i64> = corners.iter().map(|p| p[0]).collect();
    let ys: BTreeSet<i64> = corners.iter().map(|p| p[1]).collect();
    let (Some(x0), Some(x1), Some(y0), Some(y1)) = (xs.first(), xs.last(), ys.first(), ys.last()) else { return Vec::new() };
    let mut out = Vec::new();
    for (i, x) in xs.iter().enumerate() {
        out.push(json!({"start": [x, y0 - GRID_PAST], "end": [x, y1 + GRID_PAST], "name": grid_name(i, false)}));
    }
    for (i, y) in ys.iter().enumerate() {
        out.push(json!({"start": [x0 - GRID_PAST, y], "end": [x1 + GRID_PAST, y], "name": grid_name(i, true)}));
    }
    out
}
