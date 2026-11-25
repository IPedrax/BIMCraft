//! Rebar details: the drawing a detailer issues for a reinforced member, a view of its own that goes on a sheet and prints with it.
//! It holds only what is chosen (the member, a scale, where its sections are cut): the sets of bars, their tags, the shapes and the
//! steel list are worked out from the member and its bars each time it is drawn, so reinforcing the member again changes it with no
//! command, and a file means the same thing to every version. [`crate::sheet`]'s `detail` module draws it.
//!
//! A *set* is bars that are copies of one another moved by one step: the stirrups of a zone, a row of bottom bars, one mesh layer.
//! Where two runs of different step meet, the bar between them goes to the run with the closer spacing, so the closer end zone of a
//! beam keeps its bars and the wider middle starts one spacing in, as `reinforce` makes them.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use crate::model::{Element, Guid, Id, MAX_COORD, MAX_TEXT, Model, Rebar};
use crate::reinforce::{Frame, Kind, Located, along, member_frame};
use crate::steel::{self, Bars, Item, Scope};
use crate::view3d::{P3, cross, dot, sub, unit};

/// The most sections a detail cuts.
pub const MAX_STATIONS: usize = 6;
/// The most details a project holds.
pub const MAX_DETAILS: usize = 20_000;
/// The scales a detail is drawn at, 1 : n.
pub const SCALE: RangeInclusive<f64> = 1.0..=10_000.0;
/// Two bars are copies of one another, and a bar lies in a plane, when no point is further off than this, mm.
const NEAR: f64 = 0.5;

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}

/// A reinforced member's detail (a beam, a column, a footing or a floor): one drawing, its elevation or plan, its sections and its
/// steel list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RebarDetail {
    pub name: String,
    /// The member, by GUID.
    pub host: Guid,
    /// It is drawn at 1 : `scale` unless a viewport says another.
    pub scale: f64,
    /// Where its sections are cut, mm along the member from its start. None: the middle of each zone of stirrups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stations: Vec<f64>,
    /// Whether its steel list is under it.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub list: bool,
}

impl RebarDetail {
    pub fn new(name: String, host: Guid, scale: f64) -> RebarDetail {
        RebarDetail { name, host, scale, stations: Vec::new(), list: true }
    }
}

/// What the project must hold of a detail. Its stations are checked against the member's length when one is asked for, not here,
/// since a member made shorter later must not fail the command that shortened it: a station past its end is left out of the drawing.
pub fn check(m: &Model, who: &str, rd: &RebarDetail) -> Result<(), String> {
    if rd.name.trim().is_empty() {
        return Err(format!("{who}: a view needs a name"));
    }
    if rd.name.chars().count() > MAX_TEXT {
        return Err(format!("{who}: its name is longer than {MAX_TEXT} characters"));
    }
    if !(rd.scale.is_finite() && SCALE.contains(&rd.scale)) {
        return Err(format!("{who}: `scale` is {} to {}, not {}", SCALE.start(), SCALE.end(), rd.scale));
    }
    if rd.stations.len() > MAX_STATIONS {
        return Err(format!("{who}: it cuts {} sections, more than the {MAX_STATIONS} a detail holds", rd.stations.len()));
    }
    if let Some(x) = rd.stations.iter().find(|x| !(x.is_finite() && (0.0..=MAX_COORD).contains(*x))) {
        return Err(format!("{who}: a station of {x} mm isn't a length along a member"));
    }
    match m.elements.get(rd.host) {
        Some(Element::Beam(_) | Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) | Element::Floor(_)) => Ok(()),
        Some(other) => Err(format!("{who}: a {} has no detail; a beam, a column, a footing or a floor has one", other.category())),
        None => Err(format!("{who}: its member isn't an element of the project")),
    }
}

fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}

/// A point to hundredths of a millimetre, which bars are put in order by: float noise must not decide it.
fn at_hundredths(p: P3) -> [i64; 3] {
    [(p[0] * 100.0).round() as i64, (p[1] * 100.0).round() as i64, (p[2] * 100.0).round() as i64]
}

fn start_of(rb: &Rebar) -> P3 {
    rb.path.first().copied().unwrap_or([0.0; 3])
}

/// A bar's point in a member's own axes: how far along it, across it and up from the frame's origin, mm.
pub(crate) fn in_frame(f: &Frame, p: P3) -> P3 {
    [along(f, f.a, p), along(f, f.b, p), along(f, f.c, p)]
}

/// Whether every point of `path` is at one place along the member: a stirrup or a tie, in a plane square to it.
pub(crate) fn square(f: &Frame, path: &[P3]) -> bool {
    let mut us = path.iter().map(|p| along(f, f.a, *p));
    let Some(u0) = us.next() else { return false };
    us.all(|u| (u - u0).abs() <= NEAR)
}

/// A set of bars: copies of one another, one step apart.
#[derive(Clone, Debug, PartialEq)]
pub struct Set {
    /// Its bars, in order along the step.
    pub bars: Vec<Id>,
    pub layer: String,
    pub grade: String,
    pub diameter: f64,
    /// The number of its mark among its member's (`N2`: the zones of one stirrup share it).
    pub mark: usize,
    pub count: usize,
    /// From one bar to the next; none for a set of one.
    pub step: Option<P3>,
    /// The length of `step`, mm.
    pub spacing: Option<f64>,
    /// One bar's cut length, mm.
    pub length: f64,
    /// Where its first bar starts.
    pub at: P3,
}

/// The step that moves bar `a` onto bar `b`, if `b` is a copy of `a`: the same number of points, each as far from its own as the
/// first is, to within [`NEAR`].
fn step_between(a: &Rebar, b: &Rebar) -> Option<P3> {
    if a.path.len() != b.path.len() || a.path.is_empty() {
        return None;
    }
    let d = sub(start_of(b), start_of(a));
    a.path.iter().zip(&b.path).all(|(p, q)| norm(sub(sub(*q, *p), d)) <= NEAR).then_some(d)
}

fn make(g: &[&Item<'_>], idx: &[usize], step: Option<P3>) -> Option<Set> {
    let first = g.get(*idx.first()?)?;
    let step = step.filter(|_| idx.len() > 1);
    Some(Set {
        bars: idx.iter().filter_map(|i| g.get(*i)).map(|it| it.id).collect(),
        layer: first.bar.layer.clone(),
        grade: first.bar.grade.clone(),
        diameter: first.bar.diameter,
        mark: first.number,
        count: idx.len(),
        step,
        spacing: step.map(norm),
        length: first.length,
        at: start_of(first.bar),
    })
}

/// The sets of one group of bars (of one layer, grade, diameter and shape), already in order.
fn runs(g: &[&Item<'_>]) -> Vec<Set> {
    // The step from each bar to the next, none where the next is not its copy.
    let steps: Vec<Option<P3>> = g
        .windows(2)
        .map(|w| match w {
            [a, b] => step_between(a.bar, b.bar),
            _ => None,
        })
        .collect();
    // The maximal runs of equal steps: the first step, the last, and the step.
    let mut found: Vec<(usize, usize, P3)> = Vec::new();
    let mut i = 0;
    while let Some(step) = steps.get(i).copied() {
        let Some(d) = step else {
            i += 1;
            continue;
        };
        let mut j = i;
        while steps.get(j + 1).copied().flatten().is_some_and(|e| norm(sub(e, d)) <= NEAR) {
            j += 1;
        }
        found.push((i, j, d));
        i = j + 1;
    }
    // A run of steps `a` to `z` is the bars `a` to `z + 1`; a bar between two runs goes to the one with the closer spacing.
    let mut owner: Vec<Option<usize>> = vec![None; g.len()];
    for (r, (a, z, d)) in found.iter().enumerate() {
        for slot in owner.iter_mut().take(z + 2).skip(*a) {
            let closer = |held: usize| found.get(held).is_none_or(|(_, _, e)| norm(*d) < norm(*e));
            if slot.is_none_or(closer) {
                *slot = Some(r);
            }
        }
    }
    let mut out = Vec::new();
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); found.len()];
    for (b, o) in owner.iter().enumerate() {
        match o {
            Some(r) => {
                if let Some(list) = members.get_mut(*r) {
                    list.push(b);
                }
            }
            None => out.extend(make(g, &[b], None)),
        }
    }
    for ((_, _, d), list) in found.iter().zip(&members) {
        out.extend(make(g, list, Some(*d)));
    }
    out
}

/// The sets of the bars of `host`, by mark, then by where they start. The counts of the sets add up to the member's bars.
pub fn sets(m: &Model, bars: &Bars, host: Id) -> Vec<Set> {
    let scope = Scope { level: None, only: Some(BTreeSet::from([host])) };
    let items = steel::items(m, bars, &scope);
    // Layer, grade, diameter in hundredths and shape.
    type Key = (String, String, i64, Vec<i64>);
    let mut groups: BTreeMap<Key, Vec<&Item<'_>>> = BTreeMap::new();
    for it in &items {
        let key = (it.bar.layer.clone(), it.bar.grade.clone(), (it.bar.diameter * 100.0).round() as i64, steel::shape(&it.bar.path).key());
        groups.entry(key).or_default().push(it);
    }
    let mut out = Vec::new();
    for mut g in groups.into_values() {
        g.sort_by(|a, b| at_hundredths(start_of(a.bar)).cmp(&at_hundredths(start_of(b.bar))).then(a.id.cmp(&b.id)));
        out.extend(runs(&g));
    }
    out.sort_by(|a, b| a.mark.cmp(&b.mark).then_with(|| at_hundredths(a.at).cmp(&at_hundredths(b.at))));
    out
}

/// What a set's tag says: its mark, how many, its diameter, its spacing where its bars spread across the picture it is drawn in
/// (`spread`), and one bar's cut length; each in the project's unit but the diameter, which is in millimetres: `N2 7 ø5 c/100 C=1180`.
pub fn tag_text(m: &Model, set: &Set, spread: bool) -> String {
    tag_count(m, set, set.count, spread)
}

/// [`tag_text`] saying `count` bars: a tag that stands for several sets of one mark (a column's bars are rows of them).
pub(crate) fn tag_count(m: &Model, set: &Set, count: usize, spread: bool) -> String {
    let mut t = steel::mark_text(set.mark);
    if count > 1 {
        t.push_str(&format!(" {count}"));
    }
    t.push_str(&format!(" ø{}", set.diameter));
    if spread && let Some(s) = set.spacing {
        t.push_str(&format!(" c/{}", m.length(s)));
    }
    t.push_str(&format!(" C={}", m.length(set.length)));
    t
}

/// How far along the member each point of `set`'s bars is (only each bar's first, with `first_only`), mm.
fn along_of(m: &Model, set: &Set, f: &Frame, first_only: bool) -> Vec<f64> {
    set.bars
        .iter()
        .filter_map(|id| match m.elements.get(*id) {
            Some(Element::Rebar(rb)) => Some(rb),
            _ => None,
        })
        .flat_map(|rb| rb.path.iter().take(if first_only { 1 } else { usize::MAX }).map(|p| along(f, f.a, *p)))
        .collect()
}

fn lowest_and_highest(xs: &[f64]) -> Option<(f64, f64)> {
    Some((xs.iter().copied().reduce(f64::min)?, xs.iter().copied().reduce(f64::max)?))
}

/// A station in whole tenths of a millimetre, unless there is one within a millimetre of it already.
fn put(at: &mut Vec<f64>, x: f64) {
    let x = (x * 10.0).round() / 10.0;
    if !at.iter().any(|y| (y - x).abs() < 1.0) {
        at.push(x);
    }
}

/// Where a member's sections are cut when none is chosen. A beam's: the middle of each zone of stirrups, or the middle of the beam
/// where it has none. A column's: the foot, the middle and the head of its stirrups (of its bars, where it has none). A footing and
/// a floor have none. Whole tenths of a millimetre, in order, no more than [`MAX_STATIONS`].
pub(crate) fn default_stations(m: &Model, sets: &[Set], b: &Located) -> Vec<f64> {
    let mut at: Vec<f64> = Vec::new();
    match b.kind {
        Kind::Footing | Kind::Floor => return Vec::new(),
        Kind::Beam => {
            for s in sets.iter().filter(|s| s.layer == "stirrup") {
                if let Some((lo, hi)) = lowest_and_highest(&along_of(m, s, &b.frame, true)) {
                    put(&mut at, (lo + hi) / 2.0);
                }
            }
        }
        Kind::Column => {
            let stirrups: Vec<f64> = sets.iter().filter(|s| s.layer == "stirrup").flat_map(|s| along_of(m, s, &b.frame, true)).collect();
            let all: Vec<f64> =
                if stirrups.is_empty() { sets.iter().flat_map(|s| along_of(m, s, &b.frame, false)).collect() } else { stirrups };
            if let Some((lo, hi)) = lowest_and_highest(&all) {
                for x in [lo, (lo + hi) / 2.0, hi] {
                    put(&mut at, x);
                }
            }
        }
    }
    if at.is_empty() {
        put(&mut at, b.length / 2.0);
    }
    at.sort_by(f64::total_cmp);
    at.truncate(MAX_STATIONS);
    at
}

/// Whether `set`'s bars spread along the member (the stirrups of a zone, not a row of bottom bars lying behind one another), so its
/// tag in an elevation says its spacing.
pub(crate) fn spreads(set: &Set, f: &Frame) -> bool {
    set.step.is_some_and(|s| dot(s, f.a).abs() >= 1.0)
}

/// Whether `set`'s tag says its spacing in the picture its member's detail draws it in: a beam's or a column's elevation, a footing's
/// or a floor's plan.
pub(crate) fn spread_in(set: &Set, b: &Located) -> bool {
    match b.kind {
        Kind::Beam | Kind::Column => spreads(set, &b.frame),
        Kind::Footing | Kind::Floor => spreads_over(set, [b.frame.a, b.frame.b]),
    }
}

/// Whether `set`'s bars spread across a picture of the plane of `axes` (a plan's), so its tag there says its spacing.
pub(crate) fn spreads_over(set: &Set, axes: [P3; 2]) -> bool {
    set.step.is_some_and(|s| dot(s, axes[0]).hypot(dot(s, axes[1])) >= 1.0)
}

/// A bar unfolded into its plane, for the steel list to sketch: its points as (along the first leg, across it), mm.
pub struct Sketch {
    pub points: Vec<[f64; 2]>,
    /// Its points leave one plane by more than [`NEAR`], so the sketch is the bar projected.
    pub spatial: bool,
}

/// The sketch of the bar along `path`: the first leg along x, the plane from the first two legs that are not parallel.
pub fn sketch(path: &[P3]) -> Sketch {
    let mut pts: Vec<P3> = Vec::with_capacity(path.len());
    for p in path {
        if pts.last().is_none_or(|q| norm(sub(*p, *q)) > 1e-6) {
            pts.push(*p);
        }
    }
    let (Some(&p0), Some(&p1)) = (pts.first(), pts.get(1)) else {
        return Sketch { points: pts.iter().map(|_| [0.0, 0.0]).collect(), spatial: false };
    };
    let e1 = unit(sub(p1, p0));
    let normal = pts.windows(2).skip(1).find_map(|w| match w {
        [a, b] => {
            let d = sub(*b, *a);
            let c = cross(e1, d);
            (norm(c) > 1e-3 * norm(d)).then(|| unit(c))
        }
        _ => None,
    });
    let e2 = normal.map_or([0.0; 3], |n| cross(n, e1));
    let spatial = normal.is_some_and(|n| pts.iter().any(|p| dot(sub(*p, p0), n).abs() > NEAR));
    let points = pts
        .iter()
        .map(|p| {
            let d = sub(*p, p0);
            [dot(d, e1), dot(d, e2)]
        })
        .collect();
    Sketch { points, spatial }
}

/// The member's frame, when `rd` has one to draw: a member with a type (a floor, an outline).
pub(crate) fn host_frame(m: &Model, rd: &RebarDetail) -> Option<Located> {
    m.elements.get(rd.host).and_then(|e| member_frame(m, e))
}
