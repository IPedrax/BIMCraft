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

use crate::import::{Facts, Import, clip, kind, micron, mm};
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

pub(crate) fn wanted(f: &Facts, out: &mut crate::inventory::Wants) {
    // Nothing of an opening is read from its data but its placement: its record, its type's name, and what its graphics draw.
    let mine: Vec<u32> = [OST_DOORS, OST_WINDOWS].into_iter().flat_map(|c| f.of_category(c)).map(|r| r.id).collect();
    out.placements.extend(&mine);
    out.drawings.extend(mine);
}

/// The type of a family instance: the one family symbol of its category its reference list names that has a name entry of the
/// category, and the name. None where it names none, or two with different names.
///
/// From Revit 2025 ([`NAMED_TYPES`]) a type has no record of its own, so there is no symbol to find: there the type is the one id of
/// the first list with a name entry that names the instance's own category (one that says none is not taken, since it could be any
/// family's or type's). Before 2025 only a symbol is taken: the finding was measured on 2025 files alone.
pub(crate) fn symbol_of(r: &Record, symbols: &BTreeSet<u32>, g: &Gathered, category: i64, release: u32) -> Option<(u32, String)> {
    one_named(r, &r.refs, g, |id, c| symbols.contains(&id) && c.is_none_or(|c| c == category))
        .or_else(|| if release >= NAMED_TYPES { one_named(r, &r.refs, g, |_, c| c == Some(category)) } else { None })
}

/// The first release whose family types are known by their name entries alone (the 2025 finding in docs/rvt-provenance.md).
const NAMED_TYPES: u32 = 2025;

/// The family of a family instance as Revit 2025 writes it: the one id of the second reference list with a name entry of the
/// instance's own category, and the name. None before 2025, where nothing says what the second list names.
pub(crate) fn family_of(r: &Record, g: &Gathered, category: i64, release: u32) -> Option<(u32, String)> {
    if release < NAMED_TYPES {
        return None;
    }
    one_named(r, &r.refs2, g, |_, c| c == Some(category))
}

/// The one name among the ids of `list` (other than `r`'s own) that have a name entry `takes` takes, by id and the entry's category,
/// with the first id of that name. None where none is, or two names are.
fn one_named(r: &Record, list: &[u64], g: &Gathered, takes: impl Fn(u32, Option<i64>) -> bool) -> Option<(u32, String)> {
    let mut named = list
        .iter()
        .filter_map(|&v| u32::try_from(v).ok())
        .filter(|id| *id != r.id)
        .filter_map(|id| g.names.get(&id).filter(|(_, c)| takes(id, *c)).map(|(n, _)| (id, n.clone())))
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
pub(crate) fn symbol_name(r: &Record, symbols: &BTreeSet<u32>, g: &Gathered, category: i64, release: u32) -> Option<String> {
    symbol_of(r, symbols, g, category, release).map(|(_, n)| n)
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

/// The families the doors and windows that draw a shape come in with: by category and Revit family, the model's name of each, and
/// by family and Revit type, the shapes and sizes each of its types holds (the first the Revit type's name, the next numbered on).
#[derive(Default)]
struct OpeningFamilies {
    names: std::collections::BTreeMap<(bool, Option<u32>), String>,
    variants: std::collections::BTreeMap<(String, String), Vec<Variant>>,
}

/// A type's shape in its opening's axes, and its width and height, mm.
type Variant = (Vec<[[f64; 3]; 3]>, f64, f64);

impl OpeningFamilies {
    /// The family and type opening `p` (record `r`, `width` wide at `offset` along its wall) comes in with, its shape what its
    /// graphics draw in the opening's own axes (x along its wall from its middle, y across from the wall's middle plane toward
    /// the wall's left, its exterior, z up from its sill); or why not.
    fn shaped(
        &mut self,
        cx: &mut Import,
        f: &Facts,
        g: &Gathered,
        r: &Record,
        p: &Pending,
        (width, offset): (f64, f64),
    ) -> Result<(String, String), String> {
        let drawn = crate::generic::drawn_where_it_stands(g, r)?;
        let wall = cx.model.wall(p.host).ok_or("its host wall is gone")?;
        let (start, end) = (wall.start, wall.end);
        let long = start.dist(end);
        if long <= 0.0 {
            return Err("its host wall has no length".into());
        }
        let x = [(end.0 - start.0) / long, (end.1 - start.1) / long];
        let y = [-x[1], x[0]];
        let origin = [start.0 + x[0] * offset, start.1 + x[1] * offset, cx.model.own_base(wall) + p.sill];
        let local = |q: [f64; 3]| -> [f64; 3] {
            let d = [q[0] - origin[0], q[1] - origin[1]];
            [micron(d[0] * x[0] + d[1] * x[1]), micron(d[0] * y[0] + d[1] * y[1]), micron(q[2] - origin[2])]
        };
        let triangles: Vec<[[f64; 3]; 3]> = drawn.triangles.iter().map(|t| t.map(local)).collect();
        let category = if p.door { OST_DOORS } else { OST_WINDOWS };
        let revit = crate::openings::family_of(r, g, category, f.release);
        let name = match self.names.get(&(p.door, revit.as_ref().map(|(id, _)| *id))) {
            Some(n) => n.clone(),
            None => {
                let base = revit
                    .as_ref()
                    .map(|(_, n)| clip(n))
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| if p.door { "Doors".to_string() } else { "Windows".to_string() });
                let mut taken: BTreeSet<String> = cx.model.families.keys().cloned().collect();
                let n = crate::import::fresh(&mut taken, &base);
                self.names.insert((p.door, revit.as_ref().map(|(id, _)| *id)), n.clone());
                n
            }
        };
        let held = self.variants.entry((name.clone(), p.name.clone())).or_default();
        let k = match held.iter().position(|(t, w, h)| *t == triangles && (w - width).abs() < 0.05 && (h - p.height).abs() < 0.05) {
            Some(k) => k,
            None => {
                held.push((triangles.clone(), width, p.height));
                held.len() - 1
            }
        };
        let type_name = if k == 0 { p.name.clone() } else { clip(&format!("{} ({})", p.name, k + 1)) };
        let family = cx.model.families.entry(name.clone()).or_insert_with(|| opening_family(p.door));
        if family.types.contains_key(&type_name) {
            return Ok((name, type_name));
        }
        let solid = crate::shapes::Shape { triangles, materials: drawn.materials, ..crate::shapes::Shape::default() };
        if !crate::generic::show(family, &type_name, &solid, &g.materials) {
            family.types.remove(&type_name);
            return Err("its family holds as many shapes as one family may".into());
        }
        let values = family.types.entry(type_name.clone()).or_default();
        values.insert("Width".to_string(), width.into());
        values.insert("Height".to_string(), p.height.into());
        Ok((name, type_name))
    }
}

/// A door's or a window's family whose types are Revit's: its opening's width and height, each type's own, and no form but the
/// shapes its types show.
fn opening_family(door: bool) -> bimcraft_core::family::Family {
    use bimcraft_core::family::{Category, Family, Kind, Parameter};
    let length = |name: &str| Parameter { name: name.to_string(), kind: Kind::Length, value: 1000.0.into(), ..Parameter::default() };
    Family {
        category: if door { Category::Door } else { Category::Window },
        parameters: vec![length("Width"), length("Height")],
        ..Family::default()
    }
}

/// The most two neighbouring holes may overlap and be fitted, as a share of the narrower one: Revit lets windows overlap, BIMCraft
/// does not.
const MAX_FITTED_OVERLAP: f64 = 0.1;

/// The doors and windows whose nearest wall is one the parts of a curtain wall name as their host: those that may come in as
/// components (`Import::in_curtain` says which do), whose graphics are read for it.
pub(crate) fn in_curtain(f: &Facts) -> Vec<u32> {
    let walls: BTreeSet<u32> = f.of_category(crate::walls::OST_WALLS).map(|r| r.id).collect();
    let hosts = crate::walls::curtain_hosts(f);
    [OST_DOORS, OST_WINDOWS]
        .into_iter()
        .flat_map(|c| f.of_category(c))
        .filter(|r| candidates(r, &walls).first().is_some_and(|w| hosts.contains(w)))
        .map(|r| r.id)
        .collect()
}

/// Why a door or a window whose host is a curtain wall is not set in it.
const CURTAIN_HOST: &str = "its host is a curtain wall, which holds a door or a window only in a wall set in its cell";

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    let walls: BTreeSet<u32> = f.of_category(crate::walls::OST_WALLS).map(|r| r.id).collect();
    let mut pending: Vec<Pending> = Vec::new();
    for (category, label) in [(OST_DOORS, "door"), (OST_WINDOWS, "window")] {
        let symbols = f.symbols.get(&category).cloned().unwrap_or_default();
        for r in f.of_category(category) {
            let read = (|| -> Result<Pending, String> {
                let named = candidates(r, &walls);
                if named.is_empty() {
                    // A window set in a roof or a floor is a skylight: BIMCraft's windows stand in walls, and it has no opening
                    // for another plane, so it is left out.
                    let planes = [crate::roofs::OST_ROOFS, crate::floors::OST_FLOORS, crate::floors::OST_CEILINGS];
                    if planes.iter().any(|c| f.of_category(*c).any(|o| r.refs.contains(&u64::from(o.id)))) {
                        return Err("it is set in a roof or a floor (a skylight), and BIMCraft's windows stand in walls".into());
                    }
                    return Err("its record names no wall".into());
                }
                // The host is the nearest wall the list names (rvt-rs RE-23, RE-85) that the opening fits in; failing one, the
                // nearest it names, and then the hole is cut down to what that wall gives. A wall whose box cuts the opening
                // short is the file's own inconsistency: Revit draws the frame a little past the wall's box.
                // The nearest a curtain wall: it is the host, and a BIMCraft curtain wall holds a door or a window only in a wall
                // set in its cell, so it is set in no other wall.
                if named.first().is_some_and(|w| cx.curtain.contains(w)) {
                    return Err(CURTAIN_HOST.into());
                }
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
                    let (long, tall) = (wall.start.dist(wall.end), cx.model.top_z(wall) - cx.model.own_base(wall));
                    let fits_here =
                        offset - width / 2.0 >= -1.0 && offset + width / 2.0 <= long + 1.0 && sill >= 0.0 && sill + height <= tall + 1.0;
                    let candidate = (*host, host_id, width, offset, height, sill);
                    first.get_or_insert(candidate);
                    if fits_here {
                        fits = Some(candidate);
                        break;
                    }
                }
                let (host, host_id, width, mut offset, height, sill) = match fits.or(first) {
                    Some(c) => c,
                    None => {
                        if let Some(why) = unreadable {
                            return Err(why);
                        }
                        return Err(if named.iter().any(|w| !cx.curtain.contains(w)) {
                            format!("its host wall {} was left out", named[0])
                        } else {
                            CURTAIN_HOST.to_string()
                        });
                    }
                };
                let wall = cx.model.wall(host_id).ok_or("its host wall is gone")?;
                let (long, tall) = (wall.start.dist(wall.end), cx.model.top_z(wall) - cx.model.own_base(wall));
                let sill = sill.max(0.0);
                let overruns = (offset - width / 2.0) < -1.0 || (offset + width / 2.0) > long + 1.0 || sill + height > tall + 1.0;
                let width = width.min(long);
                if offset - width / 2.0 < 0.0 {
                    offset = width / 2.0;
                }
                if offset + width / 2.0 > long {
                    offset = long - width / 2.0;
                }
                let height = height.min((tall - sill).max(0.0));
                if width <= 0.0 || height <= 0.0 {
                    return Err(format!("its host wall {host} leaves no room for it"));
                }
                let name = symbol_name(r, &symbols, g, category, f.release).unwrap_or_else(|| format!("Imported {label}"));
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
                // A door or a window in a curtain wall comes in as a component of its family, drawn as Revit draws it.
                Err(why) if why == CURTAIN_HOST => {
                    cx.note(label, r.id, &format!("{why}: it comes in as a component of its family"));
                    cx.in_curtain.insert(r.id);
                }
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
    let mut families = OpeningFamilies::default();
    for p in pending {
        let (width, offset) = (micron(p.span.1 - p.span.0), micron((p.span.0 + p.span.1) / 2.0));
        let label = if p.door { "door" } else { "window" };
        // Its shape, where its graphics draw one that reads: a family of its own, drawn as Revit draws it; else a type of the
        // project's own, the size of its box.
        let shaped = f
            .current
            .get(&p.id)
            .ok_or_else(|| "its record is gone".to_string())
            .and_then(|r| families.shaped(cx, f, g, r, &p, (width, offset)));
        let (type_name, family) = match shaped {
            Ok((family, type_name)) => {
                cx.note(label, p.id, "comes in with its shape, read from the file, in a family of its own");
                (type_name, Some(family))
            }
            Err(why) => {
                // One whose graphics are not in the file at all is an opening of its box, as ever, said of none.
                if why != crate::generic::NO_GRAPHICS {
                    let why = crate::generic::short(&why);
                    cx.note(label, p.id, &format!("comes in as an opening of its box: its shape is not read: {why}"));
                }
                let types = if p.door { &mut cx.model.types.door } else { &mut cx.model.types.window };
                let type_name = kind(
                    types,
                    &p.name,
                    |t| (t.width - width).abs() < 0.05 && (t.height - p.height).abs() < 0.05,
                    OpeningType { width, height: p.height, sill: p.sill, operation: None },
                );
                (type_name, None)
            }
        };
        let opening = Opening {
            type_name,
            family,
            host: cx.model.guid(p.host),
            offset,
            sill: p.sill,
            mark: String::new(),
            flip_facing: false,
            flip_hand: false,
            slanted: false,
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
