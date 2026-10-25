//! Doors and windows: set in a wall, centred some way along it, from a sill up.
//!
//! A door's or window's **host** is a wall its reference list names. The list is in ascending ElementId order, so the host is
//! the nearest wall below the record's own id, else the nearest above (rvt-rs RE-23, RE-85); a curtain wall is not a host,
//! and a door or window whose only candidates are curtain walls is set in none. Its **hole** is the instance's box along its
//! wall: the box's extent along the wall is its width, its height is the box's, and its sill is how far the bottom of the box
//! is above the wall's base; it is centred where the box is. In a wall along an axis the box's extent along it is the width; in one
//! at an angle the width is solved from the box ([`turned`], plain geometry), except near 45° and where the box is no turned
//! rectangle. A door or window in a curved wall, or one of those, is left out and listed. Its **type** is the
//! one family symbol of its category its list names that has a name entry of the category, which names it. Which side a door
//! swings to and its hand are drawn by the family and not read: every opening has its default flips.

use std::collections::BTreeSet;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Opening, OpeningType};

use crate::import::{Facts, Import, kind, micron, mm};
use crate::inventory::Gathered;
use crate::records::Record;

/// `OST_Doors`.
pub(crate) const OST_DOORS: i64 = -2_000_023;
/// `OST_Windows`.
pub(crate) const OST_WINDOWS: i64 = -2_000_014;

/// The walls a record names that could host it, nearest first: those below its own id, nearest first, then those above.
fn candidates(r: &Record, walls: &BTreeSet<u32>) -> Vec<u32> {
    let named: Vec<u32> = r.refs.iter().filter_map(|&v| u32::try_from(v).ok()).filter(|&id| id != r.id && walls.contains(&id)).collect();
    let (below, above): (Vec<u32>, Vec<u32>) = named.iter().partition(|&&id| id < r.id);
    below.into_iter().rev().chain(above).collect()
}

/// Where a box lies along a wall: its width and the distance of its middle from the wall's start, both in millimetres. A wall that
/// runs along an axis gives them by the box's extent in that axis; a wall at an angle, by [`turned`]. None where the box gives none.
fn along(start: P2, end: P2, bbox: &[f64; 6]) -> Option<(f64, f64)> {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let (lo, hi, from, to) = if dy.abs() <= 0.01 && dx.abs() > 0.01 {
        (bbox[0], bbox[3], start.0, end.0)
    } else if dx.abs() <= 0.01 && dy.abs() > 0.01 {
        (bbox[1], bbox[4], start.1, end.1)
    } else {
        return turned(start, end, bbox);
    };
    let middle = (mm(lo) + mm(hi)) / 2.0;
    Some((micron(mm(hi) - mm(lo)), micron((middle - from) * (to - from).signum())))
}

/// How near 45° a wall may run for [`turned`]: there the two equations are the same and the width is lost in the noise.
const MIN_TURN: f64 = 0.1;

/// [`along`] for a wall at an angle. The box is axis-aligned, so it is the extent of a rectangle turned to the wall: if the wall's
/// direction is (cos, sin), its extents are width·|cos| + depth·|sin| in x and width·|sin| + depth·|cos| in y, two equations in
/// the width and the depth across the wall. Near 45° (|cos² − sin²| below [`MIN_TURN`]) they are one equation, and a box that
/// is no turned rectangle (a negative width or depth) gives none. The middle of the box is the middle of the opening.
fn turned(start: P2, end: P2, bbox: &[f64; 6]) -> Option<(f64, f64)> {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let length = dx.hypot(dy);
    if length <= 0.01 {
        return None;
    }
    let (cos, sin) = ((dx / length).abs(), (dy / length).abs());
    let det = cos * cos - sin * sin;
    if det.abs() < MIN_TURN {
        return None;
    }
    let (ex, ey) = (mm(bbox[3]) - mm(bbox[0]), mm(bbox[4]) - mm(bbox[1]));
    let (width, depth) = ((cos * ex - sin * ey) / det, (cos * ey - sin * ex) / det);
    let (x, y) = ((mm(bbox[0]) + mm(bbox[3])) / 2.0, (mm(bbox[1]) + mm(bbox[4])) / 2.0);
    let offset = ((x - start.0) * dx + (y - start.1) * dy) / length;
    (width > 0.0 && depth > 0.0 && offset.is_finite()).then(|| (micron(width), micron(offset)))
}

pub(crate) fn wanted(f: &Facts, out: &mut BTreeSet<u32>) {
    // Nothing of an opening is read from its data: its record and its type's name are all.
    let _ = (f, out);
}

/// The type of a family instance: the one family symbol of its category its reference list names that has a name entry of the
/// category, and the name. None where it names none, or two with different names.
pub(crate) fn symbol_of(r: &Record, symbols: &BTreeSet<u32>, g: &Gathered, category: i64) -> Option<(u32, String)> {
    let mut named = r
        .refs
        .iter()
        .filter_map(|&v| u32::try_from(v).ok())
        .filter(|id| *id != r.id && symbols.contains(id))
        .filter_map(|id| g.names.get(&id).filter(|(_, c)| c.is_none_or(|c| c == category)).map(|(n, _)| (id, n.clone())))
        .collect::<Vec<_>>();
    named.sort();
    named.dedup();
    let names: BTreeSet<&String> = named.iter().map(|(_, n)| n).collect();
    match (named.first(), names.len()) {
        (Some(first), 1) => Some(first.clone()),
        _ => None,
    }
}

/// The name of the type of a family instance ([`symbol_of`]).
pub(crate) fn symbol_name(r: &Record, symbols: &BTreeSet<u32>, g: &Gathered, category: i64) -> Option<String> {
    symbol_of(r, symbols, g, category).map(|(_, n)| n)
}

/// A door or window read from its record, before the hole it makes is fitted among its neighbours'.
struct Pending {
    id: u32,
    door: bool,
    host: bimcraft_core::model::Id,
    name: String,
    /// The hole along the wall: from and to, in millimetres from the wall's start.
    span: (f64, f64),
    height: f64,
    sill: f64,
    /// Whether the hole was cut down to what its wall gives, its box overrunning it.
    clamped: bool,
}

/// The most two neighbouring holes may overlap and be fitted, as a share of the narrower one: Revit lets windows overlap, BIMCraft
/// does not.
const MAX_FITTED_OVERLAP: f64 = 0.1;

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    let walls: BTreeSet<u32> = f.of_category(crate::walls::OST_WALLS).map(|r| r.id).collect();
    let mut pending: Vec<Pending> = Vec::new();
    for (category, label) in [(OST_DOORS, "door"), (OST_WINDOWS, "window")] {
        let symbols = f.symbols.get(&category).cloned().unwrap_or_default();
        for r in f.of_category(category) {
            let read = (|| -> Result<Pending, String> {
                let named = candidates(r, &walls);
                if named.is_empty() {
                    return Err("its record names no wall".into());
                }
                // The host is the nearest wall the list names that the opening fits in; failing that, the nearest it names at
                // all, so that the model's rule says why (a door that hangs off the end of its wall, say).
                let mut first: Option<(u32, bimcraft_core::model::Id, f64, f64, f64, f64)> = None;
                let mut fits = None;
                let mut unreadable: Option<String> = None;
                for host in named.iter().filter(|w| !cx.curtain.contains(w)) {
                    let Some(host_id) = cx.walls.get(host).copied() else { continue };
                    let Some(wall) = cx.model.wall(host_id) else { continue };
                    if wall.arc != 0.0 {
                        unreadable.get_or_insert(format!("its host wall {host} is curved, and a curved wall's box gives no width"));
                        continue;
                    }
                    let Some((width, offset)) = along(wall.start, wall.end, &r.bbox) else {
                        unreadable.get_or_insert(format!(
                            "its host wall {host} runs at an angle, and the box of a door or window in it gives no width"
                        ));
                        continue;
                    };
                    let height = micron(mm(r.bbox[5]) - mm(r.bbox[2]));
                    let sill = micron(mm(r.bbox[2]) - cx.model.own_base(wall));
                    let sill = if (-2.0..2.0).contains(&sill) { 0.0 } else { sill };
                    let fits_here = offset - width / 2.0 >= -1.0
                        && offset + width / 2.0 <= wall.start.dist(wall.end) + 1.0
                        && sill >= 0.0
                        && cx.model.own_base(wall) + sill + height <= cx.model.top_z(wall) + 1.0;
                    let candidate = (*host, host_id, width, offset, height, sill);
                    if first.is_none() {
                        first = Some(candidate);
                    }
                    if fits_here {
                        fits = Some(candidate);
                        break;
                    }
                }
                let (host, host_id, width, mut offset, mut height, sill) = match fits.or(first) {
                    Some(c) => c,
                    None => {
                        if let Some(why) = unreadable {
                            return Err(why);
                        }
                        return Err(if named.iter().any(|w| !cx.curtain.contains(w)) {
                            format!("its host wall {} was left out", named[0])
                        } else {
                            "its host is a curtain wall, which BIMCraft has no element for".to_string()
                        });
                    }
                };
                let _ = host;
                let wall = cx.model.wall(host_id).ok_or("its host wall is gone")?;
                // A box that overruns its wall — a door taller than the wall's box, a window past its end — comes in as far as
                // the wall goes, and the note says so.
                let (long, tall) = (wall.start.dist(wall.end), cx.model.top_z(wall) - cx.model.own_base(wall));
                let width = if width > long { width.min(long) } else { width };
                let overruns = (offset - width / 2.0) < -1.0 || (offset + width / 2.0) > long + 1.0 || sill + height > tall + 1.0;
                if (offset - width / 2.0) < 0.0 {
                    offset = width / 2.0;
                }
                if offset + width / 2.0 > long {
                    offset = long - width / 2.0;
                }
                if sill + height > tall {
                    height = tall - sill;
                }
                if width <= 0.0 || height <= 0.0 {
                    return Err("its wall leaves no room for it".into());
                }
                let name = symbol_name(r, &symbols, g, category).unwrap_or_else(|| format!("Imported {label}"));
                Ok(Pending {
                    id: r.id,
                    door: label == "door",
                    host: host_id,
                    name,
                    span: (offset - width / 2.0, offset + width / 2.0),
                    height,
                    sill,
                    clamped: overruns,
                })
            })();
            match read {
                Ok(p) => pending.push(p),
                Err(why) => cx.left_out(label, r.id, &why),
            }
        }
    }
    // Neighbours in a wall that overlap a little are fitted: each gives up half the overlap.
    let mut order: Vec<usize> = (0..pending.len()).collect();
    order.sort_by(|&a, &b| {
        (pending[a].host, pending[a].span.0).partial_cmp(&(pending[b].host, pending[b].span.0)).unwrap_or(std::cmp::Ordering::Equal)
    });
    for pair in order.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if pending[a].host != pending[b].host {
            continue;
        }
        let overlap = pending[a].span.1 - pending[b].span.0;
        let narrower = (pending[a].span.1 - pending[a].span.0).min(pending[b].span.1 - pending[b].span.0);
        if overlap > 0.0 && overlap <= MAX_FITTED_OVERLAP * narrower {
            let middle = micron((pending[a].span.1 + pending[b].span.0) / 2.0);
            pending[a].span.1 = middle;
            pending[b].span.0 = middle;
            for id in [pending[a].id, pending[b].id] {
                cx.note(
                    "opening",
                    id,
                    "it overlaps a neighbour in its wall by up to a tenth of its width, which BIMCraft does not allow: narrowed to fit",
                );
            }
        }
    }
    for p in pending {
        let (width, offset) = (micron(p.span.1 - p.span.0), micron((p.span.0 + p.span.1) / 2.0));
        let types = if p.door { &mut cx.model.types.door } else { &mut cx.model.types.window };
        let type_name = kind(
            types,
            &p.name,
            |t| (t.width - width).abs() < 0.05 && (t.height - p.height).abs() < 0.05,
            OpeningType { width, height: p.height, sill: p.sill },
        );
        let opening = Opening {
            type_name,
            family: None,
            host: cx.model.guid(p.host),
            offset,
            sill: p.sill,
            mark: String::new(),
            flip_facing: false,
            flip_hand: false,
            params: Default::default(),
        };
        if p.clamped {
            cx.note("opening", p.id, "its box overruns its wall, and it comes in as far as the wall goes");
        }
        let (element, label) = if p.door { (Element::Door(opening), "Door") } else { (Element::Window(opening), "Window") };
        let id = cx.model.insert(element);
        cx.from.insert(id, format!("{label} {}", p.id));
    }
}
