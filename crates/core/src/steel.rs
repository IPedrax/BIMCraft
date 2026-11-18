//! Steel: the shape and the mark of a bar, the steel list and the cut plan. Nothing here is stored. A mark, a shape and a list are
//! worked out from the bars when asked, so a changed bar changes them and a file means the same thing to every version.
//!
//! A bar's shape is its legs (the lengths between its points), the angle it turns after each leg and the angle between the plane of
//! each pair of legs and the next. Bars of one grade, one diameter and one shape are the same bar, and share a mark inside their
//! host: `N1`, `N2` and so on, bottom bars first and bars drawn by hand last. A bar with no host is marked among the bars of its level.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use bimcraft_i18n::msg;
use serde_json::{Value, json};

use crate::model::{Element, Guid, Id, Model, Rebar, TOL};
use crate::rebar;
use crate::schedule::{Field, Filter, Kind, Op, Sort};
use crate::view3d::P3;

/// What a mark starts with; its number follows (`N1`).
pub const PREFIX: &str = "N";
/// The most legs a shape's label names before it says `...`.
pub const MAX_LABEL_LEGS: usize = 8;
/// The most rows a steel list holds, the most pieces a cut plan packs and the most patterns it lists.
pub const MAX_ROWS: usize = 100_000;
pub const MAX_PIECES: usize = 100_000;
pub const MAX_PATTERNS: usize = 10_000;
/// A closed bar with more legs than this is keyed from its first corner, not from the smallest of all its corners.
const MAX_CORNERS: usize = 64;
/// A leg shorter than this has no length: the same point twice.
const NO_LENGTH: f64 = 1e-6;

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}

/// The angle in degrees between two vectors; 0 where either has no length.
fn angle(a: P3, b: P3) -> f64 {
    let (la, lb) = (norm(a), norm(b));
    if la < NO_LENGTH || lb < NO_LENGTH {
        return 0.0;
    }
    (dot(a, b) / (la * lb)).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Each element of `v` with the next: for a closed ring the last with the first, else the pairs that touch.
fn pairs<T: Copy>(v: &[T], closed: bool) -> Vec<(T, T)> {
    if closed {
        v.iter().copied().zip(v.iter().copied().cycle().skip(1)).take(v.len()).collect()
    } else {
        v.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect()
    }
}

/// What a bar's points make of it, apart from where it lies.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// The last point is the first: a hoop.
    pub closed: bool,
    /// The lengths between its points, mm. A hoop's last leg is the one that closes it.
    pub legs: Vec<f64>,
    /// The angle in degrees it turns after each leg: one fewer than the legs for an open bar, and for a hoop one for each leg, the
    /// last leading back to the first.
    pub turns: Vec<f64>,
    /// The angle in degrees between the plane of the two legs before a leg and the plane of the two after it, `twists[k]` for leg
    /// `k + 1`: a bar in one plane that turns the same way all along has 0, one that turns back on itself 180. An open bar has two
    /// fewer than its legs, since its first and last legs have a bend at one end only.
    pub twists: Vec<f64>,
}

/// The shape of a bar along `path`.
pub fn shape(path: &[P3]) -> Shape {
    let mut points: Vec<P3> = Vec::with_capacity(path.len());
    for p in path {
        if points.last().is_none_or(|q| norm(sub(*p, *q)) > NO_LENGTH) {
            points.push(*p);
        }
    }
    let closed = points.len() >= 4 && points.first().zip(points.last()).is_some_and(|(a, b)| norm(sub(*a, *b)) <= TOL);
    if closed {
        points.pop();
    }
    let count = if closed { points.len() } else { points.len().saturating_sub(1) };
    let dir: Vec<P3> = points.iter().zip(points.iter().cycle().skip(1)).take(count).map(|(a, b)| sub(*b, *a)).collect();
    let legs = dir.iter().map(|d| norm(*d)).collect();
    let turns = pairs(&dir, closed).into_iter().map(|(a, b)| angle(a, b)).collect();
    let normals: Vec<P3> = pairs(&dir, closed).into_iter().map(|(a, b)| cross(a, b)).collect();
    let twists = pairs(&normals, closed).into_iter().map(|(a, b)| angle(a, b)).collect();
    Shape { closed, legs, turns, twists }
}

fn whole(x: f64) -> i64 {
    x.round() as i64
}

impl Shape {
    /// The shape as the numbers two bars share when they are the same bar: lengths to the millimetre and angles to the degree, an
    /// open bar read from either end (the smaller kept), a hoop from any corner either way round (the smallest kept), and turns and
    /// twists unsigned, since a bar and its mirror image are one bar turned over.
    pub fn key(&self) -> Vec<i64> {
        let n = self.legs.len();
        let legs: Vec<i64> = self.legs.iter().map(|x| whole(*x)).collect();
        let turns: Vec<i64> = self.turns.iter().map(|x| whole(*x)).collect();
        let twists: Vec<i64> = self.twists.iter().map(|x| whole(*x)).collect();
        let head = vec![i64::from(self.closed), i64::try_from(n).unwrap_or(i64::MAX)];
        if !self.closed {
            let both = |l: &[i64], t: &[i64], w: &[i64]| [head.clone(), l.to_vec(), t.to_vec(), w.to_vec()].concat();
            let back = |v: &[i64]| v.iter().rev().copied().collect::<Vec<i64>>();
            return both(&legs, &turns, &twists).min(both(&back(&legs), &back(&turns), &back(&twists)));
        }
        // Leg `j` of a hoop with the turn at its far end and its own twist; read the other way round, the turn is the one at its start.
        let at = |v: &[i64], j: usize| v.get(j % n.max(1)).copied().unwrap_or(0);
        let before = |j: usize| (j + n.saturating_sub(1)) % n.max(1);
        let read = |start: usize, forward: bool| -> Vec<i64> {
            let mut key = head.clone();
            for k in 0..n {
                let j = if forward { (start + k) % n } else { (start + n - k) % n };
                let turn = if forward { at(&turns, j) } else { at(&turns, before(j)) };
                key.extend([at(&legs, j), turn, at(&twists, before(j))]);
            }
            key
        };
        let starts = if n <= MAX_CORNERS { n } else { 1 };
        (0..starts).flat_map(|s| [read(s, true), read(s, false)]).min().unwrap_or_default()
    }

    /// The legs said in millimetres, joined by ` x `, with `closed` first for a hoop: `500 x 300`, `closed 140 x 440 x 140 x 440`.
    /// There is no letter code for a shape: a code is a guess at what the engineer calls a bar, and the legs say it.
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = self.legs.iter().take(MAX_LABEL_LEGS).map(|l| whole(*l).to_string()).collect();
        if self.legs.len() > MAX_LABEL_LEGS {
            parts.push("...".into());
        }
        let text = parts.join(" x ");
        if self.closed { format!("closed {text}") } else { text }
    }
}

/// How a layer ranks when marks are numbered: the layers `reinforce` makes, in the order a detailer reads a section, then any
/// other layer by name, then a bar drawn by hand (it has none).
fn rank(layer: &str) -> (u8, &str) {
    match layer {
        "bottom" => (0, ""),
        "bent" => (1, ""),
        "top" => (2, ""),
        "skin" => (3, ""),
        "bars" => (4, ""),
        "stirrup" => (5, ""),
        "tie" => (6, ""),
        "" => (8, ""),
        other => (7, other),
    }
}

/// The number of each of `bars`' marks, which bars of one grade, diameter and shape share. Marks are numbered by the lowest layer
/// among a mark's bars, then diameter (the thickest first), then total leg length (the longest first), then the key itself, so
/// reinforcing a host again gives the same marks.
fn number(bars: &[(Id, &Rebar)]) -> BTreeMap<Id, usize> {
    struct Mark {
        rank: (u8, String),
        legs: i64,
        ids: Vec<Id>,
    }
    // Grade, diameter in hundredths and shape key.
    type Key = (String, i64, Vec<i64>);
    let mut marks: BTreeMap<Key, Mark> = BTreeMap::new();
    for (id, rb) in bars {
        let shape = shape(&rb.path);
        let (r, name) = rank(&rb.layer);
        let key = (rb.grade.clone(), whole(rb.diameter * 100.0), shape.key());
        let mark =
            marks.entry(key).or_insert_with(|| Mark { rank: (r, name.to_string()), legs: whole(shape.legs.iter().sum()), ids: Vec::new() });
        if (r, name) < (mark.rank.0, mark.rank.1.as_str()) {
            mark.rank = (r, name.to_string());
        }
        mark.ids.push(*id);
    }
    let mut list: Vec<(&Key, &Mark)> = marks.iter().collect();
    list.sort_by(|(ka, a), (kb, b)| a.rank.cmp(&b.rank).then(kb.1.cmp(&ka.1)).then(b.legs.cmp(&a.legs)).then(ka.cmp(kb)));
    list.iter().enumerate().flat_map(|(i, (_, mark))| mark.ids.iter().map(move |id| (*id, i + 1))).collect()
}

/// What a mark of number `n` reads: `N3`.
pub fn mark_text(n: usize) -> String {
    format!("{PREFIX}{n}")
}

/// What bars are marked among: a host's, or, for bars with none, their level's.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Group {
    Host(Guid),
    Level(Guid),
}

/// The model's bars by what they are marked among, found by one look at every element, and each group's marks the first time they
/// are asked for. One look at many bars (a schedule, `elements`) shares one of these through [`crate::rooms::Rooms::takeoff`].
pub struct Bars {
    of: HashMap<Id, Group>,
    groups: HashMap<Group, Vec<Id>>,
    marks: RefCell<HashMap<Group, Rc<BTreeMap<Id, usize>>>>,
}

impl Bars {
    pub fn new(m: &Model) -> Bars {
        let (mut of, mut groups): (HashMap<Id, Group>, HashMap<Group, Vec<Id>>) = (HashMap::new(), HashMap::new());
        for (id, e) in m.elements.iter() {
            if let Element::Rebar(rb) = e {
                let group = rb.host.map_or(Group::Level(rb.level), Group::Host);
                of.insert(*id, group);
                groups.entry(group).or_default().push(*id);
            }
        }
        Bars { of, groups, marks: RefCell::new(HashMap::new()) }
    }

    /// The number of bar `id`'s mark among its group's.
    pub fn number(&self, m: &Model, id: Id) -> Option<usize> {
        let group = *self.of.get(&id)?;
        let known = self.marks.borrow().get(&group).cloned();
        let marks = match known {
            Some(marks) => marks,
            None => {
                let bars: Vec<(Id, &Rebar)> = self
                    .groups
                    .get(&group)?
                    .iter()
                    .filter_map(|b| match m.elements.get(*b) {
                        Some(Element::Rebar(rb)) => Some((*b, rb)),
                        _ => None,
                    })
                    .collect();
                let marks = Rc::new(number(&bars));
                self.marks.borrow_mut().insert(group, Rc::clone(&marks));
                marks
            }
        };
        marks.get(&id).copied()
    }
}

/// The host's `Mark` parameter, which is empty where the bar has no host or the host has none.
pub fn host_mark(m: &Model, rb: &Rebar) -> String {
    let host = rb.host.and_then(|h| m.elements.get(h));
    host.and_then(Element::params).and_then(|p| p.get("Mark")).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// What a bar adds to its element JSON: its mark, its host's mark, its shape and the shape said.
pub fn describe(m: &Model, bars: &Bars, id: Id, rb: &Rebar) -> [(&'static str, Value); 4] {
    let shape = shape(&rb.path);
    let mark = bars.number(m, id).map(mark_text).unwrap_or_default();
    [
        ("mark", json!(mark)),
        ("host_mark", json!(host_mark(m, rb))),
        ("shape", json!({"closed": shape.closed, "legs": shape.legs, "turns": shape.turns, "twists": shape.twists})),
        ("shape_label", json!(shape.label())),
    ]
}

/// The bars a list or a plan is of: those on `level`, and those among `only` (a host stands for its bars, a bar for itself).
#[derive(Default)]
pub struct Scope {
    pub level: Option<Guid>,
    pub only: Option<BTreeSet<Id>>,
}

impl Scope {
    fn takes(&self, m: &Model, id: Id, rb: &Rebar) -> bool {
        self.level.is_none_or(|l| rb.level == l)
            && self
                .only
                .as_ref()
                .is_none_or(|set| set.contains(&id) || rb.host.and_then(|h| m.elements.id(h)).is_some_and(|h| set.contains(&h)))
    }
}

/// A bar in a scope, with its mark's number and its cut length.
pub struct Item<'a> {
    pub id: Id,
    pub bar: &'a Rebar,
    pub number: usize,
    pub length: f64,
}

/// The bars in `scope`, in order of id.
pub fn items<'a>(m: &'a Model, bars: &Bars, scope: &Scope) -> Vec<Item<'a>> {
    m.elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Rebar(rb) if scope.takes(m, *id, rb) => {
                Some(Item { id: *id, bar: rb, number: bars.number(m, *id)?, length: rebar::length(&rb.path) })
            }
            _ => None,
        })
        .collect()
}

/// A row of the steel list: the bars of one host (or, with none, of one level) that share a mark.
pub struct Row {
    pub host: Option<Id>,
    pub host_mark: String,
    pub level: String,
    pub mark: String,
    pub grade: String,
    pub diameter: f64,
    pub shape: String,
    pub count: usize,
    /// One bar's length: the first's, where the bars of a mark differ by under a millimetre.
    pub length: f64,
    pub total_length: f64,
    pub weight_kg: f64,
    pub bars: Vec<Id>,
}

fn level_name(m: &Model, level: Guid) -> String {
    match m.elements.get(level) {
        Some(Element::Level(l)) => l.name.clone(),
        _ => String::new(),
    }
}

/// The rows of the steel list of `items`: by host, then mark. A list longer than [`MAX_ROWS`] is refused rather than cut.
pub fn rows(m: &Model, items: &[Item]) -> Result<Vec<Row>, String> {
    let mut by: BTreeMap<(u8, Id, usize), Vec<&Item>> = BTreeMap::new();
    for item in items {
        let group = match item.bar.host {
            Some(h) => (0, m.elements.id(h).unwrap_or(0)),
            None => (1, m.elements.id(item.bar.level).unwrap_or(0)),
        };
        by.entry((group.0, group.1, item.number)).or_default().push(item);
    }
    if by.len() > MAX_ROWS {
        return Err(format!(
            "the list has {} rows, more than the {MAX_ROWS} a steel list holds: ask for one level or some hosts",
            by.len()
        ));
    }
    Ok(by
        .into_iter()
        .filter_map(|((kind, host, number), list)| {
            let first = list.first()?;
            Some(Row {
                host: (kind == 0).then_some(host),
                host_mark: host_mark(m, first.bar),
                level: level_name(m, first.bar.level),
                mark: mark_text(number),
                grade: first.bar.grade.clone(),
                diameter: first.bar.diameter,
                shape: shape(&first.bar.path).label(),
                count: list.len(),
                length: first.length,
                total_length: list.iter().map(|i| i.length).sum(),
                weight_kg: list.iter().map(|i| rebar::weight_kg(i.bar.diameter, i.length)).sum(),
                bars: list.iter().map(|i| i.id).collect(),
            })
        })
        .collect())
}

/// A piece to cut from a stock bar: one bar, with its host's mark and its own.
#[derive(Clone)]
pub struct Piece {
    pub bar: Id,
    pub host_mark: String,
    pub mark: String,
    pub length: f64,
}

/// What a cut plan is told, in mm: the stock bars' length, what a cut wastes, and the shortest leftover worth keeping (0: none is).
pub struct Stock {
    pub length: f64,
    pub kerf: f64,
    pub min_offcut: f64,
}

/// One cutting pattern: identical stock bars are merged into one with a `count`, and each piece holds the ids of its bars in all of them.
pub struct Pattern {
    pub count: usize,
    pub pieces: Vec<(String, String, f64, Vec<Id>)>,
    /// What is left of a stock bar after its pieces, mm.
    pub offcut: f64,
}

pub struct Packed {
    pub stock_bars: usize,
    pub used: f64,
    pub waste: f64,
    /// The leftovers of at least the minimum, mm.
    pub reusable: Vec<f64>,
    pub patterns: Vec<Pattern>,
    /// The pieces longer than a stock bar, which are not planned: a lap is the engineer's to design.
    pub too_long: Vec<Piece>,
}

/// Hundredths of a millimetre, whole, so that no length is compared as a float.
fn units(x: f64) -> i64 {
    whole(x.clamp(0.0, 1e12) * 100.0)
}

fn millimetres(u: i64) -> f64 {
    u as f64 / 100.0
}

/// `pieces` cut from stock bars, by best fit decreasing: longest first (ties by bar id), each into the stock bar whose free length
/// is the smallest that still holds it (ties by the lowest bar), and a new stock bar when none does. A piece costs its length and a
/// cut, and a stock bar holds its length and one cut, so a bar with `n` pieces has `n - 1` cuts in it. It claims no optimum.
pub fn pack(mut pieces: Vec<Piece>, stock: &Stock) -> Packed {
    let (length, kerf, keep) = (units(stock.length), units(stock.kerf), units(stock.min_offcut));
    pieces.sort_by(|a, b| b.length.total_cmp(&a.length).then(a.bar.cmp(&b.bar)));
    let (fit, too_long): (Vec<Piece>, Vec<Piece>) = pieces.into_iter().partition(|p| units(p.length) <= length);
    let mut bins: Vec<(i64, Vec<usize>)> = Vec::new();
    let mut free: BTreeSet<(i64, usize)> = BTreeSet::new();
    for (i, p) in fit.iter().enumerate() {
        let cost = units(p.length) + kerf;
        match free.range((cost, 0)..).next().copied() {
            Some((room, b)) => {
                free.remove(&(room, b));
                if let Some(bin) = bins.get_mut(b) {
                    bin.0 = room - cost;
                    bin.1.push(i);
                    free.insert((room - cost, b));
                }
            }
            None => {
                free.insert((length + kerf - cost, bins.len()));
                bins.push((length + kerf - cost, vec![i]));
            }
        }
    }
    let used: i64 = fit.iter().map(|p| units(p.length)).sum();
    let reusable: Vec<i64> = bins.iter().map(|b| b.0).filter(|room| keep > 0 && *room >= keep).collect();
    let bought = length.saturating_mul(i64::try_from(bins.len()).unwrap_or(i64::MAX));
    let waste = bought - used - reusable.iter().sum::<i64>();
    // Bins with the same pieces are one pattern.
    let mut patterns: Vec<Pattern> = Vec::new();
    let mut seen: HashMap<Vec<(String, String, i64)>, usize> = HashMap::new();
    for (room, list) in &bins {
        let mut held: Vec<&Piece> = list.iter().filter_map(|i| fit.get(*i)).collect();
        held.sort_by(|a, b| {
            b.length.total_cmp(&a.length).then(a.host_mark.cmp(&b.host_mark)).then(a.mark.cmp(&b.mark)).then(a.bar.cmp(&b.bar))
        });
        let signature: Vec<(String, String, i64)> = held.iter().map(|p| (p.host_mark.clone(), p.mark.clone(), units(p.length))).collect();
        match seen.get(&signature).copied().and_then(|at| patterns.get_mut(at)) {
            Some(pattern) => {
                pattern.count += 1;
                for (slot, p) in pattern.pieces.iter_mut().zip(&held) {
                    slot.3.push(p.bar);
                }
            }
            None => {
                seen.insert(signature, patterns.len());
                patterns.push(Pattern {
                    count: 1,
                    pieces: held.iter().map(|p| (p.host_mark.clone(), p.mark.clone(), p.length, vec![p.bar])).collect(),
                    offcut: millimetres(*room),
                });
            }
        }
    }
    Packed {
        stock_bars: bins.len(),
        used: millimetres(used),
        waste: millimetres(waste),
        reusable: reusable.into_iter().map(millimetres).collect(),
        patterns,
        too_long,
    }
}

/// What the schedule templates make: a steel list or a steel summary, as the fields, filters and sort of a schedule of bars.
pub struct Template {
    pub name: String,
    pub fields: Vec<Field>,
    pub filters: Vec<Filter>,
    pub sort: Vec<Sort>,
}

/// The template `which` (`steel_list` or `steel_summary`) of one `host`'s bars and one `level`'s (by name), with the columns `show`
/// says (`count`, `weight` or `both`): every list is grouped, and its name has the host's id and the level's name in it, so a
/// list of one host does not replace the project's.
pub fn template(m: &Model, which: &str, host: Option<Id>, level: Option<&str>, show: &str) -> Template {
    let word = |english: &str| m.language.word(english).to_string();
    let plain = |name: &str| Field { name: name.to_string(), ..Field::default() };
    let total = Field {
        name: "Total Length".into(),
        formula: Some("Count * Length".into()),
        kind: Some(Kind::Length),
        heading: Some(word(msg!("Total Length"))),
        total: true,
    };
    let weight = Field { name: "Weight".into(), total: true, ..Field::default() };
    let (base, mut fields, sort) = if which == "steel_summary" {
        let fields = vec![plain("Level"), plain("Grade"), plain("Diameter"), plain("Count"), total, weight];
        (word(msg!("Steel Summary")), fields, ["Level", "Grade", "Diameter"])
    } else {
        let fields = vec![
            plain("Host Mark"),
            plain("Mark"),
            plain("Diameter"),
            plain("Grade"),
            plain("Shape"),
            plain("Count"),
            plain("Length"),
            total,
            weight,
        ];
        (word(msg!("Steel List")), fields, ["Host", "Mark", ""])
    };
    // By count, the weight goes; by weight, the count (the total length still reads it).
    fields.retain(|f| match show {
        "count" => f.name != "Weight",
        "weight" => f.name != "Count",
        _ => true,
    });
    let mut filters = Vec::new();
    let mut name = base;
    if let Some(h) = host {
        filters.push(Filter { field: "Host".into(), op: Op::Equals, value: json!(h) });
        name = format!("{name} {h}");
    }
    if let Some(l) = level {
        filters.push(Filter { field: "Level".into(), op: Op::Equals, value: json!(l) });
        name = format!("{name} {l}");
    }
    let sort = sort.iter().filter(|s| !s.is_empty()).map(|s| Sort { field: (*s).into(), descending: false }).collect();
    Template { name, fields, filters, sort }
}
