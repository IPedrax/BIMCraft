//! Which way the house generator's swing doors are set to open, and where their tags go, by the rules in
//! `crate::generate::swing`.

pub use crate::generate::swing::*;

use super::HouseGraph;
use super::place::{Layout, Rect};

fn holds(r: &Rect, p: [f64; 2]) -> bool {
    p[0] > r.x0 as f64 && p[0] < r.x1 as f64 && p[1] > r.y0 as f64 && p[1] < r.y1 as f64
}

/// Sets each swing door's flips, and what the extractor will say of it, from the rules. A door whose host wall is not found
/// exactly once is an error, so the try is skipped.
pub fn orient(g: &HouseGraph, l: &mut Layout) -> Result<(), String> {
    let exterior = exterior_rule();
    for i in 0..l.doors.len() {
        let at = [l.doors[i].at[0] as f64, l.doors[i].at[1] as f64];
        let hosts: Vec<_> = l
            .walls
            .iter()
            .filter(|w| {
                let (a, b) = ([w.a[0] as f64, w.a[1] as f64], [w.b[0] as f64, w.b[1] as f64]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = dx.hypot(dy);
                if len < 1.0 {
                    return false;
                }
                let s = ((at[0] - a[0]) * dx + (at[1] - a[1]) * dy) / len;
                let d = ((at[0] - a[0]) * dy - (at[1] - a[1]) * dx).abs() / len;
                d <= 1.0 && s >= 0.0 && s <= len
            })
            .collect();
        let swings = l.doors[i].op == "swing";
        let [w] = hosts.as_slice() else {
            if swings {
                return Err(format!("door D{} has {} host walls under it, not one", i + 1, hosts.len()));
            }
            continue;
        };
        let (a, b) = ([w.a[0] as f64, w.a[1] as f64], [w.b[0] as f64, w.b[1] as f64]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let n = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
        let half = if w.exterior { 150.0 } else { 50.0 };
        l.doors[i].normal = n;
        l.doors[i].half = half;
        if !swings {
            l.doors[i].opens_into = None;
            l.doors[i].hinge = None;
            continue;
        }
        let k = half + 150.0;
        let room_at = |sign: f64| {
            let p = [at[0] + sign * n[0] * k, at[1] + sign * n[1] * k];
            l.rects.iter().position(|r| holds(r, p))
        };
        let (left, right) = (room_at(1.0), room_at(-1.0));
        // The default is the left side and the start jamb; a rule may ask for the other. The target is the room (None:
        // outside) the door should open into, when a rule enforces one.
        let mut hinge_end = false;
        let mut target: Option<Option<usize>> = None;
        match (left, right) {
            (Some(x), Some(y)) if x != y => {
                let (kx, ky) = (g.rooms[x].kind.name(), g.rooms[y].kind.name());
                if let Some(r) = rule_for(kx, ky) {
                    hinge_end = r.hinge == "end";
                    let rk = r.opens_into.as_str();
                    if kx != ky && rk == kx {
                        target = Some(Some(x));
                    } else if kx != ky && rk == ky {
                        target = Some(Some(y));
                    }
                }
            }
            (Some(x), None) | (None, Some(x)) => {
                if let Some(r) = exterior {
                    hinge_end = r.hinge == "end";
                    target = match r.opens_into.as_str() {
                        "either" => None,
                        "exterior" => Some(None),
                        _ => Some(Some(x)),
                    };
                }
            }
            _ => {}
        }
        let face = target.is_some_and(|t| t != left);
        let side = if face { right } else { left };
        let d = &mut l.doors[i];
        d.flip_facing = face;
        d.flip_hand = hinge_end;
        d.opens_into = Some(side.map_or("exterior".to_string(), |r| g.rooms[r].id.clone()));
        d.hinge = Some(if hinge_end { "end" } else { "start" });
    }
    Ok(())
}

/// How far a tag's text reaches from the point it names, mm: a circle is cruder than its box and safe.
const TAG_REACH: f64 = 380.0;

/// A door tag the default offset puts under a swung leaf, moved: `(the door's index, the new offset)`. A tag the leaf of a
/// door nearby would cover is hidden by its mask and clips the leaf, so each is tried on either side of its wall at growing
/// distances, and the first that keeps clear of every leaf is taken.
pub fn tag_moves(l: &Layout) -> Vec<(usize, [f64; 2])> {
    let doors = &l.doors;
    // Each swing leaf as a box in its wall's frame: along the wall, and out from its centre line on the swing side.
    let leaf_hit = |j: usize, p: [f64; 2]| {
        let d = &doors[j];
        if d.op != "swing" || d.normal == [0.0, 0.0] {
            return false;
        }
        let u = [d.normal[1], -d.normal[0]];
        let (dx, dy) = (p[0] - d.at[0] as f64, p[1] - d.at[1] as f64);
        let (along, out) = (dx * u[0] + dy * u[1], dx * d.normal[0] + dy * d.normal[1]);
        let w = d.width as f64;
        let side = if d.flip_facing { -1.0 } else { 1.0 };
        let out = out * side;
        let (ax, ay) = ((along.abs() - w / 2.0).max(0.0), (-out).max(out - (d.half + w)).max(0.0));
        ax.hypot(ay) < TAG_REACH
    };
    let mut moves = Vec::new();
    for (i, d) in doors.iter().enumerate() {
        if d.normal == [0.0, 0.0] {
            continue;
        }
        // The default: the side opposite the leaf for a door, 400 mm past the face.
        let first = if d.flip_facing { 1.0 } else { -1.0 };
        let u = [d.normal[1], -d.normal[0]];
        let at = |sign: f64, far: f64, shift: f64| {
            let k = sign * (d.half + far);
            [d.at[0] as f64 + d.normal[0] * k + u[0] * shift, d.at[1] as f64 + d.normal[1] * k + u[1] * shift]
        };
        let clear = |sign: f64, far: f64, shift: f64| !(0..doors.len()).any(|j| leaf_hit(j, at(sign, far, shift)));
        if clear(first, 400.0, 0.0) {
            continue;
        }
        // Nearest first: the same side, then the other, farther out and farther along the wall.
        let mut found = None;
        'search: for far in [400.0, 700.0, 1000.0] {
            for shift in [0.0, 600.0, -600.0, 900.0, -900.0] {
                for sign in [first, -first] {
                    if clear(sign, far, shift) {
                        found = Some((sign, far, shift));
                        break 'search;
                    }
                }
            }
        }
        if let Some((sign, far, shift)) = found {
            let k = sign * (d.half + far);
            moves.push((i, [(d.normal[0] * k + u[0] * shift).round(), (d.normal[1] * k + u[1] * shift).round()]));
        }
    }
    moves
}
