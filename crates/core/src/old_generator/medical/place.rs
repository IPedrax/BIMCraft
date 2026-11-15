//! Geometry for a medical day department.
//!
//! ```text
//!   +------+------------------------------------------------------+-----+   x runs along the plan, y across it
//!   |      | staff corridor                                       |     |
//!   |      +----------+---------+----------+---------+------------+     |
//!   | start|  offices |  cross  |  offices |  cross  |  offices   | end |   the office band, on some plans
//!   |      +----------+ corridor+----------+ corridor+------------+ cor-|
//!   |      |  block   |   X     |  block   |   X     |  block     | ri- |   the core: a cross corridor, then a block
//!   |      +----------+---------+----------+---------+------------+ dor |
//!   |      | patient corridor                                     |     |
//!   |      +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+----+     |
//!   |      | window band: cubicles, a washroom, stations          |     |
//!   +------+------------------------------------------------------+-----+
//! ```
//!
//! Every room is an axis-aligned rectangle and the rectangles tile the plan, in whole millimetres on wall centre lines. Where two
//! rooms share an edge there is a door at the middle of the run (a link by a door), a room separation line over the run (an open
//! link) or a wall. The length is solved from the area. The seed turns and mirrors the plan last, never shears it.

use std::collections::{BTreeMap, BTreeSet};

use super::super::Rng;
use super::super::house::place::{DoorAt, Rect, WallAt, counter_clockwise, point, shared};
use super::{Case, Flags, Kind, Link, Means, Room};

const SALT: u64 = 0x0ED1_DA00;
const TURN_SALT: u64 = SALT + 0x1000;
/// How many draws are tried for one brief, in order.
pub const TRIES: u64 = 24;
/// What a door needs on each side of its leaf in the wall it is in.
const DOOR_CLEAR: i64 = 400;
const SINGLE: i64 = 915;
const SINGLE_WIDE: i64 = 1067;
const PAIR: i64 = 1525;

#[derive(Clone, Debug)]
pub struct Layout {
    /// A room's rectangle, by its index in the case, after the plan is turned.
    pub rects: Vec<Rect>,
    pub doors: Vec<DoorAt>,
    /// Room separation lines, one for each open link.
    pub separators: Vec<[[i64; 2]; 2]>,
    pub walls: Vec<WallAt>,
    pub boundary: Vec<[i64; 2]>,
    pub width: i64,
    pub depth: i64,
    pub gross_mm2: i64,
    pub quarters: u64,
    pub mirror: bool,
}

fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Rect {
    Rect { x0, y0, x1, y1 }
}

fn round100(x: f64) -> i64 {
    ((x / 100.0).round() as i64) * 100
}

fn pick(rng: &mut Rng, lo: i64, hi: i64, step: i64) -> i64 {
    lo + step * rng.below(((hi - lo) / step + 1) as u64) as i64
}

/// The rooms and links of a plan as they are made.
struct Builder {
    rooms: Vec<Room>,
    links: Vec<Link>,
    rooms_made: usize,
    corridors_made: usize,
}

impl Builder {
    fn room(&mut self, kind: Kind, r: Rect) -> usize {
        self.rooms_made += 1;
        let n = self.rooms_made;
        self.rooms.push(Room { id: format!("1-09{n:02}"), name: format!("{} {n}", kind.label()), kind, rect: r });
        self.rooms.len() - 1
    }

    fn corridor(&mut self, name: &str, r: Rect) -> usize {
        self.corridors_made += 1;
        let n = self.corridors_made;
        self.rooms.push(Room { id: format!("C1-09{n:02}"), name: name.to_string(), kind: Kind::Corridor, rect: r });
        self.rooms.len() - 1
    }

    fn link(&mut self, a: usize, b: usize, means: Means) {
        self.links.push(Link { a, b, means });
    }
}

/// What a block of the core is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Block {
    Treatment { utility_w: i64, treatment_rooms: i64, staff_pair: bool },
    Isolation { anteroom_w: i64 },
    Service,
}

impl Block {
    fn letter(self) -> char {
        match self {
            Block::Treatment { .. } => 'T',
            Block::Isolation { .. } => 'I',
            Block::Service => 'S',
        }
    }

    fn least_width(self) -> i64 {
        match self {
            Block::Treatment { utility_w, treatment_rooms, .. } => utility_w + treatment_rooms * 3400 + 2400,
            Block::Isolation { anteroom_w } => anteroom_w + 3800,
            Block::Service => 6000,
        }
    }
}

/// One draw for a brief: a plan whose gross area is the one asked, or why this draw won't do.
pub fn draw(seed: u64, area_m2: f64, t: u64) -> Result<Case, String> {
    let mut rng = Rng::new(seed, SALT + t);
    let grow = ((area_m2 - 400.0) / 800.0).clamp(0.0, 1.0);
    let wd = pick(&mut rng, 3200, 3800, 100);
    let pc = pick(&mut rng, 2500, 3000, 100);
    let core = pick(&mut rng, 9000 + ((1500.0 * grow) as i64 / 500) * 500, 12000, 500);
    let office_band = rng.below(100) < 50;
    let od = if office_band { pick(&mut rng, 3200, 4400, 200) } else { 0 };
    let sc = pick(&mut rng, 1300, 2600, 100);
    let (y2, y3) = (wd + pc, wd + pc + core);
    let y4 = y3 + od;
    let depth = y4 + sc;
    let length = round100(area_m2 * 1.0e6 / depth as f64);
    if length * 10 < 12 * depth {
        return Err(format!("{length} mm long for {depth} mm deep is too squat for the pattern"));
    }
    if length * 10 > 34 * depth {
        return Err(format!("{length} mm long for {depth} mm deep is too long and narrow for the pattern"));
    }

    // The start: nothing, a head row (a waiting room the full depth) or a second end corridor.
    let start = rng.below(10);
    let (head_row, second_end) = (start >= 7, (4..7).contains(&start));
    let start_w = if head_row {
        pick(&mut rng, 3000, 4000, 100)
    } else if second_end {
        pick(&mut rng, 1900, 2400, 100)
    } else {
        0
    };
    let end_w = pick(&mut rng, 1900, 2400, 100);
    let (xs, xe) = (start_w, length - end_w);
    let core_len = xe - xs;

    // The blocks: a treatment block and an isolation block, a service block last on some seeds, and more of the first two.
    let service = t < 12 && rng.below(100) < 45;
    let mut blocks = vec![
        Block::Treatment {
            utility_w: pick(&mut rng, 2400, 3000, 100),
            treatment_rooms: 1 + rng.below(2) as i64,
            staff_pair: rng.below(100) < 40,
        },
        Block::Isolation { anteroom_w: pick(&mut rng, 2400, 2900, 100) },
    ];
    let wanted = if t < 6 { 2 + rng.below(3) as usize } else { 2 };
    while blocks.len() + usize::from(service) < wanted {
        blocks.push(if rng.below(2) == 0 {
            Block::Treatment {
                utility_w: pick(&mut rng, 2400, 3000, 100),
                treatment_rooms: 1 + rng.below(2) as i64,
                staff_pair: rng.below(100) < 40,
            }
        } else {
            Block::Isolation { anteroom_w: pick(&mut rng, 2400, 2900, 100) }
        });
    }
    rng.shuffle(&mut blocks);
    if service {
        blocks.push(Block::Service);
    }
    let mut cross: Vec<i64> = blocks.iter().map(|_| pick(&mut rng, 1900, 3700, 100)).collect();
    let least: i64 = blocks.iter().map(|b| b.least_width()).sum();
    if core_len - least - cross.iter().sum::<i64>() < 0 {
        cross = cross.iter().map(|_| 1900).collect();
    }
    let slack = core_len - least - cross.iter().sum::<i64>();
    if slack < 0 {
        return Err(format!("{core_len} mm of core is too short for {} blocks and their cross corridors", blocks.len()));
    }
    let weights: Vec<u64> = blocks.iter().map(|_| 1 + rng.below(3)).collect();
    let total: u64 = weights.iter().sum();
    let mut widths: Vec<i64> = Vec::new();
    let mut given = 0;
    for (i, b) in blocks.iter().enumerate() {
        let extra = if i + 1 == blocks.len() { slack - given } else { (slack as u64 * weights[i] / total) as i64 / 100 * 100 };
        given += extra;
        widths.push(b.least_width() + extra);
    }

    // The window band.
    let window = window_band(&mut rng, xe - xs)?;

    let mut b = Builder { rooms: Vec::new(), links: Vec::new(), rooms_made: 0, corridors_made: 0 };
    let p = b.corridor("Patient Corridor", rect(xs, wd, xe, y2));
    let s = b.corridor("Staff Corridor", rect(xs, y4, xe, depth));
    let e = b.corridor("End Corridor", rect(xe, 0, length, depth));
    b.link(e, s, Means::Open);
    b.link(p, e, Means::Pair);
    if second_end {
        let e2 = b.corridor("Second End Corridor", rect(0, 0, start_w, depth));
        b.link(e2, s, Means::Open);
        b.link(p, e2, Means::Pair);
    }
    if head_row {
        let w = b.room(Kind::Waiting, rect(0, 0, start_w, depth));
        b.link(w, p, Means::Open);
    }

    let mut x = xs;
    for (kind, w) in window {
        let r = rect(x, 0, x + w, wd);
        let i = b.room(kind, r);
        match kind {
            Kind::TreatmentBay => b.link(i, p, Means::Open),
            Kind::Washroom => b.link(i, p, Means::Single(SINGLE)),
            _ => b.link(i, p, Means::Single(SINGLE_WIDE)),
        }
        x += w;
    }

    let mut x = xs;
    for (i, block) in blocks.iter().enumerate() {
        let xc = b.corridor("Cross Corridor", rect(x, y2, x + cross[i], y4));
        b.link(p, xc, Means::Open);
        b.link(xc, s, Means::Single(SINGLE_WIDE));
        let (bx0, bx1) = (x + cross[i], x + cross[i] + widths[i]);
        let at = Frame { x0: bx0, x1: bx1, y2, y3, cross: xc, patient: p, end: e };
        match *block {
            Block::Treatment { utility_w, treatment_rooms, staff_pair } => {
                treatment_block(&mut b, &mut rng, &at, utility_w, treatment_rooms, staff_pair)
            }
            Block::Isolation { anteroom_w } => isolation_block(&mut b, &mut rng, &at, anteroom_w),
            Block::Service => service_block(&mut b, &mut rng, &at),
        }
        if office_band {
            office_row(&mut b, &mut rng, bx0, bx1, y3, y4, s);
        }
        x = bx1;
    }
    if x != xe {
        return Err(format!("the blocks end at {x} mm, not at the end corridor, {xe} mm"));
    }
    let flags = Flags { service, office_band, blocks: blocks.iter().map(|b| b.letter()).collect(), head_row, second_end };
    Ok(Case { rooms: b.rooms, links: b.links, width: length, depth, flags })
}

/// The window band's rooms along `len`: cubicles, washrooms and stations (bays), and sometimes a shell space at one end.
fn window_band(rng: &mut Rng, len: i64) -> Result<Vec<(Kind, i64)>, String> {
    let washrooms = 1 + rng.below(2) as i64;
    let shell = rng.below(100) < 35;
    let (mut cubicles, mut stations) = (3 + rng.below(4) as i64, 2 + rng.below(3) as i64);
    let estimate = |c: i64, s: i64| c * 3700 + s * 4800 + washrooms * 2100 + if shell { 3300 } else { 0 };
    while estimate(cubicles, stations) * 10 > len * 11 && cubicles + stations > 5 {
        if cubicles > 3 && (stations <= 2 || rng.below(2) == 0) {
            cubicles -= 1;
        } else if stations > 2 {
            stations -= 1;
        } else {
            break;
        }
    }
    while estimate(cubicles, stations) * 10 < len * 9 && cubicles + stations < 16 {
        if rng.below(2) == 0 {
            cubicles += 1;
        } else {
            stations += 1;
        }
    }
    let mut items: Vec<(Kind, i64)> = Vec::new();
    items.extend((0..cubicles).map(|_| (Kind::TreatmentBay, pick(rng, 3400, 4000, 100))));
    items.extend((0..stations).map(|_| (Kind::TreatmentBay, pick(rng, 3800, 5800, 200))));
    items.extend((0..washrooms).map(|_| (Kind::Washroom, 2100)));
    rng.shuffle(&mut items);
    if shell {
        let at = if rng.below(2) == 0 { 0 } else { items.len() };
        items.insert(at, (Kind::Shell, 3300));
    }
    let fixed: i64 = items.iter().filter(|i| i.0 != Kind::TreatmentBay).map(|i| i.1).sum();
    let bays: i64 = items.iter().filter(|i| i.0 == Kind::TreatmentBay).map(|i| i.1).sum();
    let factor = (len - fixed) as f64 / bays as f64;
    if !(0.9..=1.4).contains(&factor) {
        return Err(format!("the window band of {len} mm cannot be made of bays at {factor:.2} of their size"));
    }
    for i in items.iter_mut().filter(|i| i.0 == Kind::TreatmentBay) {
        i.1 = round100(i.1 as f64 * factor);
    }
    let total: i64 = items.iter().map(|i| i.1).sum();
    let last = items.iter_mut().rev().find(|i| i.0 == Kind::TreatmentBay).ok_or("the window band has no bay")?;
    last.1 += len - total;
    Ok(items)
}

/// Where a block stands: its x range, the core's y range, and the corridors it touches.
struct Frame {
    x0: i64,
    x1: i64,
    y2: i64,
    y3: i64,
    cross: usize,
    patient: usize,
    end: usize,
}

/// A utility column against the cross corridor (medication, clean and soiled utility, each with a door to it), then treatment
/// rooms (a door to the patient corridor) and staff work (open to it, and sometimes a second one open to the first).
fn treatment_block(b: &mut Builder, rng: &mut Rng, f: &Frame, utility_w: i64, treatment_rooms: i64, staff_pair: bool) {
    let core = f.y3 - f.y2;
    let mut kinds = [Kind::Medication, Kind::CleanUtility, Kind::SoiledUtility];
    rng.shuffle(&mut kinds);
    let d = core / 3 / 100 * 100;
    let mut y = f.y2;
    for (k, kind) in kinds.into_iter().enumerate() {
        let y1 = if k == 2 { f.y3 } else { y + d };
        let i = b.room(kind, rect(f.x0, y, f.x0 + utility_w, y1));
        b.link(i, f.cross, Means::Single(if kind == Kind::Medication { SINGLE } else { SINGLE_WIDE }));
        y = y1;
    }
    let front = f.x1 - (f.x0 + utility_w);
    let extra = (front - treatment_rooms * 3400 - 2400) / 100 * 100;
    let share = extra / (treatment_rooms + 1) / 100 * 100;
    let mut parts: Vec<Option<i64>> = (0..treatment_rooms).map(|_| Some(3400 + share)).collect();
    parts.push(None);
    rng.shuffle(&mut parts);
    let mut x = f.x0 + utility_w;
    let last = parts.len() - 1;
    for (k, part) in parts.into_iter().enumerate() {
        let w = match part {
            Some(w) => w,
            None => front - parts_width(treatment_rooms, share),
        };
        let x1 = if k == last { f.x1 } else { x + w };
        match part {
            Some(_) => {
                let i = b.room(Kind::TreatmentRoom, rect(x, f.y2, x1, f.y3));
                b.link(i, f.patient, Means::Single(SINGLE_WIDE));
            }
            None if staff_pair => {
                let depth = 3000.min(core - 3000);
                let a = b.room(Kind::StaffWork, rect(x, f.y2, x1, f.y2 + depth));
                let c = b.room(Kind::StaffWork, rect(x, f.y2 + depth, x1, f.y3));
                b.link(a, f.patient, Means::Open);
                b.link(c, a, Means::Open);
            }
            None => {
                let a = b.room(Kind::StaffWork, rect(x, f.y2, x1, f.y3));
                b.link(a, f.patient, Means::Open);
            }
        }
        x = x1;
    }
}

fn parts_width(treatment_rooms: i64, share: i64) -> i64 {
    treatment_rooms * (3400 + share)
}

/// An isolation room on the patient corridor (a pair of doors) with an anteroom between it and the cross corridor, a washroom of its
/// own behind it, and against the cross corridor storage, an alcove and a staff washroom.
fn isolation_block(b: &mut Builder, rng: &mut Rng, f: &Frame, anteroom_w: i64) {
    let core = f.y3 - f.y2;
    let extra = (f.x1 - f.x0 - anteroom_w - 3800) / 100 * 100;
    let aw = anteroom_w + (extra / 3).min(600) / 100 * 100;
    let (ad, wd) = (pick(rng, 2800, 3400, 200), pick(rng, 2400, 3000, 200));
    let split = f.x0 + aw;
    let ante = b.room(Kind::Anteroom, rect(f.x0, f.y2, split, f.y2 + ad));
    let iso = b.room(Kind::Isolation, rect(split, f.y2, f.x1, f.y3 - wd));
    let wash = b.room(Kind::Washroom, rect(split, f.y3 - wd, f.x1, f.y3));
    b.link(ante, f.cross, Means::Single(SINGLE));
    b.link(ante, iso, Means::Single(SINGLE));
    b.link(iso, f.patient, Means::Pair);
    b.link(wash, iso, Means::Single(SINGLE));
    // Against the cross corridor, above the anteroom.
    let high = core - ad;
    let n = if high >= 3 * 2400 { 2 + rng.below(2) as i64 } else { 2_i64.min(high / 2000).max(1) };
    let mut kinds = [Kind::Storage, Kind::Washroom, Kind::Alcove];
    rng.shuffle(&mut kinds);
    let h = high / n / 100 * 100;
    let mut y = f.y2 + ad;
    for (k, kind) in kinds.into_iter().take(n as usize).enumerate() {
        let y1 = if k as i64 == n - 1 { f.y3 } else { y + h };
        let i = b.room(kind, rect(f.x0, y, split, y1));
        match kind {
            Kind::Alcove => b.link(i, f.cross, Means::Open),
            _ => b.link(i, f.cross, Means::Single(SINGLE)),
        }
        y = y1;
    }
}

/// Staff work open to the patient corridor, then shells and plant rooms stacked behind it, each with a door to the end corridor.
fn service_block(b: &mut Builder, rng: &mut Rng, f: &Frame) {
    let core = f.y3 - f.y2;
    let sw = pick(rng, 3000, 3600, 200);
    let a = b.room(Kind::StaffWork, rect(f.x0, f.y2, f.x1, f.y2 + sw));
    b.link(a, f.patient, Means::Open);
    let high = core - sw;
    let n = if high >= 3 * 2400 { 2 + rng.below(2) as i64 } else { 2 };
    let plants = 1 + rng.below(2) as i64;
    let plants = plants.min(n - 1).max(1);
    let mut kinds: Vec<Kind> = (0..n).map(|i| if i < plants { Kind::Plant } else { Kind::Shell }).collect();
    rng.shuffle(&mut kinds);
    let h = high / n / 100 * 100;
    let mut y = f.y2 + sw;
    for (k, kind) in kinds.into_iter().enumerate() {
        let y1 = if k as i64 == n - 1 { f.y3 } else { y + h };
        let i = b.room(kind, rect(f.x0, y, f.x1, y1));
        b.link(i, f.end, if kind == Kind::Plant { Means::Pair } else { Means::Single(SINGLE_WIDE) });
        y = y1;
    }
}

/// Offices (at least one), a meeting room and storage over a block, each with a door to the staff corridor.
fn office_row(b: &mut Builder, rng: &mut Rng, x0: i64, x1: i64, y0: i64, y1: i64, staff: usize) {
    let w = x1 - x0;
    let n = ((w + 1900) / 3800).max(1);
    let mut kinds: Vec<Kind> = (0..n)
        .map(|k| match rng.below(10) {
            _ if k == 0 => Kind::Office,
            0 | 1 => Kind::Storage,
            2 => Kind::Meeting,
            _ => Kind::Office,
        })
        .collect();
    rng.shuffle(&mut kinds);
    if !kinds.contains(&Kind::Office) {
        kinds[0] = Kind::Office;
    }
    let mut x = x0;
    for (k, kind) in kinds.iter().enumerate() {
        let left = n - k as i64;
        let cell = (x1 - x) / left / 100 * 100;
        let x_end = if left == 1 { x1 } else { x + cell };
        let i = b.room(*kind, rect(x, y0, x_end, y1));
        b.link(i, staff, Means::Single(if *kind == Kind::Meeting { SINGLE_WIDE } else { SINGLE }));
        x = x_end;
    }
}

type Piece = (bool, i64, i64, i64);

/// Walls, doors, room separation lines and the outline of a case, turned and mirrored by the seed.
pub fn assemble(c: &Case, seed: u64) -> Result<Layout, String> {
    let (len, depth) = (c.width, c.depth);
    let rects: Vec<Rect> = c.rooms.iter().map(|r| r.rect).collect();
    let gross: i64 = rects.iter().map(Rect::area).sum();
    if gross != len * depth {
        return Err(format!("the rooms cover {gross} mm2 of a plan of {} mm2", len * depth));
    }

    // Every edge of every room, cut where another room's corner lies on it: the stretches two rooms share.
    type Edges = Vec<(i64, i64, usize)>;
    let mut lines: BTreeMap<(bool, i64), Edges> = BTreeMap::new();
    for (i, r) in rects.iter().enumerate() {
        for (vertical, line, lo, hi) in
            [(true, r.x0, r.y0, r.y1), (true, r.x1, r.y0, r.y1), (false, r.y0, r.x0, r.x1), (false, r.y1, r.x0, r.x1)]
        {
            lines.entry((vertical, line)).or_default().push((lo, hi, i));
        }
    }
    let mut interior: Vec<(Piece, usize, usize)> = Vec::new();
    for ((vertical, line), edges) in &lines {
        let cuts: BTreeSet<i64> = edges.iter().flat_map(|(lo, hi, _)| [*lo, *hi]).collect();
        let cuts: Vec<i64> = cuts.into_iter().collect();
        for pair in cuts.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            let on: Vec<usize> = edges.iter().filter(|(lo, hi, _)| *lo <= p && q <= *hi).map(|e| e.2).collect();
            match on.as_slice() {
                [] | [_] => {}
                [a, b] => interior.push(((*vertical, *line, p, q), *a, *b)),
                _ => return Err("three rooms share a wall".into()),
            }
        }
    }
    let linked = |a: usize, b: usize| c.links.iter().find(|l| (l.a == a && l.b == b) || (l.a == b && l.b == a));
    for l in &c.links {
        if shared(&rects[l.a], &rects[l.b]).is_none() {
            return Err(format!("`{}` and `{}` have no wall in common for their link", c.rooms[l.a].id, c.rooms[l.b].id));
        }
    }
    // Walls where the pair is not open to each other, merged along a line only across stretches that touch.
    let mut stretches: Vec<Piece> =
        interior.iter().filter(|(_, a, b)| !linked(*a, *b).is_some_and(|l| l.means == Means::Open)).map(|(p, _, _)| *p).collect();
    stretches.sort();
    let mut merged: Vec<Piece> = Vec::new();
    for p in stretches {
        match merged.last_mut() {
            Some(last) if last.0 == p.0 && last.1 == p.1 && last.3 == p.2 => last.3 = p.3,
            _ => merged.push(p),
        }
    }
    // Separators: one for each open link, over the run its two rooms share.
    let mut seps: Vec<Piece> = Vec::new();
    for l in c.links.iter().filter(|l| l.means == Means::Open) {
        if let Some(run) = shared(&rects[l.a], &rects[l.b]) {
            seps.push(run);
        }
    }
    seps.sort();

    let door = |link: usize, a: usize, b: usize, at: [i64; 2], family: &'static str, width: i64, op: &'static str| DoorAt {
        link: Some(link),
        rooms: (a, Some(b)),
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
    let mut doors = Vec::new();
    for (k, l) in c.links.iter().enumerate() {
        let (family, width, op): (&'static str, i64, &'static str) = match l.means {
            Means::Open => continue,
            Means::Single(w) => ("Single Door", w, "swing"),
            Means::Pair => ("Double Door", PAIR, "double-swing"),
        };
        let Some((vertical, line, lo, hi)) = shared(&rects[l.a], &rects[l.b]) else { continue };
        let run = hi - lo;
        if run < width + 2 * DOOR_CLEAR {
            return Err(format!(
                "the wall of {run} mm between `{}` and `{}` is too short for its {width} mm door",
                c.rooms[l.a].id, c.rooms[l.b].id
            ));
        }
        doors.push(door(k, l.a, l.b, point(vertical, line, (lo + hi) / 2), family, width, op));
    }

    // The shell, one wall for each side.
    let shell: Vec<[i64; 2]> = vec![[0, 0], [len, 0], [len, depth], [0, depth]];
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
    let separators: Vec<[[i64; 2]; 2]> = seps
        .iter()
        .map(|s| {
            let (a, b) = corner(s);
            let (a, b) = ordered(mv(a), mv(b));
            [a, b]
        })
        .collect();
    let boundary = counter_clockwise(shell.into_iter().map(mv).collect());
    Ok(Layout { rects, doors, separators, walls, boundary, width: max_x - min_x, depth: max_y - min_y, gross_mm2: gross, quarters, mirror })
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

/// How much a room's kind wants a door to open into it: any room over a corridor.
fn pull(kind: Kind) -> u8 {
    if kind == Kind::Corridor { 1 } else { 2 }
}

/// Sets each door's flips, and what the extractor will say of it. A door opens into the room that is not a corridor; between two
/// corridors, into the one it is drawn from. Every door hangs from the start jamb. A door whose host wall is not found exactly once
/// is an error, so the try is skipped.
pub fn orient(c: &Case, l: &mut Layout) -> Result<(), String> {
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
        let (Some(x), Some(y)) = (left, right) else {
            return Err(format!("door D{} has no room on one side", i + 1));
        };
        let target = if pull(c.rooms[x].kind) >= pull(c.rooms[y].kind) { x } else { y };
        let face = target != x;
        let side = if face { y } else { x };
        let d = &mut l.doors[i];
        d.flip_facing = face;
        d.flip_hand = false;
        d.opens_into = Some(c.rooms[side].id.clone());
        d.hinge = Some("start");
    }
    Ok(())
}
