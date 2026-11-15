//! Geometry for a studio-office graph.
//!
//! ```text
//!   +------+---------+----------------+      the core band, north of the studio:
//!   | wc wc| reception|  meeting | store |       the WCs over a passage on the west, the reception, and the corridor
//!   |  wc  |          +----------------+       strip under the meeting rooms and stores on the east
//!   +-passage-+       |  corridor      |
//!   +------------------------------------+
//!   |                studio              |   the studio, the south band; the passage is open to it
//!   +------------------------------------+
//! ```
//!
//! The shell is where the core lies against the studio: the full length of it (`bar`), flush with one end (`flush_west`,
//! `flush_east`) or inset at both (`inset`). The seed turns and mirrors the whole plan last. All in whole millimetres on wall
//! centre lines.

use std::collections::{BTreeMap, BTreeSet};

use super::super::Rng;
use super::super::house::place::{DoorAt, Rect, WallAt, counter_clockwise, point, shared, trace};
use super::{Kind, Means, StudioGraph};

pub const SHELLS: [&str; 4] = ["bar", "flush_west", "flush_east", "inset"];

const SALT: u64 = 0x5700_0010;
const PASSAGE_DEPTH: i64 = 1500;
const WC_WIDTH: i64 = 1800;
/// What a door needs on each side of its leaf in the wall it is in.
const DOOR_CLEAR: i64 = 400;
const WINDOW_WIDTH: i64 = 1800;
/// A window has a slot of this much wall, mm, with the window in its middle.
const WINDOW_SLOT: i64 = 3000;
const MOST_WINDOWS: i64 = 10;
/// The studio's least depth, mm.
const LEAST_DEPTH: i64 = 3600;

#[derive(Clone, Debug)]
pub struct WindowAt {
    pub at: [i64; 2],
    pub width: i64,
    pub ty: String,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub shell: &'static str,
    /// A room's rectangle, by its index in the graph.
    pub rects: Vec<Rect>,
    /// The reception's way in first, the studio's way out last.
    pub doors: Vec<DoorAt>,
    pub windows: Vec<WindowAt>,
    /// Room separation lines, for the open links.
    pub separators: Vec<Vec<[i64; 2]>>,
    pub walls: Vec<WallAt>,
    /// The outline of the plan, counter-clockwise.
    pub boundary: Vec<[i64; 2]>,
    pub width: i64,
    pub depth: i64,
    pub gross_mm2: i64,
}

fn round100(x: f64) -> i64 {
    ((x / 100.0).round() as i64) * 100
}

/// The tries for a studio office, in the fixed order they are made: the seed's shell first, then the next, and for each shell
/// the seed's proportion first.
pub fn attempts<'a>(g: &'a StudioGraph, seed: u64, area_m2: f64) -> impl Iterator<Item = Result<Layout, String>> + 'a {
    let mut rng = Rng::new(seed, SALT + 7);
    let (shell0, aspect0) = (rng.below(SHELLS.len() as u64) as usize, rng.below(3) as usize);
    (0..SHELLS.len())
        .flat_map(move |s| (0..3).map(move |a| (s, a)))
        .map(move |(s, a)| build_once(g, seed, area_m2, (shell0 + s) % SHELLS.len(), (aspect0 + a) % 3))
}

fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Rect {
    Rect { x0, y0, x1, y1 }
}

fn build_once(g: &StudioGraph, seed: u64, area_m2: f64, shell: usize, aspect: usize) -> Result<Layout, String> {
    let mut rng = Rng::new(seed, SALT);
    let a = area_m2;
    let grow = ((a - 150.0) / 390.0).clamp(0.0, 1.0);
    let wcs = g.wcs.len() as i64;
    let cd = g.corridor_width;
    let wd = [2600, 3000, 3400][rng.below(3) as usize];
    let dc = (PASSAGE_DEPTH + wd).max(cd + 2200);
    let wz = wcs * WC_WIDTH;
    let wr = round100(4400.0 + 400.0 * rng.below(4) as f64 + 1200.0 * grow);
    let widths: Vec<i64> = g
        .upper
        .iter()
        .map(|&i| {
            if g.rooms[i].kind == Kind::Meeting {
                round100((4500.0 + 3000.0 * grow) * (90 + 5 * rng.below(5)) as f64 / 100.0)
            } else {
                2400 + 600 * rng.below(3) as i64
            }
        })
        .collect();
    let wc: i64 = widths.iter().sum();
    let wcore = wz + wr + wc;
    let studio_mm2 = a * 1.0e6 - (wcore * dc) as f64;
    if studio_mm2 < 40.0e6 {
        return Err(format!(
            "{a:.1} m2 leaves the studio only {:.1} m2 beside a core of {:.1} m2",
            studio_mm2 / 1.0e6,
            (wcore * dc) as f64 / 1.0e6
        ));
    }
    let asp = [1.3, 1.8, 2.4][aspect];
    let ideal = (studio_mm2 * asp).sqrt();
    let (len, x0) = match SHELLS[shell] {
        "bar" => (wcore, 0),
        "flush_west" => (round100(ideal).max(wcore + 3000), 0),
        "flush_east" => {
            let l = round100(ideal).max(wcore + 3000);
            (l, l - wcore)
        }
        _ => {
            let e = round100((ideal - wcore as f64) / 2.0).max(1500);
            (wcore + 2 * e, e)
        }
    };
    let ds = round100(studio_mm2 / len as f64);
    if ds < LEAST_DEPTH {
        return Err(format!("the studio would be only {ds} mm deep in the {} shell", SHELLS[shell]));
    }
    if ds > 3 * len {
        return Err(format!("the studio would be {ds} mm deep against {len} mm long in the {} shell", SHELLS[shell]));
    }
    let mut rects = vec![rect(0, 0, 0, 0); g.rooms.len()];
    rects[g.studio] = rect(0, 0, len, ds);
    rects[g.passage] = rect(x0, ds, x0 + wz, ds + PASSAGE_DEPTH);
    for (k, &w) in g.wcs.iter().enumerate() {
        let k = k as i64;
        rects[w] = rect(x0 + k * WC_WIDTH, ds + PASSAGE_DEPTH, x0 + (k + 1) * WC_WIDTH, ds + dc);
    }
    let xr = x0 + wz;
    rects[g.reception] = rect(xr, ds, xr + wr, ds + dc);
    let xc = xr + wr;
    rects[g.corridor] = rect(xc, ds, x0 + wcore, ds + cd);
    let mut x = xc;
    for (&i, &w) in g.upper.iter().zip(&widths) {
        rects[i] = rect(x, ds + cd, x + w, ds + dc);
        x += w;
    }
    assemble(g, seed, SHELLS[shell], rects)
}

type Piece = (bool, i64, i64, i64);

fn assemble(g: &StudioGraph, seed: u64, shell: &'static str, rects: Vec<Rect>) -> Result<Layout, String> {
    let gross: i64 = rects.iter().map(Rect::area).sum();
    let open: BTreeSet<(usize, usize)> = g.links.iter().filter(|l| l.means == Means::Open).map(|l| (l.a.min(l.b), l.a.max(l.b))).collect();

    // Every edge of every room, cut where another room's corner lies on it: one piece for each stretch with a wall to it.
    type Edges = Vec<(i64, i64, usize)>;
    let mut lines: BTreeMap<(bool, i64), Edges> = BTreeMap::new();
    for (i, r) in rects.iter().enumerate() {
        for (vertical, line, lo, hi) in
            [(true, r.x0, r.y0, r.y1), (true, r.x1, r.y0, r.y1), (false, r.y0, r.x0, r.x1), (false, r.y1, r.x0, r.x1)]
        {
            lines.entry((vertical, line)).or_default().push((lo, hi, i));
        }
    }
    let mut exterior: Vec<(Piece, usize)> = Vec::new();
    let mut interior: Vec<Piece> = Vec::new();
    let mut opened: Vec<Piece> = Vec::new();
    for ((vertical, line), edges) in &lines {
        let cuts: BTreeSet<i64> = edges.iter().flat_map(|(lo, hi, _)| [*lo, *hi]).collect();
        let cuts: Vec<i64> = cuts.into_iter().collect();
        for pair in cuts.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            let rooms: Vec<usize> = edges.iter().filter(|(lo, hi, _)| *lo <= p && q <= *hi).map(|e| e.2).collect();
            match rooms[..] {
                [] => {}
                [r] => exterior.push(((*vertical, *line, p, q), r)),
                [x, y] if open.contains(&(x.min(y), x.max(y))) => opened.push((*vertical, *line, p, q)),
                [_, _] => interior.push((*vertical, *line, p, q)),
                _ => return Err("three rooms share a wall".into()),
            }
        }
    }
    let merge = |mut pieces: Vec<Piece>| -> Vec<Piece> {
        pieces.sort();
        let mut out: Vec<Piece> = Vec::new();
        for p in pieces {
            match out.last_mut() {
                Some(last) if last.0 == p.0 && last.1 == p.1 && last.3 == p.2 => last.3 = p.3,
                _ => out.push(p),
            }
        }
        out
    };
    let corner = |s: &Piece| (point(s.0, s.1, s.2), point(s.0, s.1, s.3));
    let ext_pieces: Vec<Piece> = exterior.iter().map(|(p, _)| *p).collect();
    let boundary = trace(&ext_pieces)?;

    // Doors: the link's, at the middle of the run the two rooms share; the reception's way in, in its north wall, and the
    // studio's way out, in the middle of its south wall.
    let mut doors = Vec::new();
    let mut separators: Vec<Vec<[i64; 2]>> = Vec::new();
    let door =
        |link: Option<usize>, rooms: (usize, Option<usize>), at: [i64; 2], family: &'static str, width: i64, op: &'static str| DoorAt {
            link,
            rooms,
            at,
            family,
            ty: format!("{width} x 2100"),
            width,
            op,
            flip_facing: false,
            flip_hand: false,
            opens_into: None,
            hinge: None,
            normal: [0.0, 0.0],
            half: 0.0,
        };
    let rc = &rects[g.reception];
    if rc.x1 - rc.x0 < 2000 + 2 * DOOR_CLEAR {
        return Err("the reception is too narrow for the way in".into());
    }
    doors.push(door(None, (g.reception, None), [(rc.x0 + rc.x1) / 2, rc.y1], "Double Door", 2000, "double-swing"));
    for (k, l) in g.links.iter().enumerate() {
        let Some((vertical, line, lo, hi)) = shared(&rects[l.a], &rects[l.b]) else {
            return Err(format!("`{}` and `{}` have no wall in common for their link", g.rooms[l.a].id, g.rooms[l.b].id));
        };
        if l.means == Means::Open {
            continue;
        }
        let pick = Rng::new(seed, SALT + 100 + k as u64).below(2);
        let run = hi - lo;
        let (family, width, op) = match l.means {
            Means::Swing => {
                let order: [i64; 2] = if pick == 0 { [900, 800] } else { [800, 900] };
                let Some(w) = order.into_iter().find(|w| run >= w + DOOR_CLEAR) else {
                    return Err(format!(
                        "the wall of {run} mm between `{}` and `{}` is too short for its door",
                        g.rooms[l.a].id, g.rooms[l.b].id
                    ));
                };
                ("Single Door", w, "swing")
            }
            _ => {
                let order: [i64; 2] = if pick == 0 { [1600, 1800] } else { [1800, 1600] };
                let Some(w) = order.into_iter().find(|w| run >= w + DOOR_CLEAR) else {
                    return Err(format!(
                        "the wall of {run} mm between `{}` and `{}` is too short for its door",
                        g.rooms[l.a].id, g.rooms[l.b].id
                    ));
                };
                ("Double Door", w, "double-swing")
            }
        };
        doors.push(door(Some(k), (l.a, Some(l.b)), point(vertical, line, (lo + hi) / 2), family, width, op));
    }
    if g.studio_door {
        let s = &rects[g.studio];
        doors.push(door(None, (g.studio, None), [(s.x0 + s.x1) / 2, s.y0], "Double Door", 1800, "double-swing"));
    }

    // Windows in the exterior walls of the studio, the reception and the meeting rooms, a slot to each, clear of the doors.
    let mut windows = Vec::new();
    for ((vertical, line, p, q), room) in &exterior {
        if !matches!(g.rooms[*room].kind, Kind::Office | Kind::Reception | Kind::Meeting) {
            continue;
        }
        let slots = ((q - p - 1200) / WINDOW_SLOT).clamp(0, MOST_WINDOWS);
        for s in 0..slots {
            let along = p + 600 + s * WINDOW_SLOT + WINDOW_SLOT / 2;
            let at = point(*vertical, *line, along);
            let clear =
                doors.iter().all(|d| (d.at[0] - at[0]).abs().max((d.at[1] - at[1]).abs()) >= (d.width + WINDOW_WIDTH) / 2 + DOOR_CLEAR);
            if clear {
                windows.push(WindowAt { at, width: WINDOW_WIDTH, ty: format!("{WINDOW_WIDTH} x 1200") });
            }
        }
    }
    if windows.is_empty() {
        return Err("no wall is long enough for a window".into());
    }
    for piece in merge(opened) {
        let (a, b) = corner(&piece);
        separators.push(vec![a, b]);
    }
    let walls_out: Vec<WallAt> = merge(ext_pieces)
        .iter()
        .map(|s| (true, s))
        .chain(merge(interior).iter().map(|s| (false, s)))
        .map(|(exterior, s)| {
            let (a, b) = corner(s);
            WallAt { exterior, a, b }
        })
        .collect();

    // Turned and mirrored by the seed, then moved so that the plan's south-west corner is the origin.
    let mut turn = Rng::new(seed, SALT + 2);
    let (quarters, mirror) = (turn.below(4), turn.below(2) == 1);
    let tf = |p: [i64; 2]| -> [i64; 2] {
        let [x, y] = if mirror { [-p[0], p[1]] } else { p };
        match quarters {
            0 => [x, y],
            1 => [-y, x],
            2 => [-x, -y],
            _ => [y, -x],
        }
    };
    let mut every: Vec<[i64; 2]> = Vec::new();
    for r in &rects {
        every.extend([[r.x0, r.y0], [r.x1, r.y1]].map(tf));
    }
    let (min_x, min_y) = (every.iter().map(|p| p[0]).min().unwrap_or(0), every.iter().map(|p| p[1]).min().unwrap_or(0));
    let (max_x, max_y) = (every.iter().map(|p| p[0]).max().unwrap_or(0), every.iter().map(|p| p[1]).max().unwrap_or(0));
    let mv = |p: [i64; 2]| -> [i64; 2] {
        let q = tf(p);
        [q[0] - min_x, q[1] - min_y]
    };
    let ordered = |a: [i64; 2], b: [i64; 2]| if a <= b { (a, b) } else { (b, a) };
    let rects: Vec<Rect> = rects
        .iter()
        .map(|r| {
            let (a, b) = (mv([r.x0, r.y0]), mv([r.x1, r.y1]));
            Rect { x0: a[0].min(b[0]), y0: a[1].min(b[1]), x1: a[0].max(b[0]), y1: a[1].max(b[1]) }
        })
        .collect();
    for d in &mut doors {
        d.at = mv(d.at);
    }
    for w in &mut windows {
        w.at = mv(w.at);
    }
    let separators = separators.into_iter().map(|run| run.into_iter().map(mv).collect()).collect();
    let walls = walls_out
        .into_iter()
        .map(|w| {
            let (a, b) = ordered(mv(w.a), mv(w.b));
            WallAt { exterior: w.exterior, a, b }
        })
        .collect();
    let boundary: Vec<[i64; 2]> = boundary.into_iter().map(mv).collect();
    let boundary = counter_clockwise(boundary);
    Ok(Layout { shell, rects, doors, windows, separators, walls, boundary, width: max_x - min_x, depth: max_y - min_y, gross_mm2: gross })
}

fn holds(r: &Rect, p: [f64; 2]) -> bool {
    p[0] > r.x0 as f64 && p[0] < r.x1 as f64 && p[1] > r.y0 as f64 && p[1] < r.y1 as f64
}

/// How much a room's kind wants a door to open into it: a hall over the studio, and the rest over a hall.
fn pull(kind: Kind) -> u8 {
    match kind {
        Kind::Office => 0,
        Kind::Hall => 1,
        _ => 2,
    }
}

/// Sets each door's flips, and what the extractor will say of it. A door opens into the room that is not a hall, and into the
/// hall from the studio; an outside door opens out. Every door hangs from the start jamb. A door whose host wall is not found
/// exactly once is an error, so the try is skipped.
pub fn orient(g: &StudioGraph, l: &mut Layout) -> Result<(), String> {
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
        let [w] = hosts.as_slice() else {
            return Err(format!("door D{} has {} host walls under it, not one", i + 1, hosts.len()));
        };
        let (a, b) = ([w.a[0] as f64, w.a[1] as f64], [w.b[0] as f64, w.b[1] as f64]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let n = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
        let half = if w.exterior { 150.0 } else { 50.0 };
        l.doors[i].normal = n;
        l.doors[i].half = half;
        let k = half + 150.0;
        let room_at = |sign: f64| {
            let p = [at[0] + sign * n[0] * k, at[1] + sign * n[1] * k];
            l.rects.iter().position(|r| holds(r, p))
        };
        let (left, right) = (room_at(1.0), room_at(-1.0));
        let target: Option<Option<usize>> = match (left, right) {
            (Some(x), Some(y)) if x != y => Some(Some(if pull(g.rooms[x].kind) >= pull(g.rooms[y].kind) { x } else { y })),
            (Some(_), None) | (None, Some(_)) => Some(None),
            _ => None,
        };
        let face = target.is_some_and(|t| t != left);
        let side = if face { right } else { left };
        let d = &mut l.doors[i];
        d.flip_facing = face;
        d.flip_hand = false;
        d.opens_into = Some(side.map_or("exterior".to_string(), |r| g.rooms[r].id.clone()));
        d.hinge = Some("start");
    }
    Ok(())
}
