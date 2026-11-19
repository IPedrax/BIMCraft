//! Quantities: what a beam, a column, a footing, a strip footing and a floor come to in concrete, in formwork and in steel.
//! Nothing here is stored. A volume, a formwork area and a rebar weight are worked out when asked, so a changed member changes
//! them and a file means the same thing to every version. The rules are written once, here, and the schedule's fields, the
//! `quantities` command and the IFC all read them through [`take`]. They are this project's conventions, not any program's, and
//! each says what it leaves out ([the area](../../../docs/areas/quantities.md)).

use std::f64::consts::PI;

use bimcraft_i18n::msg;
use serde_json::{Value, json};

use crate::geom::{self, P2};
use crate::model::{Element, Floor, Id, Model, Profile, RunType, TOL};
use crate::rooms::Rooms;
use crate::runs::Geom;
use crate::schedule::{Field, Filter, Op, Sort};
use crate::steel::{Bars, Template};
use crate::walls::Plane;

/// The categories the quantities count, as a schedule or a command names them.
pub const CATEGORIES: [&str; 6] = ["beam", "column", "structuralcolumn", "footing", "stripfooting", "floor"];

/// What one member takes off: its concrete in m³, the area of formwork it needs in m² and the weight of the bars it hosts in kg.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Take {
    pub volume_m3: f64,
    pub formwork_m2: f64,
    pub rebar_kg: f64,
}

impl Take {
    /// Steel per cubic metre of concrete, kg/m³; none where there is no concrete.
    pub fn rebar_ratio(&self) -> Option<f64> {
        (self.volume_m3 > 0.0).then_some(self.rebar_kg / self.volume_m3)
    }
}

/// What a member's rules look at of the rest of the model, found once for one look at many members: the concrete columns, the
/// concrete beams and the structural floors, by where they stand and how high they reach.
pub struct Index {
    columns: Vec<Member>,
    beams: Vec<Member>,
    floors: Vec<Slab>,
}

/// A column or a beam as the rest meet it: its outline in plan, the box round that and its lowest and highest points.
struct Member {
    ring: Vec<P2>,
    reach: [f64; 4],
    span: (f64, f64),
}

/// A structural floor as a beam's sides meet it: its pieces in plan (less its openings), each between its underside's plane and its
/// top's, the box round them all and the lowest and highest the floor reaches.
struct Slab {
    pieces: Vec<(Vec<P2>, Plane, Plane)>,
    reach: [f64; 4],
    span: (f64, f64),
}

/// The box (west, south, east, north) round some points.
fn reach(points: &[P2]) -> [f64; 4] {
    points.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |[x0, y0, x1, y1], q| {
        [x0.min(q.0), y0.min(q.1), x1.max(q.0), y1.max(q.1)]
    })
}

fn meets(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

impl Index {
    pub fn new(m: &Model) -> Index {
        let (mut columns, mut beams, mut floors) = (Vec::new(), Vec::new(), Vec::new());
        for (_, e) in m.elements.iter() {
            match e {
                // A steel column's outline is not convex, and a beam is not cut by it as it is by concrete.
                Element::Column(_) | Element::StructuralColumn(_) if m.column_type(e).is_some_and(|t| t.profile.is_none()) => {
                    let ring = m.column_outline(e);
                    if ring.len() >= 3 {
                        columns.push(Member { reach: reach(&ring), span: m.column_span(e), ring });
                    }
                }
                Element::Beam(_) if m.run_type(e).is_some_and(|t| !is_steel(m, t)) => {
                    if let Some(g) = m.run_geom(e) {
                        let ring = g.footprint();
                        beams.push(Member { reach: reach(&ring), span: g.z_span(), ring });
                    }
                }
                Element::Floor(f) if f.structural => {
                    let pieces = m.floor_prisms(f);
                    let points: Vec<P2> = pieces.iter().flat_map(|(ring, ..)| ring.iter().copied()).collect();
                    let span = pieces
                        .iter()
                        .flat_map(|(ring, under, over)| ring.iter().map(move |q| (under.at(*q), over.at(*q))))
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), (u, o)| (low.min(u), high.max(o)));
                    if !points.is_empty() {
                        floors.push(Slab { reach: reach(&points), span, pieces });
                    }
                }
                _ => {}
            }
        }
        Index { columns, beams, floors }
    }
}

/// Whether a run's type is a steel section, which has a weight and no concrete: it says metal, or says nothing and has a section
/// profile, which is what `element_json` weighs a beam by.
// shortcut: a type with a profile and no material is steel until materials carry a density, as `element_json` has it.
pub fn is_steel(m: &Model, t: &RunType) -> bool {
    t.material == m.language.word("Metal") || (t.material.is_empty() && t.profile.is_some())
}

/// A section's area and perimeter (mm² and mm): a profile's own, a circle, or a rectangle.
fn section(width: f64, depth: f64, round: bool, profile: Option<&Profile>) -> (f64, f64) {
    if let Some(p) = profile {
        (p.area(width, depth), p.perimeter(width, depth))
    } else if round {
        (PI * width * width / 4.0, PI * width)
    } else {
        (width * depth, 2.0 * (width + depth))
    }
}

/// What member `e` (element `id`) takes off, or none for what is not one (or has no type, or is a steel section, which `Weight`
/// covers). Its rebar weight is that of the bars that name it as their host.
pub fn take(m: &Model, ix: &Index, bars: &Bars, id: Id, e: &Element) -> Option<Take> {
    let mut t = match e {
        Element::Beam(_) | Element::StripFooting(_) => run_take(m, ix, e),
        Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) => column_take(m, ix, e),
        Element::Floor(f) => Some(floor_take(m, ix, f)),
        _ => None,
    }?;
    t.rebar_kg = bars.weight(m, m.guid(id));
    (t.volume_m3.is_finite() && t.formwork_m2.is_finite() && t.rebar_kg.is_finite()).then_some(t)
}

/// A beam's or a strip footing's section by its length. A rectangular beam is formed on its bottom and its two sides, a strip
/// footing on its two sides (it is cast on the soil, so no soffit); any other section by its whole perimeter. A beam is counted
/// from face to face of the concrete columns it meets (its axis runs to their centres), and a rectangular one loses, on each side a
/// structural floor covers, that floor's thickness of forms and half its width of concrete by that thickness: the floor holds
/// that concrete and rests on that side. A strip footing is counted along its whole axis.
fn run_take(m: &Model, ix: &Index, e: &Element) -> Option<Take> {
    let (t, g) = (m.run_type(e)?, m.run_geom(e)?);
    if is_steel(m, t) {
        return None;
    }
    let (area, perimeter) = section(g.width, g.depth, g.round, t.profile.as_ref());
    let rect = !g.round && t.profile.is_none();
    let counted = matches!(e, Element::Beam(_)) && !g.vertical();
    let parts = if counted { clear_parts(&g, ix) } else { Vec::new() };
    let slope = if g.vertical() { 1.0 } else { g.length() / g.plan_length() };
    let length = if counted { parts.iter().map(|(p, q)| p.dist(*q)).sum::<f64>() * slope } else { g.length() };
    let formed = match (e, rect) {
        (Element::Beam(_), true) => g.width + 2.0 * g.depth,
        (Element::StripFooting(_), true) => 2.0 * g.depth,
        _ => perimeter,
    };
    let (volume, forms) = (area * length, formed * length);
    // shortcut: a round or profiled beam loses nothing to a floor over it, and two beams crossing are counted twice at the crossing.
    let (less_volume, less_forms) = if counted && rect { covered(&g, &parts, slope, ix) } else { (0.0, 0.0) };
    Some(Take { volume_m3: (volume - less_volume).max(0.0) / 1e9, formwork_m2: (forms - less_forms).max(0.0) / 1e6, ..Take::default() })
}

/// The stretches of a beam's axis, in plan, that lie between the concrete columns whose height meets its own.
// shortcut: a beam leaning to or from a column is cut at the column's outline in plan, not along its own slope.
fn clear_parts(g: &Geom, ix: &Index) -> Vec<(P2, P2)> {
    let (a, b) = (P2(g.a[0], g.a[1]), P2(g.b[0], g.b[1]));
    let (low, high) = g.z_span();
    let near = reach(&[a, b]);
    let mut parts = vec![vec![a, b]];
    for c in ix.columns.iter().filter(|c| meets(&c.reach, &near) && c.span.0 < high - TOL && c.span.1 > low + TOL) {
        parts = parts.iter().flat_map(|p| geom::line_outside_convex(p, &c.ring)).collect();
    }
    parts.iter().flat_map(|p| p.windows(2).map(|w| (w[0], w[1]))).collect()
}

/// What the structural floors over a rectangular beam's sides take off it, as (concrete mm³, forms mm²). Each side is the axis
/// moved out by half the width. Wherever a side runs inside a floor, the floor covers it as high as the beam and the floor
/// overlap there (their planes, so a slope is right), and that height times the stretch (true length) is forms the floor does
/// away with, and half the width by it is concrete the floor holds.
fn covered(g: &Geom, parts: &[(P2, P2)], slope: f64, ix: &Index) -> (f64, f64) {
    let (_, bottom, top) = g.prism();
    let (low, high) = g.z_span();
    let half = g.width / 2.0;
    let (mut volume, mut forms) = (0.0, 0.0);
    for slab in ix.floors.iter().filter(|s| s.span.0 < high - TOL && s.span.1 > low + TOL) {
        for &(p, q) in parts.iter().filter(|(p, q)| p.dist(*q) > TOL) {
            let out = (q - p).unit().perp() * half;
            for side in [out, out * -1.0] {
                let (a, b) = (p + side, q + side);
                if !meets(&slab.reach, &reach(&[a, b])) {
                    continue;
                }
                for (ring, under, over) in &slab.pieces {
                    for (t0, t1) in inside(a, b, ring) {
                        let mid = a + (b - a) * ((t0 + t1) / 2.0);
                        let height = (top.at(mid).min(over.at(mid)) - bottom.at(mid).max(under.at(mid))).max(0.0);
                        let stretch = (t1 - t0) * a.dist(b) * slope;
                        forms += height * stretch;
                        volume += half * height * stretch;
                    }
                }
            }
        }
    }
    (volume, forms)
}

/// The stretches of a→b, as fractions of its length, that lie inside `ring`, in order: it is cut where it crosses the ring's edges
/// and each piece kept whose middle is inside.
fn inside(a: P2, b: P2, ring: &[P2]) -> Vec<(f64, f64)> {
    let d = b - a;
    let mut cuts = vec![0.0, 1.0];
    for (c, e) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        let edge = *e - *c;
        let den = d.cross(edge);
        if den.abs() < 1e-12 {
            continue;
        }
        let (t, u) = ((*c - a).cross(edge) / den, (*c - a).cross(d) / den);
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            cuts.push(t);
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2).filter(|w| w[1] - w[0] > 1e-12 && geom::contains(ring, a + d * ((w[0] + w[1]) / 2.0))).map(|w| (w[0], w[1])).collect()
}

/// A column's or a footing's section by its height, formed on its sides only (a footing is cast on the soil or on a blinding). A
/// column's height is less what the structural floors over its centre take of it: the slab is one piece of concrete across the
/// column, and rests on its head, so the column's forms stop at the slab's underside. A beam meeting its head is not taken off.
// shortcut: the beams that meet a column's head are not taken off its forms, which leaves a few percent over.
fn column_take(m: &Model, ix: &Index, e: &Element) -> Option<Take> {
    let t = m.column_type(e)?;
    if t.profile.is_some() {
        return None;
    }
    let (bottom, top) = m.column_span(e);
    let held = match e {
        Element::Footing(_) => 0.0,
        _ => e.column().map_or(0.0, |col| held_by_floors(ix, col.at, bottom, top)),
    };
    let height = (top - bottom - held).max(0.0);
    let (area, perimeter) = section(t.width, t.depth, t.round, None);
    Some(Take { volume_m3: area * height / 1e9, formwork_m2: perimeter * height / 1e6, ..Take::default() })
}

/// A floor's concrete as it is (`Model::floor_volume`: the slab whole and continuous, less its openings and the shafts through
/// it), and, for a structural floor, its forms: the soffit and the free edges. A floor that is not structure (a finish) has no forms.
fn floor_take(m: &Model, ix: &Index, f: &Floor) -> Take {
    let volume_m3 = m.floor_volume(f) / 1e9;
    if !f.structural {
        return Take { volume_m3, ..Take::default() };
    }
    let prisms = m.floor_prisms(f);
    // The members that carry it reach its underside from below: they top out at or above its lowest underside and begin below its highest.
    let (low, high) = prisms
        .iter()
        .flat_map(|(ring, under, _)| ring.iter().map(move |q| under.at(*q)))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), z| (l.min(z), h.max(z)));
    let carrying: Vec<&Member> = ix.columns.iter().chain(&ix.beams).filter(|s| s.span.1 >= low - TOL && s.span.0 < high - TOL).collect();
    let formwork_m2 = (soffit(m, &prisms, &carrying) + edges(m, f, &carrying)) / 1e6;
    Take { volume_m3, formwork_m2, ..Take::default() }
}

/// The true area of a floor's underside (mm²): each piece of it less the shafts through it and less where a column or a beam
/// that carries it lies, which is formed by that member.
fn soffit(m: &Model, prisms: &[(Vec<P2>, Plane, Plane)], carrying: &[&Member]) -> f64 {
    let mut total = 0.0;
    for (ring, under, over) in prisms {
        let rise = ring.iter().map(|q| over.at(*q) - under.at(*q)).fold(0.0, f64::max);
        let mut pieces = m.less_shafts(&under.lift(ring), rise).0;
        for member in carrying {
            pieces = pieces
                .into_iter()
                .flat_map(|p| if meets(&reach(&p), &member.reach) { geom::subtract_convex(&p, &member.ring) } else { vec![p] })
                .collect();
        }
        let plan: f64 = pieces.iter().map(|p| geom::area(p).abs()).sum();
        total += plan * (1.0 + under.g.dot(under.g)).sqrt();
    }
    total
}

/// The area of a floor's free edges (mm²): its outline's and its openings' lengths times its thickness, less what lies inside a
/// member that carries it, which forms that edge.
// shortcut: the edges of a shaft through the floor are not formed, and an edge is measured in plan, at the thickness.
fn edges(m: &Model, f: &Floor, carrying: &[&Member]) -> f64 {
    let free = |a: P2, b: P2| -> f64 {
        let near = reach(&[a, b]);
        let mut parts = vec![vec![a, b]];
        for member in carrying.iter().filter(|s| meets(&s.reach, &near)) {
            parts = parts.iter().flat_map(|p| geom::line_outside_convex(p, &member.ring)).collect();
        }
        parts.iter().map(|p| p.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>()).sum()
    };
    let length: f64 = std::iter::once(&f.boundary)
        .chain(&f.openings)
        .map(|ring| ring.iter().zip(ring.iter().cycle().skip(1)).map(|(a, b)| free(*a, *b)).sum::<f64>())
        .sum();
    length * m.floor_depth(f)
}

/// How much of the height from `bottom` to `top` at `at` the structural floors over that point hold: where each lies between its
/// underside and its top.
fn held_by_floors(ix: &Index, at: P2, bottom: f64, top: f64) -> f64 {
    let near = [at.0, at.1, at.0, at.1];
    ix.floors
        .iter()
        .filter(|s| meets(&s.reach, &near))
        .flat_map(|s| &s.pieces)
        .filter(|(ring, ..)| geom::contains(ring, at))
        .map(|(_, under, over)| (top.min(over.at(at)) - bottom.max(under.at(at))).max(0.0))
        .sum()
}

/// What `element_json` adds for `e`: its `volume_m3` to a thousandth of a cubic metre, its `formwork_m2` to a hundredth of a square
/// metre, its `rebar_kg` to a hundredth of a kilogram and its `rebar_ratio` (kg/m³, null over no concrete), for the members that have them. The index is found the first time one is asked for, so a look at elements that
/// are none of these never builds it.
pub fn fields(m: &Model, rooms: &Rooms, id: Id, e: &Element) -> Vec<(&'static str, Value)> {
    let member = matches!(
        e,
        Element::Beam(_)
            | Element::StripFooting(_)
            | Element::Column(_)
            | Element::StructuralColumn(_)
            | Element::Footing(_)
            | Element::Floor(_)
    );
    if !member {
        return Vec::new();
    }
    let Some(t) = take(m, rooms.quantities(), rooms.takeoff(), id, e) else { return Vec::new() };
    let mut out = vec![("volume_m3", json!((t.volume_m3 * 1e3).round() / 1e3))];
    // A floor that is not structure has no forms, which is not the same as none to be formed.
    if !matches!(e, Element::Floor(f) if !f.structural) {
        out.push(("formwork_m2", json!((t.formwork_m2 * 1e2).round() / 1e2)));
        out.push(("rebar_kg", json!((t.rebar_kg * 1e2).round() / 1e2)));
        out.push(("rebar_ratio", t.rebar_ratio().map_or(Value::Null, |r| json!((r * 10.0).round() / 10.0))));
    }
    out
}

/// What the Materials Summary template makes: a schedule of the six categories, by level and category, with the count, the concrete,
/// the forms and the steel of each group, and their totals. `level` (a level's name) keeps one level's members and puts its name in
/// the schedule's, so it does not replace the project's. A schedule of several categories has no project parameter among its fields,
/// so it cannot split by one: `quantities` does.
pub fn template(m: &Model, level: Option<&str>) -> Template {
    let plain = |name: &str| Field { name: name.to_string(), ..Field::default() };
    let total = |name: &str| Field { name: name.to_string(), total: true, ..Field::default() };
    let fields = vec![plain("Level"), plain("Category"), plain("Count"), total("Volume"), total("Formwork Area"), total("Rebar Weight")];
    let sort: Vec<Sort> = ["Level", "Category"].iter().map(|f| Sort { field: (*f).into(), descending: false }).collect();
    let mut name = m.language.word(msg!("Materials Summary")).to_string();
    let mut filters = Vec::new();
    if let Some(l) = level {
        filters.push(Filter { field: "Level".into(), op: Op::Equals, value: json!(l) });
        name = format!("{name} {l}");
    }
    Template { name, fields, filters, sort }
}
