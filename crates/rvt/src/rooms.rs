//! Rooms: a name and a number at a level, and a point inside the walls that enclose them.
//!
//! A room's **name** and **number** are parameter entries in its object (see [`crate::data`]); its **level** is read as a wall's
//! is, from the levels its reference list names. BIMCraft finds a room's boundary from the walls round a point of it, so a room's
//! **point** is a point of its record's box that the imported walls enclose: the middle of the box if they do, else the nearest
//! point of a grid over the box that they do. The walls must enclose a region whose box is the room's box to within the walls'
//! thickness (600 mm); a room no walls enclose (Revit's own not-enclosed room, or one bounded by room separation lines, which
//! BIMCraft has none of) comes in at the middle of its box, and the report says so. Making the regions of a level is what costs, and a room tries a grid
//! of points, so a level's faces are made once for all its rooms.

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_core::geom::{self, P2};
use bimcraft_core::ids::Guid;
use bimcraft_core::model::{Element, Room, TOL};

use crate::import::{Facts, Import, clip, point};
use crate::inventory::{Gathered, Wants};
use crate::records::Record;
use crate::walls::constraints;

/// `OST_Rooms`.
pub(crate) const OST_ROOMS: i64 = -2_000_160;
/// How far a region's box may differ from the room's, in millimetres.
const SLACK: f64 = 600.0;
/// Points tried along each side of the room's box.
const GRID: usize = 9;

pub(crate) fn wanted(f: &Facts, out: &mut Wants) {
    out.rooms.extend(f.of_category(OST_ROOMS).map(|r| r.id));
}

/// A point of `room`'s box in plan that the walls of its level (`all`, the faces they make) enclose in a region as big as the
/// box, the nearest to its middle.
fn point_in(all: &[Vec<P2>], r: &Record) -> Option<P2> {
    let (lo, hi) = (point(r.bbox[0], r.bbox[1]), point(r.bbox[3], r.bbox[4]));
    let middle = P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
    let mut candidates: Vec<P2> = (0..GRID * GRID)
        .map(|k| {
            let (i, j) = ((k % GRID) as f64 + 0.5, (k / GRID) as f64 + 0.5);
            P2(lo.0 + (hi.0 - lo.0) * i / GRID as f64, lo.1 + (hi.1 - lo.1) * j / GRID as f64)
        })
        .collect();
    candidates.sort_by(|a, b| a.dist(middle).total_cmp(&b.dist(middle)));
    std::iter::once(middle).chain(candidates).find(|p| {
        let Some(region) = geom::region_in(all, *p, TOL) else { return false };
        let (mut a, mut b) = (P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN));
        for q in &region.outer {
            (a, b) = (P2(a.0.min(q.0), a.1.min(q.1)), P2(b.0.max(q.0), b.1.max(q.1)));
        }
        let near = |x: f64, y: f64| (x - y).abs() <= SLACK;
        near(a.0, lo.0) && near(a.1, lo.1) && near(b.0, hi.0) && near(b.1, hi.1) && geom::area(&region.outer).abs() > 0.0
    })
}

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    // The faces of each level's walls, made once for all its rooms: a room tries a grid of points, and the walls of a level do
    // not change as rooms come in.
    let mut faces: BTreeMap<Guid, Vec<Vec<P2>>> = BTreeMap::new();
    for r in f.of_category(OST_ROOMS) {
        if !seen.insert(r.id) {
            continue;
        }
        let made = (|| -> Result<Room, String> {
            let (base, _) = constraints(r, &f.elevations);
            let level = base.and_then(|b| cx.levels.get(&b).copied()).ok_or("it stands on no level")?;
            let all = faces.entry(level).or_insert_with(|| geom::faces(&cx.model.bounding_lines(level), TOL));
            // A room no walls enclose — Revit's own "not enclosed" room, or one whose sides are room separation lines, which BIMCraft
            // has none of — comes in at the middle of its box: it is a room in the file, with a name and a number, and the plan
            // draws whatever boundary the walls there make. The note says that.
            let (point, enclosed) = match point_in(all, r) {
                Some(p) => (p, true),
                None => {
                    let (lo, hi) = (point(r.bbox[0], r.bbox[1]), point(r.bbox[3], r.bbox[4]));
                    (P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0), false)
                }
            };
            if !enclosed {
                cx.note(
                    "room",
                    r.id,
                    "no walls enclose its box (Revit's own not-enclosed room, or one bounded by room separation lines, which BIMCraft has none of): it comes in at the middle of its box",
                );
            }
            let (number, name) = g.rooms.get(&r.id).cloned().unwrap_or_default();
            let name = if name.trim().is_empty() { "Room".to_string() } else { clip(&name) };
            Ok(Room { level, point, name, number: clip(&number), params: Default::default() })
        })();
        match made {
            Ok(room) => {
                let id = cx.model.insert(Element::Room(room));
                cx.from.insert(id, format!("Room {}", r.id));
            }
            Err(why) => {
                if std::env::var("BIMCRAFT_DIAG_ROOMS").is_ok() {
                    let (base, _) = constraints(r, &f.elevations);
                    let level = base.and_then(|b| cx.levels.get(&b).copied());
                    let walls = level.map(|l| cx.model.bounding_lines(l).len()).unwrap_or(0);
                    let boxed = (point(r.bbox[0], r.bbox[1]), point(r.bbox[3], r.bbox[4]));
                    eprintln!("room {} level {:?} walls {} box {:?}", r.id, level, walls, boxed);
                }
                cx.left_out("room", r.id, &why)
            }
        }
    }
}
