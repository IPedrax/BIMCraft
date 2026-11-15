//! Geometry for an office-pattern graph.
//!
//! ```text
//!   +--------+-------------------------+----------+     x runs along the corridor, y across it
//!   |        | north rooms (optional)  |          |
//!   | lobby  +-------------------------+   open   |     the lobby at x = 0, the full depth; the open office
//!   |        | corridor                |  office  |     at the far end, the full depth; the rooms in bands
//!   |        +-------------------------+          |     on one or both sides of the corridor
//!   |        | south rooms             |          |
//!   +--------+-------------------------+----------+
//! ```
//!
//! Sizes are drawn from the seed and scaled by the area; the length is solved from the area, so the gross area is what was asked
//! to within a rounding. A chamfer cuts the lobby's south-west corner and the open office's north-east corner at 45 degrees. The
//! seed turns and mirrors the plan last. All in whole millimetres on wall centre lines.

use std::collections::{BTreeMap, BTreeSet};

use super::super::Rng;
use super::super::house::place::{DoorAt, Rect, WallAt, counter_clockwise, point, shared};
use super::{Kind, Link, Means, PatternGraph, Room};

const SALT: u64 = 0x0FF1_CE00;
const TURN_SALT: u64 = SALT + 0x1000;
const PICK_SALT: u64 = SALT + 0x2000;
/// How many draws are tried for one brief, in order.
pub const TRIES: u64 = 12;
/// What a door needs on each side of its leaf in the wall it is in.
const DOOR_CLEAR: i64 = 400;
const WINDOW_WIDTH: i64 = 1800;
const MIN_ROOM: i64 = 3000;
const MAX_ROOM: i64 = 5200;
/// A window sits at least this far from the ends of its wall.
pub const WINDOW_EDGE: i64 = 300;

/// The dimensions of one plan, local: the lengths the rooms of each band add up to are `run`.
#[derive(Clone, Debug)]
pub struct Case {
    pub south_d: i64,
    pub north_d: i64,
    pub corr: i64,
    pub lobby_w: i64,
    pub open_w: i64,
    pub run: i64,
    pub south: Vec<(Kind, i64)>,
    pub north: Vec<(Kind, i64)>,
    pub c_sw: i64,
    pub c_ne: i64,
}

impl Case {
    pub fn depth(&self) -> i64 {
        self.south_d + self.corr + self.north_d
    }
}

#[derive(Clone, Debug)]
pub struct WindowAt {
    pub at: [i64; 2],
    pub width: i64,
    pub ty: String,
}

#[derive(Clone, Debug)]
pub struct Layout {
    /// A room's rectangle, by its index in the graph.
    pub rects: Vec<Rect>,
    /// The lobby's way in first.
    pub doors: Vec<DoorAt>,
    pub windows: Vec<WindowAt>,
    pub walls: Vec<WallAt>,
    /// The outline of the plan, counter-clockwise.
    pub boundary: Vec<[i64; 2]>,
    pub width: i64,
    pub depth: i64,
    pub gross_mm2: i64,
    pub double_loaded: bool,
    pub chamfers: usize,
    pub quarters: u64,
    pub mirror: bool,
}

fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Rect {
    Rect { x0, y0, x1, y1 }
}

fn round100(x: f64) -> i64 {
    ((x / 100.0).round() as i64) * 100
}

fn floor200(x: i64) -> i64 {
    x.div_euclid(200) * 200
}

/// Rooms of the kinds, side by side, whose widths add up to `width`: each between 3000 and 5200 mm.
fn band(rng: &mut Rng, width: i64, kinds: &[Kind], depth: i64, focus: bool) -> Vec<(Kind, i64)> {
    let mut rooms = Vec::new();
    let mut left = width;
    while left >= MIN_ROOM {
        // What cannot be split into two rooms of at least 3000 mm is one room, a little wider than most.
        let w = if left <= MAX_ROOM || left < 2 * MIN_ROOM {
            left
        } else {
            let most = MAX_ROOM.min(left - MIN_ROOM);
            MIN_ROOM + 200 * rng.below(((most - MIN_ROOM) / 200 + 1) as u64) as i64
        };
        let mut kind = kinds[rng.below(kinds.len() as u64) as usize];
        // A focus room is a small room: bigger ones are offices.
        if kind == Kind::FocusRoom && (!focus || w * depth > 25_000_000) {
            kind = Kind::PrivateOffice;
        }
        rooms.push((kind, w));
        left -= w;
    }
    rooms
}

fn count(south: &[(Kind, i64)], north: &[(Kind, i64)], kind: Kind) -> usize {
    south.iter().chain(north).filter(|r| r.0 == kind).count()
}

/// Makes the plan hold a restroom, a pantry, a conference room and a private office, by changing a room whose kind there is
/// more than one of (or a focus room).
fn ensure(south: &mut [(Kind, i64)], north: &mut [(Kind, i64)]) -> Result<(), String> {
    for (need, south_only) in [(Kind::Restroom, true), (Kind::Pantry, true), (Kind::Conference, false), (Kind::PrivateOffice, false)] {
        if count(south, north, need) > 0 {
            continue;
        }
        let spare = |kind: Kind, s: &[(Kind, i64)], n: &[(Kind, i64)]| kind == Kind::FocusRoom || count(s, n, kind) >= 2;
        let pick = south.iter().enumerate().rev().find(|(_, r)| spare(r.0, south, north)).map(|(i, _)| (false, i)).or_else(|| {
            if south_only { None } else { north.iter().enumerate().rev().find(|(_, r)| spare(r.0, south, north)).map(|(i, _)| (true, i)) }
        });
        let Some((in_north, i)) = pick else {
            return Err(format!("the bands leave no room to make a {}", need.name()));
        };
        if in_north { north[i].0 = need } else { south[i].0 = need }
    }
    Ok(())
}

/// One draw for a brief: the dimensions of a plan whose gross area is the one asked, or why this draw won't do.
pub fn draw(seed: u64, area_m2: f64, t: u64) -> Result<Case, String> {
    let mut rng = Rng::new(seed, SALT + t);
    let grow = ((area_m2 - 200.0) / 400.0).clamp(0.0, 1.0);
    let wide = (grow * 8.0) as u64;
    let double = t >= 6 || rng.below(100) >= 35;
    let south_d = if double { 3600 + 200 * rng.below(5) as i64 } else { 4600 + 200 * rng.below(6) as i64 };
    let north_d = if double { 3400 + 200 * rng.below(7) as i64 } else { 0 };
    let corr = 1800 + 200 * rng.below(5) as i64;
    let lobby_w = 4200 + 200 * rng.below(4 + wide) as i64;
    let open_w0 = 6400 + 400 * rng.below(3 + 2 * wide) as i64;
    let c_sw0 = if rng.below(100) < 45 { 1200 + 200 * rng.below(7) as i64 } else { 0 };
    let c_ne0 = if rng.below(100) < 40 { 1600 + 200 * rng.below(9) as i64 } else { 0 };
    let focus = rng.below(100) < 45;
    let depth = south_d + corr + north_d;
    let c_sw = floor200(c_sw0.min(lobby_w - 2500));
    let c_sw = if c_sw < 1200 { 0 } else { c_sw };
    let clamp_ne = |c: i64, open_w: i64| {
        let c = floor200(c.min(open_w.min(depth) - 3000));
        if c < 1600 { 0 } else { c }
    };
    let c_ne0 = clamp_ne(c_ne0, open_w0);
    let length = round100((area_m2 * 1.0e6 + ((c_sw * c_sw + c_ne0 * c_ne0) / 2) as f64) / depth as f64);
    if length * 10 < 16 * depth {
        return Err(format!("{length} mm long for {depth} mm deep is too squat for the pattern"));
    }
    if length * 10 > 70 * depth {
        return Err(format!("{length} mm long for {depth} mm deep is too long and narrow for the pattern"));
    }
    let (min_run, max_run) = if double { (2 * MIN_ROOM, 36_000) } else { (4 * MIN_ROOM, 36_000) };
    let mut open_w = open_w0;
    let mut run = length - lobby_w - open_w;
    if run < min_run {
        open_w = length - lobby_w - min_run;
        run = min_run;
        if open_w < 6400 {
            return Err(format!("{length} mm is too short for a lobby, {min_run} mm of rooms and an open office"));
        }
    }
    if run > max_run {
        open_w += run - max_run;
        run = max_run;
    }
    let c_ne = clamp_ne(c_ne0, open_w);
    let mut south =
        band(&mut rng, run, &[Kind::PrivateOffice, Kind::PrivateOffice, Kind::Conference, Kind::Restroom, Kind::Pantry], south_d, focus);
    let mut north = if north_d > 0 {
        band(&mut rng, run, &[Kind::PrivateOffice, Kind::PrivateOffice, Kind::Conference, Kind::FocusRoom], north_d, focus)
    } else {
        Vec::new()
    };
    ensure(&mut south, &mut north)?;
    Ok(Case { south_d, north_d, corr, lobby_w, open_w, run, south, north, c_sw, c_ne })
}

const LABELS: [(Kind, &str); 5] = [
    (Kind::PrivateOffice, "Private Office"),
    (Kind::Conference, "Conference"),
    (Kind::Restroom, "Restroom"),
    (Kind::Pantry, "Pantry"),
    (Kind::FocusRoom, "Focus Room"),
];

/// The graph of a case: the lobby, the corridor and the open office first, then the south rooms, then the north ones. Each kind
/// is numbered across both sides.
pub fn graph_of(c: &Case) -> PatternGraph {
    let mut rooms = vec![
        Room { id: "lobby".into(), name: "Lobby".into(), kind: Kind::Lobby },
        Room { id: "corridor".into(), name: "Corridor".into(), kind: Kind::Hall },
        Room { id: "open_office".into(), name: "Open Office".into(), kind: Kind::OpenOffice },
    ];
    let wide = c.corr >= 1600 + DOOR_CLEAR;
    let door = if wide { Means::DoubleSwing } else { Means::Swing };
    let mut links = vec![Link { a: 0, b: 1, means: door }, Link { a: 1, b: 2, means: door }];
    let mut counts: BTreeMap<Kind, usize> = BTreeMap::new();
    for &(kind, w) in c.south.iter().chain(&c.north) {
        let n = counts.entry(kind).or_default();
        *n += 1;
        let label = LABELS.iter().find(|(k, _)| *k == kind).map_or("Room", |(_, l)| l);
        rooms.push(Room { id: format!("{}_{n}", kind.name()), name: format!("{label} {n}"), kind });
        let means = if kind == Kind::Conference && w >= 4200 { Means::DoubleSwing } else { Means::Swing };
        links.push(Link { a: 1, b: rooms.len() - 1, means });
    }
    PatternGraph { rooms, links, entry: 0, lobby: 0, corridor: 1, open: 2 }
}

type Piece = (bool, i64, i64, i64);

pub fn assemble(g: &PatternGraph, c: &Case, seed: u64) -> Result<Layout, String> {
    let (depth, cy0) = (c.depth(), c.south_d);
    let cy1 = c.south_d + c.corr;
    let open_x = c.lobby_w + c.run;
    let len = open_x + c.open_w;
    let mut rects = vec![rect(0, 0, 0, 0); g.rooms.len()];
    rects[g.lobby] = rect(0, 0, c.lobby_w, depth);
    rects[g.corridor] = rect(c.lobby_w, cy0, open_x, cy1);
    rects[g.open] = rect(open_x, 0, len, depth);
    let mut x = c.lobby_w;
    for (k, &(_, w)) in c.south.iter().enumerate() {
        rects[3 + k] = rect(x, 0, x + w, cy0);
        x += w;
    }
    if x != open_x {
        return Err("the south rooms do not reach the open office".into());
    }
    x = c.lobby_w;
    for (k, &(_, w)) in c.north.iter().enumerate() {
        rects[3 + c.south.len() + k] = rect(x, cy1, x + w, depth);
        x += w;
    }
    if !c.north.is_empty() && x != open_x {
        return Err("the north rooms do not reach the open office".into());
    }
    let gross: i64 = rects.iter().map(Rect::area).sum::<i64>() - (c.c_sw * c.c_sw + c.c_ne * c.c_ne) / 2;

    // The shell: a rectangle with the chamfers cut, counter-clockwise.
    let mut shell: Vec<[i64; 2]> = vec![[c.c_sw, 0], [len, 0]];
    if c.c_ne > 0 {
        shell.extend([[len, depth - c.c_ne], [len - c.c_ne, depth]]);
    } else {
        shell.push([len, depth]);
    }
    shell.push([0, depth]);
    if c.c_sw > 0 {
        shell.push([0, c.c_sw]);
    }

    // Every edge of every room, cut where another room's corner lies on it: a wall for each stretch two rooms share.
    type Edges = Vec<(i64, i64, usize)>;
    let mut lines: BTreeMap<(bool, i64), Edges> = BTreeMap::new();
    for (i, r) in rects.iter().enumerate() {
        for (vertical, line, lo, hi) in
            [(true, r.x0, r.y0, r.y1), (true, r.x1, r.y0, r.y1), (false, r.y0, r.x0, r.x1), (false, r.y1, r.x0, r.x1)]
        {
            lines.entry((vertical, line)).or_default().push((lo, hi, i));
        }
    }
    let mut interior: Vec<Piece> = Vec::new();
    for ((vertical, line), edges) in &lines {
        let cuts: BTreeSet<i64> = edges.iter().flat_map(|(lo, hi, _)| [*lo, *hi]).collect();
        let cuts: Vec<i64> = cuts.into_iter().collect();
        for pair in cuts.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            let n = edges.iter().filter(|(lo, hi, _)| *lo <= p && q <= *hi).count();
            match n {
                0 | 1 => {}
                2 => interior.push((*vertical, *line, p, q)),
                _ => return Err("three rooms share a wall".into()),
            }
        }
    }
    interior.sort();
    let mut merged: Vec<Piece> = Vec::new();
    for p in interior {
        match merged.last_mut() {
            Some(last) if last.0 == p.0 && last.1 == p.1 && last.3 == p.2 => last.3 = p.3,
            _ => merged.push(p),
        }
    }

    // Doors: the lobby's way in, in the middle of the end wall, clear of the chamfer; then each link's, at the middle of the run
    // its two rooms share.
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
    let way_in = ((c.c_sw + 1600).max(depth * 62 / 100) / 100) * 100;
    if way_in + 900 + DOOR_CLEAR > depth {
        return Err("the lobby's end wall is too short for its way in".into());
    }
    let mut doors = vec![door(None, (g.lobby, None), [0, way_in], "Double Door", 1800, "double-swing")];
    for (k, l) in g.links.iter().enumerate() {
        let Some((vertical, line, lo, hi)) = shared(&rects[l.a], &rects[l.b]) else {
            return Err(format!("`{}` and `{}` have no wall in common for their link", g.rooms[l.a].id, g.rooms[l.b].id));
        };
        let pick = Rng::new(seed, PICK_SALT + k as u64).below(2);
        let run = hi - lo;
        let (family, order, op): (&'static str, [i64; 2], &'static str) = match l.means {
            Means::Swing => ("Single Door", if pick == 0 { [900, 800] } else { [800, 900] }, "swing"),
            Means::DoubleSwing => ("Double Door", if pick == 0 { [1600, 1800] } else { [1800, 1600] }, "double-swing"),
        };
        let Some(w) = order.into_iter().find(|w| run >= w + DOOR_CLEAR) else {
            return Err(format!("the wall of {run} mm between `{}` and `{}` is too short for its door", g.rooms[l.a].id, g.rooms[l.b].id));
        };
        doors.push(door(Some(k), (l.a, Some(l.b)), point(vertical, line, (lo + hi) / 2), family, w, op));
    }

    // Windows, centred in the south and north rooms at least 3200 mm wide (not the restrooms), and on the open office's end wall.
    let mut windows: Vec<WindowAt> = Vec::new();
    let window = |at: [i64; 2]| WindowAt { at, width: WINDOW_WIDTH, ty: format!("{WINDOW_WIDTH} x 1200") };
    for (i, room) in g.rooms.iter().enumerate().skip(3) {
        let r = &rects[i];
        if room.kind == Kind::Restroom || r.x1 - r.x0 < 3200 {
            continue;
        }
        let along = (r.x0 + r.x1) / 2;
        windows.push(window(if r.y0 == 0 { [along, 0] } else { [along, depth] }));
    }
    let n = (c.open_w / 4000).clamp(2, 4);
    for i in 1..n {
        let yy = (depth * i / n / 100) * 100;
        if c.c_ne > 0 && yy > depth - c.c_ne - 1200 {
            continue;
        }
        windows.push(window([len, yy]));
    }
    if windows.is_empty() {
        return Err("no wall is long enough for a window".into());
    }

    // Walls outside: one for each side of the shell.
    let exterior: Vec<WallAt> = (0..shell.len()).map(|i| WallAt { exterior: true, a: shell[i], b: shell[(i + 1) % shell.len()] }).collect();

    // Turned and mirrored by the seed, then moved so that the plan's south-west corner is the origin.
    let mut turn = Rng::new(seed, TURN_SALT);
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
    let corner = |s: &Piece| (point(s.0, s.1, s.2), point(s.0, s.1, s.3));
    let walls: Vec<WallAt> = exterior
        .iter()
        .map(|w| (w.exterior, w.a, w.b))
        .chain(merged.iter().map(|s| {
            let (a, b) = corner(s);
            (false, a, b)
        }))
        .map(|(exterior, a, b)| {
            let (a, b) = ordered(mv(a), mv(b));
            WallAt { exterior, a, b }
        })
        .collect();
    let boundary = counter_clockwise(shell.into_iter().map(mv).collect());
    let layout = Layout {
        rects,
        doors,
        windows,
        walls,
        boundary,
        width: max_x - min_x,
        depth: max_y - min_y,
        gross_mm2: gross,
        double_loaded: !c.north.is_empty(),
        chamfers: usize::from(c.c_sw > 0) + usize::from(c.c_ne > 0),
        quarters,
        mirror,
    };
    for (i, w) in layout.windows.iter().enumerate() {
        let on: Vec<_> = layout
            .walls
            .iter()
            .filter(|wall| wall.exterior && along(wall, w.at).is_some_and(|(s, len)| s >= WINDOW_EDGE && s <= len - WINDOW_EDGE))
            .collect();
        if on.len() != 1 {
            return Err(format!("window W{} is on {} exterior walls, not one", i + 1, on.len()));
        }
    }
    Ok(layout)
}

/// Where `at` lies along `w`: the distance from its start and its length, when it lies on it.
pub fn along(w: &WallAt, at: [i64; 2]) -> Option<(i64, i64)> {
    let (a, b) = ([w.a[0] as f64, w.a[1] as f64], [w.b[0] as f64, w.b[1] as f64]);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if len < 1.0 {
        return None;
    }
    let p = [at[0] as f64, at[1] as f64];
    let s = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len;
    let d = ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / len;
    (d <= 1.0 && s >= 0.0 && s <= len).then_some((s.round() as i64, len.round() as i64))
}

fn holds(r: &Rect, p: [f64; 2]) -> bool {
    p[0] > r.x0 as f64 && p[0] < r.x1 as f64 && p[1] > r.y0 as f64 && p[1] < r.y1 as f64
}

/// How much a room's kind wants a door to open into it: the open office over the corridor, and the rest over it.
fn pull(kind: Kind) -> u8 {
    match kind {
        Kind::OpenOffice => 0,
        Kind::Hall => 1,
        _ => 2,
    }
}

/// Sets each door's flips, and what the extractor will say of it. A door opens into the room that is not the corridor, and into
/// the corridor from the open office; the way in opens out. Every door hangs from the start jamb (`flip_hand` is never set). A
/// door whose host wall is not found exactly once is an error, so the try is skipped.
pub fn orient(g: &PatternGraph, l: &mut Layout) -> Result<(), String> {
    for i in 0..l.doors.len() {
        let at = [l.doors[i].at[0] as f64, l.doors[i].at[1] as f64];
        let hosts: Vec<_> = l.walls.iter().filter(|w| along(w, l.doors[i].at).is_some()).collect();
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
