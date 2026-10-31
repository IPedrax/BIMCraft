//! The reference tower: an original procedural office tower of up to 100 storeys and about ten thousand elements a storey,
//! a million in all, the baseline of the next core ([docs/spatial-core-plan.md]). Never derived from a customer's model.
//!
//! One storey is built through the ordinary commands, its repeated parts laid out with `copy` (doubling, so a row of 28 is
//! five commands, not 28), then stacked: each storey's elements are fresh copies with ids and GUIDs of their own, their
//! levels and hosts pointed at the storey's, and their rooms, doors and windows numbered by storey.
//!
//! The plate is 84 × 60 m of 6 m bays. Round the facade, 3 m offices; inside them a 2 m ring corridor, which has the
//! inner block as an island; inside that a ring of 4 m meeting rooms; in the middle a core of service rooms divided by
//! crossing walls, with four lift shafts. Every room bounds, has a ceiling and a tag, and is furnished. Above the
//! ceilings run the services: luminaires with junction boxes and conduits, sprinklers with tees, hangers and branch
//! pipes, air terminals, fan coils with their pipes and valves, smoke detectors, a cable tray and lights round the
//! corridor, and the service rooms' water and drains.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Guid, Id, Level, Model};

/// The most storeys the tower has.
pub const MAX_STOREYS: u32 = 100;
/// Floor to floor, mm.
pub const STOREY: i64 = 4000;
/// The plate, mm.
pub const WIDTH: i64 = 84_000;
pub const DEPTH: i64 = 60_000;
/// The ceilings' height over their level, mm.
const CEILING: i64 = 2700;
const INTERIOR: &str = "Interior - 100mm";

/// The reference tower of `storeys` storeys (1 to [`MAX_STOREYS`]).
pub fn tower(storeys: u32) -> Result<Model, String> {
    if !(1..=MAX_STOREYS).contains(&storeys) {
        return Err(format!("tower: `storeys` must be between 1 and {MAX_STOREYS}"));
    }
    let mut s = Session::new();
    run(&mut s, "new", json!({"name": "Reference Tower"}))?;
    families(&mut s)?;
    structure(&mut s)?;
    facade(&mut s)?;
    partitions(&mut s)?;
    offices(&mut s)?;
    ring(&mut s)?;
    core(&mut s)?;
    services(&mut s)?;
    run(&mut s, "tag", json!({}))?;
    run(&mut s, "tag", json!({"category": "room"}))?;
    stack(&s.model, storeys)
}

fn run(s: &mut Session, cmd: &str, p: Value) -> Result<Value, String> {
    s.execute(cmd, &p).map_err(|e| format!("tower: {cmd} {p}: {e}"))
}

/// The ids a command made.
fn made(v: &Value) -> Vec<Id> {
    match v["ids"].as_array() {
        Some(a) => a.iter().filter_map(Value::as_u64).collect(),
        None => v["id"].as_u64().into_iter().collect(),
    }
}

fn make(s: &mut Session, cmd: &str, p: Value) -> Result<Vec<Id>, String> {
    run(s, cmd, p).map(|v| made(&v))
}

/// `ids` and `count - 1` copies of them, each `step` on from the last; all their ids. Doubling: a row of `count` takes
/// about 2 log₂ `count` commands.
fn array(s: &mut Session, ids: Vec<Id>, step: [i64; 2], count: i64) -> Result<Vec<Id>, String> {
    let by = |n: i64| [step[0] * n, step[1] * n];
    // powers[k]: the first 2^k of them.
    let mut powers = vec![ids.clone()];
    let mut all = ids;
    let mut have = 1;
    while have * 2 <= count {
        let copies = make(s, "copy", json!({"ids": all, "by": by(have)}))?;
        all.extend(copies);
        have *= 2;
        powers.push(all.clone());
    }
    for (k, set) in powers.iter().enumerate().rev() {
        let size = 1i64 << k;
        if have + size <= count {
            let copies = make(s, "copy", json!({"ids": set, "by": by(have)}))?;
            all.extend(copies);
            have += size;
        }
    }
    Ok(all)
}

fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Value {
    json!([[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}

fn wall(s: &mut Session, a: [i64; 2], b: [i64; 2]) -> Result<Vec<Id>, String> {
    make(s, "wall", json!({"points": [a, b], "height": STOREY, "type": INTERIOR}))
}

fn closed(s: &mut Session, x0: i64, y0: i64, x1: i64, y1: i64) -> Result<Vec<Id>, String> {
    make(s, "wall", json!({"points": rect(x0, y0, x1, y1), "closed": true, "height": STOREY, "type": INTERIOR}))
}

/// The points from `a` to `b` every `step` mm, and `b`.
fn line(a: [i64; 2], b: [i64; 2], step: i64) -> Vec<[i64; 2]> {
    let length = (b[0] - a[0]).abs() + (b[1] - a[1]).abs();
    let n = (length + step - 1) / step;
    (0..=n).map(|k| [a[0] + (b[0] - a[0]) * k / n, a[1] + (b[1] - a[1]) * k / n]).collect()
}

/// The tower's own families, simple original shapes: what the starter library has no family for.
fn families(s: &mut Session) -> Result<(), String> {
    let block = |x: i64, y: i64, h: i64| json!({"profile": rect(-x / 2, -y / 2, x / 2, y / 2), "from": 0, "to": h});
    for (name, form) in [
        ("Sprinkler", json!({"circle": {"centre": [0, 0], "radius": 30}, "from": 0, "to": 80})),
        ("Hanger", block(40, 40, 300)),
        ("Junction Box", block(100, 100, 60)),
        ("Smoke Detector", json!({"circle": {"centre": [0, 0], "radius": 60}, "from": 0, "to": 40})),
        ("Fan Coil", block(1000, 500, 300)),
    ] {
        run(s, "family", json!({"name": name, "category": "generic", "forms": [form], "types": {"Standard": {}}}))?;
    }
    Ok(())
}

/// The slab, a column at every grid crossing, a beam along every grid line, and the lift shafts.
fn structure(s: &mut Session) -> Result<(), String> {
    run(s, "floor", json!({"boundary": rect(0, 0, WIDTH, DEPTH), "structural": true}))?;
    let column = make(s, "structural_column", json!({"at": [0, 0], "height": STOREY}))?;
    let row = array(s, column, [6000, 0], WIDTH / 6000 + 1)?;
    array(s, row, [0, 6000], DEPTH / 6000 + 1)?;
    for y in (0..=DEPTH).step_by(6000) {
        run(s, "beam", json!({"points": line([0, y], [WIDTH, y], 6000), "offset": STOREY}))?;
    }
    for x in (0..=WIDTH).step_by(6000) {
        run(s, "beam", json!({"points": line([x, 0], [x, DEPTH], 6000), "offset": STOREY}))?;
    }
    // Four lifts in the core's middle cells.
    for (cx, cy) in [(40_000, 26_000), (44_000, 26_000), (40_000, 34_000), (44_000, 34_000)] {
        run(s, "shaft", json!({"boundary": rect(cx - 1300, cy - 1300, cx + 1300, cy + 1300), "height": STOREY}))?;
    }
    Ok(())
}

/// The facade, clockwise: 3 m panels, each with two windows.
fn facade(s: &mut Session) -> Result<(), String> {
    for (a, b) in [([0, 0], [0, DEPTH]), ([0, DEPTH], [WIDTH, DEPTH]), ([WIDTH, DEPTH], [WIDTH, 0]), ([WIDTH, 0], [0, 0])] {
        let length = (b[0] - a[0]).abs() + (b[1] - a[1]).abs();
        let u = [(b[0] - a[0]).signum(), (b[1] - a[1]).signum()];
        let at = |d: i64| [a[0] + u[0] * d, a[1] + u[1] * d];
        let panel = make(s, "wall", json!({"points": [a, at(3000)], "height": STOREY, "type": "Exterior - 300mm"}))?;
        run(s, "window", json!({"at": at(750)}))?;
        run(s, "window", json!({"at": at(2250)}))?;
        array(s, panel, [u[0] * 3000, u[1] * 3000], length / 3000)?;
    }
    Ok(())
}

/// The walls inside: the corridor's two rings, the core, and the walls between rooms.
fn partitions(s: &mut Session) -> Result<(), String> {
    let (w, d) = (WIDTH, DEPTH);
    closed(s, 6000, 6000, w - 6000, d - 6000)?;
    closed(s, 8000, 8000, w - 8000, d - 8000)?;
    closed(s, 14_000, 14_000, w - 14_000, d - 14_000)?;
    // Where the bands of rooms turn the corners.
    for (a, b) in [(0, 6000), (w - 6000, w)] {
        for y in [6000, d - 6000] {
            wall(s, [a, y], [b, y])?;
        }
    }
    for (a, b) in [(8000, 14_000), (w - 14_000, w - 8000)] {
        for y in [14_000, d - 14_000] {
            wall(s, [a, y], [b, y])?;
        }
    }
    // Offices, every 3 m.
    for (y0, y1) in [(0, 6000), (d - 6000, d)] {
        let first = wall(s, [3000, y0], [3000, y1])?;
        array(s, first, [3000, 0], w / 3000 - 1)?;
    }
    for (x0, x1) in [(0, 6000), (w - 6000, w)] {
        let first = wall(s, [x0, 9000], [x1, 9000])?;
        array(s, first, [0, 3000], (d - 12_000) / 3000 - 1)?;
    }
    // The ring of meeting rooms, every 4 m.
    for (y0, y1) in [(8000, 14_000), (d - 14_000, d - 8000)] {
        let first = wall(s, [12_000, y0], [12_000, y1])?;
        array(s, first, [4000, 0], (w - 16_000) / 4000 - 1)?;
    }
    for (x0, x1) in [(8000, 14_000), (w - 14_000, w - 8000)] {
        let first = wall(s, [x0, 18_000], [x1, 18_000])?;
        array(s, first, [0, 4000], (d - 28_000) / 4000 - 1)?;
    }
    // The core: 4 m cells in rows of 8 m, the rows' walls crossing the cells'.
    let first = wall(s, [18_000, 14_000], [18_000, d - 14_000])?;
    array(s, first, [4000, 0], (w - 28_000) / 4000 - 1)?;
    for y in (22_000..d - 14_000).step_by(8000) {
        wall(s, [14_000, y], [w - 14_000, y])?;
    }
    Ok(())
}

/// What a room holds, in its own terms: `u` along its row, `v` in from the wall its row starts at.
enum Item {
    Room(&'static str, i64, i64),
    Ceiling(i64, i64),
    /// A family at a point, turned degrees, `offset` mm up.
    Component(&'static str, i64, i64, f64, i64),
    /// A pipe from one point to another, `offset` mm up.
    Pipe(i64, i64, i64, i64, i64),
}

/// A row of rooms: its first room's corner, the way the row runs and the way into the rooms (unit steps), and how much
/// what is placed turns.
struct Row {
    origin: [i64; 2],
    along: [i64; 2],
    inward: [i64; 2],
    turn: f64,
}

impl Row {
    fn at(&self, u: i64, v: i64) -> [i64; 2] {
        [self.origin[0] + self.along[0] * u + self.inward[0] * v, self.origin[1] + self.along[1] * u + self.inward[1] * v]
    }

    /// The first room's `items` placed, then the row of `count`, `width` apart.
    fn lay(&self, s: &mut Session, items: &[Item], width: i64, count: i64) -> Result<(), String> {
        let mut ids = Vec::new();
        for item in items {
            ids.extend(match *item {
                Item::Room(name, u, v) => make(s, "room", json!({"point": self.at(u, v), "name": name}))?,
                Item::Ceiling(u, v) => make(s, "ceiling", json!({"point": self.at(u, v), "offset": CEILING}))?,
                Item::Component(family, u, v, angle, offset) => make(
                    s,
                    "component",
                    json!({"family": family, "at": self.at(u, v), "angle": (angle + self.turn) % 360.0, "offset": offset}),
                )?,
                Item::Pipe(u0, v0, u1, v1, offset) => {
                    make(s, "pipe", json!({"points": [self.at(u0, v0), self.at(u1, v1)], "offset": offset}))?
                }
            });
        }
        array(s, ids, [self.along[0] * width, self.along[1] * width], count)?;
        Ok(())
    }

    /// A door in the wall at `v`, `u` along the first room, then one for each of `count` rooms `width` apart.
    fn doors(&self, s: &mut Session, u: i64, v: i64, width: i64, count: i64) -> Result<(), String> {
        let door = make(s, "door", json!({"at": self.at(u, v)}))?;
        array(s, door, [self.along[0] * width, self.along[1] * width], count)?;
        Ok(())
    }
}

/// The four sides' rows of 3 m offices, 6 m deep, and their doors onto the corridor.
fn offices(s: &mut Session) -> Result<(), String> {
    use Item::*;
    let (w, d) = (WIDTH, DEPTH);
    let office = [
        Room("Office", 1500, 3000),
        Ceiling(1500, 3000),
        Component("Desk", 1500, 1400, 0.0, 0),
        Component("Chair", 1500, 2200, 180.0, 0),
        Component("Desk", 1500, 3800, 0.0, 0),
        Component("Chair", 1500, 4600, 180.0, 0),
        Component("Base Cabinet", 2300, 5500, 180.0, 0),
        Component("Outlet", 300, 1400, 0.0, 300),
        Component("Outlet", 2700, 1400, 0.0, 300),
        Component("Outlet", 300, 3800, 0.0, 300),
        Component("Outlet", 2700, 3800, 0.0, 300),
        Component("Data Outlet", 300, 1700, 0.0, 300),
        Component("Data Outlet", 300, 4100, 0.0, 300),
        Component("Switch", 2500, 5900, 180.0, 1100),
        Component("Smoke Detector", 1500, 3000, 0.0, CEILING),
        Component("Fan Coil", 1500, 700, 0.0, 2900),
        Pipe(1300, 1000, 1300, 5600, 3100),
        Pipe(1700, 1000, 1700, 5600, 3100),
        Component("Gate Valve", 1300, 5300, 90.0, 3100),
        Component("Gate Valve", 1700, 5300, 90.0, 3100),
    ];
    let rows = [
        (Row { origin: [0, 0], along: [1, 0], inward: [0, 1], turn: 0.0 }, w / 3000, 2),
        (Row { origin: [0, d], along: [1, 0], inward: [0, -1], turn: 180.0 }, w / 3000, 2),
        (Row { origin: [0, 6000], along: [0, 1], inward: [1, 0], turn: 270.0 }, (d - 12_000) / 3000, 0),
        (Row { origin: [w, 6000], along: [0, 1], inward: [-1, 0], turn: 90.0 }, (d - 12_000) / 3000, 0),
    ];
    for (row, count, closed_off) in rows {
        row.lay(s, &office, 3000, count)?;
        // The corner offices on the long sides open off their neighbours, not the corridor.
        let first = Row { origin: row.at(3000 * closed_off, 0), ..row };
        first.doors(s, 1500, 6000, 3000, count - 2 * closed_off)?;
    }
    run(s, "room", json!({"point": [7000, 7000], "name": "Corridor"}))?;
    run(s, "ceiling", json!({"point": [7000, 7000], "offset": CEILING}))?;
    Ok(())
}

/// The ring of meeting rooms round the core, and their doors onto the corridor.
fn ring(s: &mut Session) -> Result<(), String> {
    use Item::*;
    let (w, d) = (WIDTH, DEPTH);
    let meeting = [
        Room("Meeting", 2000, 3000),
        Ceiling(2000, 3000),
        Component("Table", 2000, 3000, 90.0, 0),
        Component("Chair", 1400, 2200, 0.0, 0),
        Component("Chair", 2600, 2200, 0.0, 0),
        Component("Chair", 1400, 3800, 180.0, 0),
        Component("Chair", 2600, 3800, 180.0, 0),
        Component("Outlet", 600, 300, 0.0, 300),
        Component("Outlet", 3400, 300, 0.0, 300),
        Component("Data Outlet", 900, 300, 0.0, 300),
        Component("Switch", 3000, 100, 0.0, 1100),
        Component("Smoke Detector", 2000, 3000, 0.0, CEILING),
    ];
    for (row, count) in [
        (Row { origin: [8000, 8000], along: [1, 0], inward: [0, 1], turn: 0.0 }, (w - 16_000) / 4000),
        (Row { origin: [8000, d - 8000], along: [1, 0], inward: [0, -1], turn: 180.0 }, (w - 16_000) / 4000),
        (Row { origin: [8000, 14_000], along: [0, 1], inward: [1, 0], turn: 270.0 }, (d - 28_000) / 4000),
        (Row { origin: [w - 8000, 14_000], along: [0, 1], inward: [-1, 0], turn: 90.0 }, (d - 28_000) / 4000),
    ] {
        row.lay(s, &meeting, 4000, count)?;
        row.doors(s, 2000, 0, 4000, count)?;
    }
    Ok(())
}

/// The core's service rooms, 4 × 8 m, in rows: the outer rows open onto the meeting rooms, the inner ones onto the outer.
fn core(s: &mut Session) -> Result<(), String> {
    use Item::*;
    let (w, d) = (WIDTH, DEPTH);
    let service = [
        Room("Service", 2000, 4000),
        Ceiling(2000, 4000),
        Component("WC", 1000, 7500, 180.0, 0),
        Component("WC", 3000, 7500, 180.0, 0),
        Component("Kitchen Sink", 2000, 7600, 180.0, 0),
        Component("Switch", 3000, 300, 0.0, 1100),
        Component("Outlet", 3800, 2000, 90.0, 300),
        Component("Data Outlet", 3800, 2400, 90.0, 300),
        Component("Smoke Detector", 2000, 4000, 0.0, CEILING),
        Component("Floor Drain", 2000, 6000, 0.0, 0),
        Pipe(500, 7000, 3500, 7000, 600),
        Component("Gate Valve", 700, 7000, 0.0, 600),
        Pipe(500, 7800, 3500, 7800, 100),
    ];
    let count = (w - 28_000) / 4000;
    for y in (14_000..d - 14_000).step_by(8000) {
        let row = Row { origin: [14_000, y], along: [1, 0], inward: [0, 1], turn: 0.0 };
        row.lay(s, &service, 4000, count)?;
    }
    let top = d - 14_000;
    Row { origin: [14_000, 14_000], along: [1, 0], inward: [0, 1], turn: 0.0 }.doors(s, 1000, 0, 4000, count)?;
    Row { origin: [14_000, top], along: [1, 0], inward: [0, 1], turn: 0.0 }.doors(s, 1000, 0, 4000, count)?;
    Row { origin: [14_000, 22_000], along: [1, 0], inward: [0, 1], turn: 0.0 }.doors(s, 2000, 0, 4000, count)?;
    Row { origin: [14_000, top - 8000], along: [1, 0], inward: [0, 1], turn: 0.0 }.doors(s, 2000, 0, 4000, count)?;
    Ok(())
}

/// What runs above the ceilings over the whole plate.
fn services(s: &mut Session) -> Result<(), String> {
    let (w, d) = (WIDTH, DEPTH);
    let component = |family: &str, at: [i64; 2], offset: i64| json!({"family": family, "at": at, "offset": offset});
    // Luminaires on a 2.4 m grid, each with its junction box; a conduit along each row.
    let (nx, ny) = (w / 2400, d / 2400);
    let mut light = make(s, "component", component("Recessed Luminaire", [1200, 1200], CEILING))?;
    light.extend(make(s, "component", component("Junction Box", [1200, 1500], 2900))?);
    let row = array(s, light, [2400, 0], nx)?;
    array(s, row, [0, 2400], ny)?;
    let conduit = make(s, "conduit", json!({"points": line([1200, 1500], [1200 + 2400 * (nx - 1), 1500], 2400), "offset": 2950}))?;
    array(s, conduit, [0, 2400], ny)?;
    // Sprinklers on a 3 m grid, each on a tee with a hanger beside it; a branch pipe along each row, and a main.
    let (nx, ny) = (w / 3000, d / 3000);
    let mut head = make(s, "component", component("Sprinkler", [1500, 1500], 2620))?;
    head.extend(make(s, "component", component("Tee", [1500, 1500], 3000))?);
    head.extend(make(s, "component", component("Hanger", [3000, 1500], 3000))?);
    let row = array(s, head, [3000, 0], nx)?;
    array(s, row, [0, 3000], ny)?;
    let branch = make(s, "pipe", json!({"points": line([1500, 1500], [1500 + 3000 * (nx - 1), 1500], 3000), "offset": 3000}))?;
    array(s, branch, [0, 3000], ny)?;
    run(s, "pipe", json!({"points": line([1500, 1500], [1500, 1500 + 3000 * (ny - 1)], 3000), "offset": 3000}))?;
    // Supply and return air terminals in every bay.
    let mut air = make(s, "component", component("Air Terminal", [3000, 1500], CEILING))?;
    air.extend(make(s, "component", component("Air Terminal", [3000, 4500], CEILING))?);
    let row = array(s, air, [6000, 0], w / 6000)?;
    array(s, row, [0, 6000], d / 6000)?;
    // A cable tray round the middle of the corridor, and its smoke detectors.
    let (x0, y0, x1, y1) = (7000, 7000, w - 7000, d - 7000);
    let mut ring = line([x0, y0], [x1, y0], 6000);
    for (a, b) in [([x1, y0], [x1, y1]), ([x1, y1], [x0, y1]), ([x0, y1], [x0, y0])] {
        ring.extend(line(a, b, 6000).into_iter().skip(1));
    }
    ring.pop();
    run(s, "cable_tray", json!({"points": ring, "closed": true, "offset": 3300}))?;
    for p in &ring {
        run(s, "component", component("Smoke Detector", *p, CEILING))?;
        run(s, "component", component("Ceiling Luminaire", [p[0] + 500, p[1] + 500], CEILING))?;
    }
    Ok(())
}

/// `storeys` storeys of `source`'s, which stand on its first level: fresh copies of its elements on each storey's
/// level, with their references to each other pointed at each other, numbered by storey.
fn stack(source: &Model, storeys: u32) -> Result<Model, String> {
    let source_levels: Vec<Guid> = source.levels().iter().map(|(id, _)| source.guid(*id)).collect();
    let mut m = source.clone();
    m.elements.clear();
    for floor in 0..=storeys {
        m.insert(Element::Level(Level::new(format!("Level {}", floor + 1), (i64::from(floor) * STOREY) as f64)));
    }
    for e in source.elements.values().filter(|e| matches!(e, Element::View3D(_))) {
        m.insert(e.clone());
    }
    let levels: Vec<Guid> = m.levels().iter().map(|(id, _)| m.guid(*id)).collect();
    for storey in 0..storeys as usize {
        let mut map: BTreeMap<Guid, Guid> = BTreeMap::new();
        for (k, level) in source_levels.iter().enumerate() {
            let to = levels.get(storey + k).ok_or("tower: a storey's elements reach past the top level")?;
            map.insert(*level, *to);
        }
        let mut inserted = Vec::new();
        let mut numbers: BTreeMap<&str, usize> = BTreeMap::new();
        for (&old, e) in &source.elements {
            if matches!(e, Element::Level(_) | Element::View3D(_)) {
                continue;
            }
            let mut e = e.clone();
            let number = |kind: &'static str, numbers: &mut BTreeMap<&str, usize>| {
                let n = numbers.entry(kind).or_default();
                *n += 1;
                format!("{}{:03}", storey + 1, n)
            };
            match &mut e {
                Element::Room(r) => r.number = number("room", &mut numbers),
                Element::Door(o) => o.mark = number("door", &mut numbers),
                Element::Window(o) => o.mark = number("window", &mut numbers),
                _ => {}
            }
            let id = m.insert(e);
            map.insert(source.guid(old), m.guid(id));
            inserted.push(id);
        }
        for id in inserted {
            if let Some(e) = m.elements.get_mut(id) {
                e.refs_mut(&mut |g| {
                    if let Some(new) = map.get(g) {
                        *g = *new;
                    }
                });
            }
        }
    }
    m.sync_library();
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Room;

    fn rooms(m: &Model) -> Vec<&Room> {
        m.elements
            .values()
            .filter_map(|e| match e {
                Element::Room(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_tower_storey_is_about_ten_thousand_elements_and_every_room_bounds() {
        let m = tower(2).unwrap();
        m.check().unwrap();
        assert_eq!(m.levels().len(), 3);
        let n = m.elements.len();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for e in m.elements.values() {
            *counts.entry(e.category()).or_default() += 1;
        }
        assert!((19_800..20_400).contains(&n), "{n} elements in two storeys: {counts:?}");
        for category in [
            "wall",
            "window",
            "door",
            "room",
            "ceiling",
            "floor",
            "structuralcolumn",
            "beam",
            "shaft",
            "component",
            "pipe",
            "conduit",
            "cabletray",
            "tag",
        ] {
            assert!(m.elements.values().any(|e| e.category() == category), "{category}");
        }
        let all = rooms(&m);
        assert_eq!(all.len(), 2 * 195);
        for r in &all {
            assert!(m.room_region(r).is_some(), "room {} ({}) isn't enclosed", r.number, r.name);
        }
        let numbers: std::collections::BTreeSet<&str> = all.iter().map(|r| r.number.as_str()).collect();
        assert_eq!(numbers.len(), all.len(), "room numbers are the project's own");
        assert!(numbers.contains("1001") && numbers.contains("2195"));
        // The corridor runs round the inner block, an island of it: between the two rings' centre lines.
        let corridor = all.iter().find(|r| r.name == "Corridor").unwrap();
        let area = m.room_region(corridor).unwrap().area();
        let ring = (WIDTH - 12_000) * (DEPTH - 12_000) - (WIDTH - 16_000) * (DEPTH - 16_000);
        assert!((area - ring as f64).abs() < 1e3, "{area} mm² against {ring}");
    }

    #[test]
    fn a_tower_is_one_to_a_hundred_storeys() {
        assert!(tower(0).is_err() && tower(MAX_STOREYS + 1).is_err());
    }
}
