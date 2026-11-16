//! A framed floor's joists, rims, trimmers, headers and blocking, worked out from the floor, its openings and the shafts
//! through it, the beams and walls under it, the walls standing on it and its framing type whenever they are asked for, and
//! never stored, as a framed wall's studs are ([`super::wall`]).
//!
//! The members stand in the floor type's structure layer (the thickest, where it has several): as deep as that layer is
//! thick, so a 2x8 floor is one with a 184 mm structure layer, from its underside to its top; the layers over it (a subfloor,
//! a finish) stay drawn as layers. The frame: `s` runs the way the joists run ([`angle`]), `across` to its left, both from
//! the floor's outline's first corner, so each member is a plumb-sided box from `s[0]` to `s[1]` and `across[0]` to
//! `across[1]` (a rim along a skewed edge in a frame of its own, [`Member::frame`]).
//!
//! The rules, in that frame:
//! - A rim joist lies along each edge of the outline the joists end on (each not parallel to them), one thickness thick inside
//!   it and the whole edge long, less the holes it crosses; where two rims meet at a corner, the one of the lower edge index
//!   runs through and the other stops where its nearer corner meets the first's inner face; at a sharp corner each stops
//!   where its inner side meets the outline; at an inward one the one that runs through runs on to the corner of their inner
//!   faces and the other starts clear of it (a wedge left between them where the corner isn't square, with a warning). A rim
//!   that would overlap another, or doesn't fit on the floor, is left out with a warning. An edge parallel to the joists has
//!   an edge joist flush with it (where it lies farthest in), running between the rims, as has one within a degree of them
//!   that takes no rim (with a warning past a millimetre off); edges in line share one; a floor no joist fits says so. Every
//!   joist ends square where it first meets a rim or a hole: on a skewed edge its nearer corner touches the rim's inner face.
//! - Grid joists are centred at stations `spacing` apart, counted from the layout's origin across the outline: `start` its
//!   least `across`, `end` its most, `centre` the middle. One that would overlap another joist (an edge joist, a trimmer, a
//!   pair under a wall) is left out. Each runs in pieces where the outline, less what it doesn't frame round, lets it.
//! - A beam or a straight wall under the floor whose top lies within [`BEARING`] of the joists' underside bears them. Where
//!   one crosses a joist away from its ends, the joist is in pieces that butt on its centre line, or (`lap`) each runs `lap`
//!   past it (no more than halfway to the next support, with a warning) and every other piece lies a thickness over, beside
//!   the one before; an edge joist, a trimmer and a pair under a wall always butt.
//! - Each hole (an opening, a shaft through the floor) is framed by the box round it in the frame: `trimmers` plies of
//!   trimmer along each side of it, running the joists' whole length, `trimmers` plies of header across each end between
//!   them, and the joists it cuts are tails from what they start on to a header. A hole whose box doesn't fit inside the rims
//!   and edge joists, runs into another hole, or whose trimmers would run into another hole's, isn't trimmed: its joists end
//!   at it, with a warning.
//! - A straight wall standing on the floor (its level's, its base at the floor's top) along the joists (within 1°) gets two
//!   joists side by side under its centre line, the whole length of the floor (`under/<wall GUID>/0` and `/1`); one whose
//!   centre line is over an edge joist stands on that, and one over an opening gets none, with a warning.
//! - Rows of solid blocking (`bridging`) divide each span between what bears the joists evenly, none longer than it, a block
//!   between each two neighbouring joists, none across a hole nor between joists less than a millimetre apart.
//!
//! Nothing is left out without a word: what can't be framed as the type says is in [`Framing::warnings`], naming what and
//! why.

use super::{EPS, Framing, Grain, Layout, MAX_STATIONS, Member, Role, spliced};
use crate::derived::Group;
use crate::geom::{self, P2};
use crate::model::{Element, Floor, Function, Guid, Id, Model, TOL};
use crate::walls::{self, Axis};

/// How near the joists' underside the top of a beam or a wall must lie to bear them, mm.
pub const BEARING: f64 = 10.0;
/// How near to the joists' way a wall standing on the floor must run for a pair of joists under it, degrees.
const PARALLEL: f64 = 1.0;
/// The most blocks a floor's rows of blocking may make.
pub(super) const MAX_BLOCKS: f64 = 65536.0;
/// How far inside a joist's sides its ends are found, so a side along an edge isn't taken as on it.
const NUDGE: f64 = 1e-4;

/// The thickest structure layer of floor `f`'s type (the first of the thickest): its index, and its underside and top,
/// absolute.
pub(crate) fn structure(m: &Model, f: &Floor) -> Option<(usize, [f64; 2])> {
    let layers = m.type_layers("floor", &f.type_name);
    let (k, _) = layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.function == Function::Structure)
        .max_by(|(i, a), (j, b)| a.thickness.total_cmp(&b.thickness).then(j.cmp(i)))?;
    let z = heights(m, f);
    Some((k, [*z.get(k + 1)?, *z.get(k)?]))
}

/// Where each of flat floor `f`'s layers starts, from its top down, absolute, and its underside last: each its share of the
/// floor's depth, as its 3D solid lays them. None without layers.
pub(crate) fn heights(m: &Model, f: &Floor) -> Vec<f64> {
    let layers = m.type_layers("floor", &f.type_name);
    let total: f64 = layers.iter().map(|l| l.thickness).sum();
    if !total.is_finite() || total <= 0.0 {
        return Vec::new();
    }
    let (top, depth) = (m.floor_top(f).z, m.floor_depth(f));
    let mut above = 0.0;
    std::iter::once(top)
        .chain(layers.iter().map(|l| {
            above += l.thickness;
            top - depth * above / total
        }))
        .collect()
}

/// Floor `f`'s structure layer, where it is framed and can be: its members stand in it, and its own solid there is drawn
/// only where a view hides Floors > Framing.
pub(crate) fn framed(m: &Model, f: &Floor) -> Option<(usize, [f64; 2])> {
    f.framing.as_ref()?;
    check(m, f).ok()?;
    structure(m, f)
}

/// The material framed floor `f`'s framing type names for its members, if it names one: else they're its structure layer's.
pub(crate) fn material<'a>(m: &'a Model, f: &Floor) -> Option<&'a str> {
    m.types.framing.get(f.framing.as_ref()?)?.material.as_deref()
}

/// The way floor `f`'s joists run, degrees from +x: its own `framing_angle`, else square to its outline's longest edge (the
/// first of the longest), turned to between 0 and 180.
pub fn angle(f: &Floor) -> f64 {
    if let Some(a) = f.framing_angle {
        return a;
    }
    let mut longest = (0.0, P2(1.0, 0.0));
    for (a, b) in geom::ring_edges(&f.boundary) {
        if a.dist(b) > longest.0 {
            longest = (a.dist(b), b - a);
        }
    }
    (longest.1.1.atan2(longest.1.0).to_degrees() + 90.0).rem_euclid(180.0)
}

/// Floor `f`'s frame: its outline's first corner, and the way its joists run.
fn frame(f: &Floor) -> (P2, P2) {
    let t = angle(f).to_radians();
    (f.boundary.first().copied().unwrap_or_default(), P2(t.cos(), t.sin()))
}

/// Ring `r` counter-clockwise.
pub(super) fn ccw(r: Vec<P2>) -> Vec<P2> {
    if geom::area(&r) < 0.0 { r.into_iter().rev().collect() } else { r }
}

/// What a framed floor must be, which `Model::check` asks of each: of a known framing type, flat and unshaped, of a type with
/// a structure layer, and no wider across its joists than [`MAX_STATIONS`] spacings, its rows of blocking no more than
/// [`MAX_BLOCKS`] blocks. Nothing an opening, a support or a wall does is refused: what can't be framed is warned of.
pub(crate) fn check(m: &Model, f: &Floor) -> Result<(), String> {
    let Some(name) = &f.framing else { return Ok(()) };
    let t = m.types.framing.get(name).ok_or_else(|| {
        let names: Vec<&str> = m.types.framing.keys().map(String::as_str).collect();
        format!("framing type `{name}` doesn't exist (there are: {}); make it with `type` {{category: framing, name}}", names.join(", "))
    })?;
    if let Some(a) = f.framing_angle.filter(|a| !(a.is_finite() && a.abs() <= 3600.0)) {
        return Err(format!("its joists' angle {a}° isn't a number from -3600 to 3600"));
    }
    if f.slope != 0.0 {
        return Err(format!(
            "only a flat floor is framed, and this one slopes {}°: make it flat (`modify` {{slope: 0}}) or `unframe` it; sloped \
             members come with roofs",
            f.slope
        ));
    }
    if !f.shape.is_empty() {
        return Err("only a flat floor is framed, and this one's shape is edited (points or split lines on its top): reset its \
                    shape or `unframe` it"
            .into());
    }
    structure(m, f).ok_or_else(|| {
        format!(
            "its type `{}` has no structure layer to frame: give the type a layer of function structure (`type` {{category: \
             floor, layers}}) or `unframe` it",
            f.type_name
        )
    })?;
    let (origin, x) = frame(f);
    let local: Vec<P2> = f.boundary.iter().map(|q| P2((*q - origin).dot(x), (*q - origin).dot(x.perp()))).collect();
    let extent = |k: fn(&P2) -> f64| {
        let (lo, hi) = local.iter().map(k).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
        hi - lo
    };
    let (long, wide) = (extent(|p| p.0), extent(|p| p.1));
    if wide / t.spacing > MAX_STATIONS {
        return Err(format!(
            "framing type `{name}` puts more than {MAX_STATIONS} joists across its {wide:.0} mm; frame a narrower floor or space \
             them wider"
        ));
    }
    if t.bridging > 0.0 && (wide / t.spacing + 1.0) * (long / t.bridging + 1.0) > MAX_BLOCKS {
        return Err(format!(
            "framing type `{name}`'s rows of blocking every {} mm make more than {MAX_BLOCKS} blocks in it; space them wider \
             (`bridging`) or make it 0",
            t.bridging
        ));
    }
    Ok(())
}

/// Where the line `across` = `a` lies inside `ring` (in the floor's frame), as `[from, to]` along `s`, in order.
fn inside(ring: &[P2], a: f64) -> Vec<[f64; 2]> {
    let mut xs: Vec<f64> =
        geom::ring_edges(ring).filter(|(p, q)| (p.1 > a) != (q.1 > a)).map(|(p, q)| p.0 + (a - p.1) * (q.0 - p.0) / (q.1 - p.1)).collect();
    xs.sort_by(f64::total_cmp);
    xs.as_chunks::<2>().0.iter().copied().filter(|i| i[1] - i[0] > EPS).collect()
}

/// What two sets of intervals share.
fn both(x: &[[f64; 2]], y: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for a in x {
        for b in y {
            let (lo, hi) = (a[0].max(b[0]), a[1].min(b[1]));
            if hi - lo > EPS {
                out.push([lo, hi]);
            }
        }
    }
    out.sort_by(|a, b| a[0].total_cmp(&b[0]));
    out
}

/// Intervals `x` less those of `y`.
fn less(x: &[[f64; 2]], y: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out = x.to_vec();
    for b in y {
        out = out
            .into_iter()
            .flat_map(|a| [[a[0], a[1].min(b[0])], [a[0].max(b[1]), a[1]]].into_iter().filter(|i| i[1] - i[0] > EPS))
            .collect();
    }
    out
}

/// Whether two intervals overlap by more than a hair.
pub(super) fn overlap(a: [f64; 2], b: [f64; 2]) -> bool {
    a[1].min(b[1]) - a[0].max(b[0]) > EPS
}

/// Where the strip `across` = `a0` to `a1` lies wholly inside `ring` and outside each of `cuts` (all in the floor's frame),
/// as `[from, to]` along `s`: where both its sides do, less wherever an edge of any of them runs between its sides (a corner
/// of a hole, or an inward corner of the outline, poking into it), so a member there touches what it ends on and never runs
/// into it.
pub(super) fn strip(ring: &[P2], cuts: &[Vec<P2>], [a0, a1]: [f64; 2]) -> Vec<[f64; 2]> {
    let (lo, hi) = if a1 - a0 > 2.0 * NUDGE { (a0 + NUDGE, a1 - NUDGE) } else { ((a0 + a1) / 2.0, (a0 + a1) / 2.0) };
    let side = |a: f64| cuts.iter().fold(inside(ring, a), |iv, c| less(&iv, &inside(c, a)));
    let blocked: Vec<[f64; 2]> = std::iter::once(ring)
        .chain(cuts.iter().map(Vec::as_slice))
        .flat_map(geom::ring_edges)
        .filter_map(|(p, q)| between(p, q, lo, hi))
        .collect();
    less(&both(&side(lo), &side(hi)), &blocked)
}

/// The part of edge `p`→`q` strictly between `across` = `lo` and `hi`, as how far it reaches along `s`; none where it doesn't
/// lie between them or reaches no distance along `s` there.
fn between(p: P2, q: P2, lo: f64, hi: f64) -> Option<[f64; 2]> {
    let d = q.1 - p.1;
    let (t0, t1) = if d.abs() < 1e-12 {
        if p.1 > lo && p.1 < hi { (0.0, 1.0) } else { return None }
    } else {
        let (ta, tb) = ((lo - p.1) / d, (hi - p.1) / d);
        (ta.min(tb).max(0.0), ta.max(tb).min(1.0))
    };
    let (s0, s1) = (p.0 + (q.0 - p.0) * t0, p.0 + (q.0 - p.0) * t1);
    (t1 > t0 && (s1 - s0).abs() > EPS).then_some([s0.min(s1), s0.max(s1)])
}

/// Whether convex rings `a` and `b` overlap by more than a hair: no side of either separates them.
pub(super) fn convex_overlap(a: &[P2], b: &[P2]) -> bool {
    let normals = |r: &[P2]| geom::ring_edges(r).map(|(p, q)| (q - p).unit().perp()).collect::<Vec<P2>>();
    let span = |r: &[P2], n: P2| r.iter().map(|p| p.dot(n)).fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], v| [lo.min(v), hi.max(v)]);
    !normals(a).into_iter().chain(normals(b)).filter(|n| n.length() > 0.5).any(|n| !overlap(span(a, n), span(b, n)))
}

/// Whether convex ring `q` lies inside `ring`, shrunk a hair so a side along one of `ring`'s edges counts as inside.
pub(super) fn within(q: &[P2], ring: &[P2]) -> bool {
    let c = q.iter().fold(P2(0.0, 0.0), |c, p| c + *p) * (1.0 / q.len().max(1) as f64);
    let q: Vec<P2> = q.iter().map(|p| *p + (c - *p).unit() * 1e-3).collect();
    q.iter().all(|p| geom::contains(ring, *p))
        && !ring.iter().any(|p| geom::contains(&q, *p))
        && !geom::ring_edges(&q).any(|(a, b)| geom::ring_edges(ring).any(|(c, d)| geom::crossing(a, b, c, d).is_some()))
}

/// Whether convex ring `q` and `ring` overlap: a corner of either inside the other, or their edges crossing.
pub(super) fn meets(q: &[P2], ring: &[P2]) -> bool {
    q.iter().any(|p| geom::contains(ring, *p))
        || ring.iter().any(|p| geom::contains(q, *p))
        || geom::ring_edges(q).any(|(a, b)| geom::ring_edges(ring).any(|(c, d)| geom::crossing(a, b, c, d).is_some()))
}

/// A rim joist along edge `edge` of the outline (in the floor's frame), from `a` along `u`, `ends` along it, a thickness in
/// from it to its left.
struct Rim {
    edge: usize,
    a: P2,
    u: P2,
    len: f64,
    ends: [f64; 2],
}

impl Rim {
    /// Square to the joists: lying in their frame.
    fn square(&self) -> bool {
        self.u.0.abs() * self.len <= TOL
    }

    /// Its plan rectangle from `e0` to `e1` along it, `th` thick, in the floor's frame.
    fn corners(&self, th: f64, [e0, e1]: [f64; 2]) -> Vec<P2> {
        let v = self.u.perp();
        vec![self.a + self.u * e0, self.a + self.u * e1, self.a + self.u * e1 + v * th, self.a + self.u * e0 + v * th]
    }

    /// The member's rectangle from `e0` to `e1`: [`Rim::corners`], or for one square to the joists the box round them in their
    /// frame, as it is laid.
    fn ring(&self, th: f64, ends: [f64; 2]) -> Vec<P2> {
        let c = self.corners(th, ends);
        if !self.square() {
            return c;
        }
        let fold = |k: fn(&P2) -> f64| c.iter().map(k).fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], x| [lo.min(x), hi.max(x)]);
        let ([s0, s1], [a0, a1]) = (fold(|p| p.0), fold(|p| p.1));
        vec![P2(s0, a0), P2(s1, a0), P2(s1, a1), P2(s0, a1)]
    }
}

/// What bears the joists or rafters: a beam or a wall under them, its centre line in the region's frame.
pub(super) struct Support {
    pub guid: Guid,
    /// Its centre line's ends, in the frame.
    pub ends: [P2; 2],
    /// And in plan.
    pub plan: [P2; 2],
    /// Half its width.
    pub half: f64,
    /// Its top at its ends (a beam's), or the wall whose top it is.
    pub top: Top,
}

pub(super) enum Top {
    Beam([f64; 2]),
    Wall(Id, f64),
}

impl Support {
    /// Where its centre line crosses the line `across` = `a`: how far along `s`, and how far along itself (0 to 1).
    pub(super) fn crossing(&self, a: f64) -> Option<(f64, f64)> {
        let [p, q] = self.ends;
        let da = q.1 - p.1;
        if da.abs() < EPS {
            return None;
        }
        let t = (a - p.1) / da;
        (-EPS..=1.0 + EPS).contains(&t).then_some((p.0 + t * (q.0 - p.0), t))
    }

    /// Its top `t` of the way along it.
    fn top(&self, m: &Model, t: f64) -> f64 {
        match &self.top {
            Top::Beam([z0, z1]) => z0 + (z1 - z0) * t,
            Top::Wall(id, len) => m.wall(*id).map_or(f64::NAN, |w| walls::top_at(m, w, t * len)),
        }
    }

    /// Where it crosses the line `across` = `a` with its top at `z`, along `s`.
    fn bears(&self, m: &Model, a: f64, z: f64) -> Option<f64> {
        self.crossing(a).filter(|(_, t)| (self.top(m, *t) - z).abs() <= BEARING).map(|(s, _)| s)
    }
}

/// Board `rim` (a rim along an edge from `a` across, running `u` across for each unit along it; a roof's ridge board) spliced
/// where it is longer than `longest` over the lines centred at `centres` across, none nearer its ends than `th`: in the frame
/// its length runs in, along `across` for one lying across, else along its own `s`. A warning starts with `who` and calls a
/// line `one`.
pub(super) fn spliced_rim(
    rim: Member,
    (a, u): (f64, f64),
    centres: &[f64],
    (th, longest): (f64, Option<f64>),
    (who, one): (&str, &str),
    warnings: &mut Vec<String>,
) -> Vec<Member> {
    let Some(longest) = longest.filter(|l| rim.length > l + EPS) else { return vec![rim] };
    let square = rim.grain == Grain::Across;
    let iv = if square { rim.across } else { rim.s };
    let mut over: Vec<f64> = centres
        .iter()
        .filter_map(|c| if square { Some(*c) } else { (u.abs() > EPS).then(|| (c - a) / u) })
        .filter(|x| *x > iv[0] + th - EPS && *x < iv[1] - th + EPS)
        .collect();
    over.sort_by(f64::total_cmp);
    over.dedup_by(|x, y| (*x - *y).abs() < EPS);
    let at = spliced(&over, iv, longest, rim.length / (iv[1] - iv[0]).max(EPS), 0.0, |len| {
        warnings.push(format!(
            "{who}{} has a piece {len:.0} mm long: no splice over a {one} keeps every piece within the longest stock length, \
             {longest:.0} mm, so it is longer than any stock; add stock",
            rim.key
        ));
    });
    if at.is_empty() {
        return vec![rim];
    }
    let ends: Vec<f64> = std::iter::once(iv[0]).chain(at).chain(std::iter::once(iv[1])).collect();
    ends.windows(2)
        .enumerate()
        .map(|(n, w)| {
            let part = [w.first().copied().unwrap_or(iv[0]), w.last().copied().unwrap_or(iv[1])];
            let (s, across) = if square { (rim.s, part) } else { (part, rim.across) };
            // Level along its length (a rim along `s` lies on a level region, one across or a ridge board level along it),
            // so its heights are the board's.
            let mut piece = Member::lying(
                format!("{}/{n}", rim.key),
                (rim.role, rim.grain),
                rim.section,
                rim.plies,
                [s, across],
                [rim.bottom, rim.top],
            );
            piece.frame = rim.frame;
            piece
        })
        .collect()
}

/// A hole a region's members frame round: an opening or a shaft, in the frame.
#[derive(Clone)]
pub(super) struct Hole {
    /// What a warning calls it: `opening 0`, `shaft 100123`.
    name: String,
    /// What its members' keys start with: `opening/0`, `opening/<shaft GUID>`.
    key: String,
    ring: Vec<P2>,
    /// The box round it: `[s0, s1]` and `[a0, a1]`.
    s: [f64; 2],
    a: [f64; 2],
    /// Trimmed: its box, grown by its trimmers and headers, is what its joists stop at; else its own ring.
    trimmed: bool,
}

impl Hole {
    /// Hole `ring` (in the frame), called `name` in warnings and `key` in its members' keys.
    pub(super) fn new(name: String, key: String, ring: Vec<P2>) -> Hole {
        let fold = |k: fn(&P2) -> f64| ring.iter().map(k).fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], v| [lo.min(v), hi.max(v)]);
        Hole { name, key, s: fold(|p| p.0), a: fold(|p| p.1), ring, trimmed: false }
    }

    /// Its box grown by `d` all round, as a ring.
    fn grown(&self, d: f64) -> Vec<P2> {
        let ([s0, s1], [a0, a1]) = (self.s, self.a);
        vec![P2(s0 - d, a0 - d), P2(s1 + d, a0 - d), P2(s1 + d, a1 + d), P2(s0 - d, a1 + d)]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Edge,
    Trimmer,
    Under,
    Grid,
}

/// A line of joists along `s`: its strip across, where it runs, and what it is.
struct Line {
    key: String,
    kind: Kind,
    across: [f64; 2],
    cover: Vec<[f64; 2]>,
}

impl Line {
    fn centre(&self) -> f64 {
        (self.across[0] + self.across[1]) / 2.0
    }

    fn clashes(&self, o: &Line) -> bool {
        overlap(self.across, o.across) && self.cover.iter().any(|a| o.cover.iter().any(|b| overlap(*a, *b)))
    }
}

/// Floor `id`'s framing: none when it isn't a framed floor; an error when it can't be framed, as `Model::check` says.
pub fn floor(m: &Model, id: Id) -> Result<Option<Framing>, String> {
    let Some(Element::Floor(f)) = m.elements.get(id) else { return Ok(None) };
    let Some(name) = &f.framing else { return Ok(None) };
    check(m, f)?;
    let (Some(t), Some((k, [z0, z1]))) = (m.types.framing.get(name), structure(m, f)) else { return Ok(None) };
    let layer = m.type_layers("floor", &f.type_name).get(k);
    let (origin, x) = frame(f);
    let y = x.perp();
    let local = |q: P2| P2((q - origin).dot(x), (q - origin).dot(y));
    let mut out = Framing {
        host: m.guid(id),
        type_name: name.clone(),
        layer: layer.map(|l| l.guid).unwrap_or_default(),
        material: t.material.clone().or_else(|| layer.map(|l| l.material.clone())).unwrap_or_default(),
        origin,
        x_axis: x,
        stock: t.stock.clone(),
        members: Vec::new(),
        warnings: Vec::new(),
        supports: Vec::new(),
        span: Some(0.0),
    };
    let ring: Vec<P2> = f.boundary.iter().map(|q| local(*q)).collect();
    if ring.len() < 3 {
        return Ok(Some(out));
    }
    let outer = ccw(ring.clone());

    // The holes: its openings, then the shafts through it.
    let bottom = walls::Plane::flat(m.floor_top(f).z - m.floor_depth(f)).lift(&ccw(f.boundary.clone()));
    let shafts = m.less_shafts(&bottom, m.floor_depth(f)).1;
    let rings = f.openings.iter().enumerate().map(|(i, r)| (format!("opening {i}"), format!("opening/{i}"), r.clone())).chain(
        shafts.iter().filter_map(|sid| match m.elements.get(*sid) {
            Some(Element::Shaft(sh)) => Some((format!("shaft {sid}"), format!("opening/{}", m.guid(*sid)), sh.boundary.clone())),
            _ => None,
        }),
    );
    let holes: Vec<Hole> = rings.map(|(name, key, ring)| Hole::new(name, key, ring.iter().map(|q| local(*q)).collect())).collect();

    // The walls standing on it, which a pair of joists goes under: straight, of its level, their base at its top.
    let top = m.floor_top(f).z;
    let plan = f.boundary.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |[x0, y0, x1, y1], q| {
        [x0.min(q.0), y0.min(q.1), x1.max(q.0), y1.max(q.1)]
    });
    let lookup = m.lookup();
    let mut standing = Vec::new();
    if t.under_partitions {
        for wid in lookup.walls_near_at(&[(plan, top - BEARING, top + BEARING)], 0.0) {
            let Some(w) = m.wall(wid).filter(|w| w.arc == 0.0 && w.level == f.level) else { continue };
            if (m.base_z(w) - top).abs() > BEARING {
                continue;
            }
            let axis = Axis::of(w);
            let c = walls::centre(m, w);
            standing.push((wid, m.guid(wid), local(axis.at(0.0, c)), local(axis.at(axis.length(), c))));
        }
    }

    // What bears them: beams, and straight walls, under it with their tops at the joists' underside.
    let (supports, curved) = gather(m, &outer, plan, [z0 - 4000.0, z0 + 1000.0], [z0 - BEARING, z0 + BEARING], &local);
    for (wid, w) in curved {
        if (m.top_z(w) - z0).abs() <= BEARING {
            out.warnings.push(format!("wall {wid} under it is curved: it isn't taken to bear the joists; check what bears them there"));
        }
    }
    let flat = |_: [f64; 2]| [[z0; 2], [z1; 2]];
    let bears = |sup: &Support, a: f64| sup.bears(m, a, z0);
    let along = |sup: &Support| (sup.top(m, 0.5) - z0).abs() <= BEARING;
    let framed = region(&Region {
        t,
        words: JOISTS,
        prefix: String::new(),
        who: String::new(),
        origin,
        x,
        ring,
        outer_edge: &|_, _| true,
        skewed_rims: true,
        holes,
        cuts: Vec::new(),
        section: [t.thickness, z1 - z0],
        z: &flat,
        supports: &supports,
        bears: &bears,
        along: &along,
        splice: Splice::Every,
        lap_edges: false,
        standing,
    });
    out.members = framed.members;
    out.warnings.extend(framed.warnings);
    out.supports = framed.bearing;
    out.span = Some(framed.longest);
    if out.supports.is_empty() && framed.lines > 0 {
        out.warnings.push(format!(
            "nothing bears its joists: no beam or wall under it has its top at their underside ({z0:.0} mm) within {BEARING} mm, so \
             they span the whole floor, {:.0} mm: put beams or walls under them, or check the span",
            framed.longest
        ));
    }
    Ok(Some(out))
}

/// The beams and the straight walls whose centre lines come near ring `outer` (in a frame `local` maps plan points into),
/// beams in the height band `beams` and walls in `walls` over the plan box `plan` ([x0, y0, x1, y1]); each in the frame, by
/// GUID. And the curved walls there, which bear nothing, for the caller to warn of.
pub(super) fn gather<'m>(
    m: &'m Model,
    outer: &[P2],
    plan: [f64; 4],
    beams: [f64; 2],
    wall_band: [f64; 2],
    local: &dyn Fn(P2) -> P2,
) -> (Vec<Support>, Vec<(Id, &'m crate::model::Wall)>) {
    let lookup = m.lookup();
    let mut supports: Vec<Support> = Vec::new();
    let mut curved = Vec::new();
    let near_plan = |ends: [P2; 2], half: f64| {
        let (lo, hi) = (P2(ends[0].0.min(ends[1].0), ends[0].1.min(ends[1].1)), P2(ends[0].0.max(ends[1].0), ends[0].1.max(ends[1].1)));
        let (blo, bhi) = outer.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), p| {
            (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))
        });
        lo.0 <= bhi.0 + half && blo.0 <= hi.0 + half && lo.1 <= bhi.1 + half && blo.1 <= hi.1 + half
    };
    for (g, _) in lookup.groups() {
        if !matches!(g, Group::Run { category: "beam", .. }) {
            continue;
        }
        for bid in lookup.group_between(g, beams[0], beams[1]) {
            let Some(e @ Element::Beam(_)) = m.elements.get(bid) else { continue };
            let Some(geo) = m.run_geom(e).filter(|g| !g.vertical()) else { continue };
            let plan_ends = [P2(geo.a[0], geo.a[1]), P2(geo.b[0], geo.b[1])];
            let ends = plan_ends.map(local);
            let half = geo.half();
            if near_plan(ends, geo.width / 2.0) {
                supports.push(Support {
                    guid: m.guid(bid),
                    ends,
                    plan: plan_ends,
                    half: geo.width / 2.0,
                    top: Top::Beam([geo.a[2] + half, geo.b[2] + half]),
                });
            }
        }
    }
    for wid in lookup.walls_near_at(&[(plan, wall_band[0], wall_band[1])], 0.0) {
        let Some(w) = m.wall(wid) else { continue };
        let axis = Axis::of(w);
        let c = walls::centre(m, w);
        let plan_ends = [axis.at(0.0, c), axis.at(axis.length(), c)];
        let ends = plan_ends.map(local);
        if !near_plan(ends, m.thickness(w) / 2.0) {
            continue;
        }
        if w.arc != 0.0 {
            curved.push((wid, w));
            continue;
        }
        supports.push(Support { guid: m.guid(wid), ends, plan: plan_ends, half: m.thickness(w) / 2.0, top: Top::Wall(wid, axis.length()) });
    }
    supports.sort_by_key(|s| s.guid);
    (supports, curved)
}

/// What a region's members are called in its keys and warnings.
#[derive(Clone, Copy)]
pub(super) struct Words {
    /// What its lines are: joists or rafters.
    pub role: Role,
    pub one: &'static str,
    pub many: &'static str,
    /// What it is: a floor, a face of a roof.
    pub host: &'static str,
}

pub(super) const JOISTS: Words = Words { role: Role::Joist, one: "joist", many: "joists", host: "floor" };

/// How a region's lines are cut over what bears them: a floor's joists over each support away from their ends; a roof's
/// rafters only where a line is longer than `longest` along its slope (`sec` times as long as in plan), into the fewest
/// pieces that holds and, of those splits, the one whose longest piece is shortest ([`spliced`]).
#[derive(Clone, Copy)]
pub(super) enum Splice {
    Every,
    Stock { longest: f64, sec: f64 },
}

/// A region framed on lines of joists or rafters, in its own frame: `s` the way they run from `origin` along `x`, `across`
/// to its left.
pub(super) struct Region<'a> {
    pub t: &'a super::FramingType,
    pub words: Words,
    /// What each key starts with: nothing for a floor, `face/<i>/` for a roof's face.
    pub prefix: String,
    /// What each warning starts with: nothing for a floor, `face <i>: ` for a roof's.
    pub who: String,
    pub origin: P2,
    pub x: P2,
    /// Its outline in the frame, as given: its edges are numbered as it runs.
    pub ring: Vec<P2>,
    /// Whether an edge from `p` to `q` (in the frame) is on the host's edge, and so takes a rim or an edge joist: a roof's
    /// face's edge along a hip, a valley or a ridge takes neither.
    pub outer_edge: &'a dyn Fn(P2, P2) -> bool,
    /// Whether a rim may lie along an edge skewed to the lines (on a level region); else only along one square to them.
    pub skewed_rims: bool,
    pub holes: Vec<Hole>,
    /// What else its members run round, framed already (a roof's ridge boards), in the frame.
    pub cuts: Vec<Vec<P2>>,
    /// One member's section: its thickness and its depth square to its slope.
    pub section: [f64; 2],
    /// A member's bottom and top at the ends of its run `[s0, s1]` along `s`: `[[bottom at s0, at s1], [top at s0, at s1]]`.
    pub z: &'a dyn Fn([f64; 2]) -> [[f64; 2]; 2],
    pub supports: &'a [Support],
    /// Where a support bears the line `across` = `a`, along `s`; none where it doesn't.
    pub bears: &'a dyn Fn(&Support, f64) -> Option<f64>,
    /// Whether a support running along the lines bears what lies along it.
    pub along: &'a dyn Fn(&Support) -> bool,
    pub splice: Splice,
    /// Whether an edge line laps as a grid line does (a roof's edge rafters); else it butts (a floor's edge joists).
    pub lap_edges: bool,
    /// The walls standing on it that a pair of joists goes under: each id, GUID and its centre line's ends in the frame.
    pub standing: Vec<(Id, Guid, P2, P2)>,
}

/// What a region frames: its members (in its frame), the warnings, what bears its lines, their longest clear span (along
/// `s`, so in plan) and how many lines it has.
pub(super) struct Framed {
    pub members: Vec<Member>,
    pub warnings: Vec<String>,
    pub bearing: Vec<Guid>,
    pub longest: f64,
    pub lines: usize,
}

/// The stations of a grid `spacing` apart across `[lo, hi]`, counted from where `layout` says, each with its number; none
/// nearer either end than half of `th`.
pub(super) fn stations(layout: Layout, [lo, hi]: [f64; 2], spacing: f64, th: f64) -> Vec<(i64, f64)> {
    let count = ((hi - lo) / spacing + EPS).floor() as i64;
    let all: Vec<(i64, f64)> = match layout {
        Layout::Start => (0..=count).map(|k| (k, lo + k as f64 * spacing)).collect(),
        Layout::End => (0..=count).map(|k| (k, hi - k as f64 * spacing)).collect(),
        Layout::Centre => {
            let half = ((hi - lo) / 2.0 / spacing + EPS).floor() as i64;
            (-half..=half).map(|k| (k, (lo + hi) / 2.0 + k as f64 * spacing)).collect()
        }
    };
    all.into_iter().filter(|(_, c)| *c >= lo + th / 2.0 - EPS && *c <= hi - th / 2.0 + EPS).collect()
}

/// Region `g`'s members: its rims, lines of joists or rafters in pieces, trimmers and headers round its holes and rows of
/// blocking, as the module's rules say.
pub(super) fn region(g: &Region) -> Framed {
    let (t, words) = (g.t, g.words);
    let (origin, x) = (g.origin, g.x);
    let y = x.perp();
    let th = t.thickness;
    let section = g.section;
    let zs = g.z;
    let pre = |k: &str| format!("{}{k}", g.prefix);
    let mut out = Framed { members: Vec::new(), warnings: Vec::new(), bearing: Vec::new(), longest: 0.0, lines: 0 };
    let n = g.ring.len();
    if n < 3 {
        return out;
    }
    // Its edges are numbered as its outline has them, whichever way that runs.
    let turned = geom::area(&g.ring) < 0.0;
    let edge_no = |i: usize| if turned { (2 * n - 2 - i % n) % n } else { i % n };
    let outer = ccw(g.ring.clone());
    let pt = |i: usize| outer.get(i % n).copied().unwrap_or_default();
    let (amin, amax) = outer.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    let mut holes: Vec<Hole> = g.holes.iter().map(|h| Hole { trimmed: false, ..h.clone() }).collect();
    let who = &g.who;
    let warnings = &mut out.warnings;

    // Its edges: along the lines (an edge joist), or not (a rim); each rim's inner face.
    let parallel: Vec<bool> = (0..n).map(|i| (pt(i + 1).1 - pt(i).1).abs() <= TOL).collect();
    let on_edge: Vec<bool> = (0..n).map(|i| (g.outer_edge)(pt(i), pt(i + 1))).collect();
    let square = |i: usize| (pt(i + 1).0 - pt(i).0).abs() <= TOL;
    let rimmed = |i: usize| {
        t.rim
            && on_edge.get(i % n).copied().unwrap_or(false)
            && !parallel.get(i % n).copied().unwrap_or(true)
            && (g.skewed_rims || square(i))
            && pt(i).dist(pt(i + 1)) > EPS
    };
    // An edge on the host's edge within a degree of the lines, unrimmed: an edge line along it, as along one that's parallel.
    let along: Vec<bool> = (0..n)
        .map(|i| {
            let d = pt(i + 1) - pt(i);
            parallel[i] || (on_edge[i] && !rimmed(i) && d.length() > EPS && d.1.abs() <= d.length() * PARALLEL.to_radians().sin())
        })
        .collect();
    if !g.skewed_rims {
        for i in (0..n).filter(|i| on_edge[*i] && !along[*i] && !square(*i) && pt(*i).dist(pt(*i + 1)) > EPS) {
            warnings.push(format!(
                "{who}its edge {} runs skewed to the {}: no rim or edge {} is put along it, and the {} end at it square; frame it \
                 by hand",
                edge_no(i),
                words.many,
                words.one,
                words.many
            ));
        }
    }
    let shifted = |i: usize| {
        let (a, b) = (pt(i), pt(i + 1));
        let d = if rimmed(i) { th } else { 0.0 };
        let shift = (b - a).unit().perp() * d;
        (a + shift, b + shift)
    };

    // The rims, each along its edge, as long as its corners let it: where two meet at a corner the one of the lower edge
    // index runs through and the other stops at its inner face; at a sharp corner each stops where its inner side meets the
    // outline; at an inward one the one that runs through runs on to the other's inner face, and that one starts clear of it.
    let convex = |p: P2, q: P2, r: P2| (q - p).cross(r - q) > EPS;
    let mut rims: Vec<Rim> = (0..n)
        .filter(|i| rimmed(*i))
        .map(|i| {
            let (a, b) = (pt(i), pt(i + 1));
            let (len, u) = (a.dist(b), (b - a).unit());
            let v = u.perp();
            // Where its side `off` in from its edge crosses the line through `p` and `q`, along it.
            let at = |off: f64, (p, q): (P2, P2)| geom::line_intersection(a + v * off, b + v * off, p, q).map(|x| (x - a).dot(u));
            let meet = |j: usize| [0.0, th].map(|off| at(off, shifted(j)).unwrap_or(f64::NAN));
            let mut ends = [0.0, len];
            let (prev, next) = ((i + n - 1) % n, (i + 1) % n);
            if convex(pt(prev), a, b) {
                if i > 0 && rimmed(prev) {
                    ends[0] = meet(prev).into_iter().filter(|x| x.is_finite()).fold(0.0, f64::max);
                }
                if let Some(x) = at(th, (pt(prev), a)).filter(|x| x.is_finite()) {
                    ends[0] = ends[0].max(x);
                }
            }
            if convex(a, b, pt(i + 2)) {
                if next < i && rimmed(next) {
                    ends[1] = meet(next).into_iter().filter(|x| x.is_finite()).fold(len, f64::min);
                }
                if let Some(x) = at(th, (b, pt(i + 2))).filter(|x| x.is_finite()) {
                    ends[1] = ends[1].min(x);
                }
            }
            Rim { edge: i, a, u, len, ends }
        })
        .collect();
    for k in 0..n {
        let (i, j) = ((k + n - 1) % n, k);
        let (before, corner, after) = (pt(i), pt(k), pt(k + 1));
        if (corner - before).cross(after - corner) >= -EPS {
            continue;
        }
        let (Some(ri), Some(rj)) = (rims.iter().position(|r| r.edge == i), rims.iter().position(|r| r.edge == j)) else { continue };
        let ((p, q), (c, d)) = (shifted(i), shifted(j));
        let Some(inner) = geom::line_intersection(p, q, c, d) else { continue };
        let (Some(was_i), Some(was_j)) = (rims.get(ri).map(|r| r.ends), rims.get(rj).map(|r| r.ends)) else { continue };
        let (Some(rin), Some(rout)) = (rims.get(ri), rims.get(rj)) else { continue };
        let (ui, uj) = (rin.u, rout.u);
        let cos = ui.dot(uj);
        // The one that runs through reaches the corner of their inner faces; the other is cut back clear of it, along its own way.
        let mut ends = [was_i, was_j];
        let through_in = i < j;
        if through_in {
            ends[0][1] = rin.len + (inner - corner).dot(ui).max(0.0);
            if cos > 0.0 {
                ends[1][0] = rin.corners(th, ends[0]).iter().map(|c| (*c - corner).dot(uj)).fold(ends[1][0], f64::max);
            }
        } else {
            ends[1][0] = -(corner - inner).dot(uj).max(0.0);
            if cos > 0.0 {
                ends[0][1] = rout.corners(th, ends[1]).iter().map(|c| (*c - rin.a).dot(ui)).fold(ends[0][1], f64::min);
            }
        }
        let (ri_ring, rj_ring) = (rin.corners(th, ends[0]), rout.corners(th, ends[1]));
        let fits = !convex_overlap(&ri_ring, &rj_ring) && within(if through_in { &ri_ring } else { &rj_ring }, &outer);
        let gap = if !fits {
            th
        } else if cos > 0.0 {
            if through_in { ends[1][0] - was_j[0] } else { was_i[1] - ends[0][1] }
        } else {
            -cos * th
        };
        if fits {
            for (k, e) in [(ri, ends[0]), (rj, ends[1])] {
                if let Some(r) = rims.get_mut(k) {
                    r.ends = e;
                }
            }
        }
        if gap > 1.0 {
            warnings.push(format!(
                "{who}the rims along its edges {} and {} meet at an inward corner that isn't square: a wedge up to {gap:.0} mm wide \
                 between them is left unfilled; frame it by hand",
                edge_no(i),
                edge_no(j)
            ));
        }
    }
    // Each rim, its pieces clear of the holes and what else is framed, each one that would overlap another rim left out.
    let mut rim_members: Vec<Member> = Vec::new();
    // Each rim member's edge's start and way, `across` (where a line `across` = c crosses it: `(c - a) / u` along it).
    let mut rim_axes: Vec<(f64, f64)> = Vec::new();
    let (mut rim_rings, mut rim_corners): (Vec<Vec<P2>>, Vec<Vec<P2>>) = (Vec::new(), Vec::new());
    for r in &rims {
        let key = pre(&format!("rim/{}", edge_no(r.edge)));
        if r.ends[1] - r.ends[0] < th - EPS {
            warnings.push(format!("{who}the rim along its edge {} would be shorter than it is thick: it's left out", edge_no(r.edge)));
            continue;
        }
        let whole = r.corners(th, r.ends);
        if !within(&whole, &outer) {
            warnings.push(format!(
                "{who}the rim along its edge {} doesn't fit on the {}: it's thinner there than a rim is thick; it's left out",
                edge_no(r.edge),
                words.host
            ));
            continue;
        }
        if let Some(o) = rim_corners.iter().position(|o| convex_overlap(o, &whole)) {
            let what = rim_members.get(o).map_or_else(String::new, |m| m.key.clone());
            warnings.push(format!(
                "{who}the rim along its edge {} would overlap {what}: it's left out; the {} is shorter there than two rims are thick",
                edge_no(r.edge),
                words.host
            ));
            continue;
        }
        let v = r.u.perp();
        let own = |q: &P2| P2((*q - r.a).dot(r.u), (*q - r.a).dot(v));
        let [e0, e1] = r.ends;
        let base = vec![P2(e0, -1.0), P2(e1, -1.0), P2(e1, th + 1.0), P2(e0, th + 1.0)];
        let cut: Vec<Vec<P2>> =
            holes.iter().map(|h| h.ring.iter().map(own).collect()).chain(g.cuts.iter().map(|c| c.iter().map(own).collect())).collect();
        let pieces = strip(&base, &cut, [0.0, th]);
        let one = pieces.len() == 1;
        for (j, p) in pieces.into_iter().enumerate() {
            let key = if one { key.clone() } else { format!("{key}/{j}") };
            if p[1] - p[0] < th - EPS {
                warnings.push(format!(
                    "{who}a piece of {key} {:.0} mm long, shorter than it is thick, is left out: move what cuts it there",
                    p[1] - p[0]
                ));
                continue;
            }
            let ring = r.ring(th, p);
            let member = if r.square() {
                let fold =
                    |k: fn(&P2) -> f64| ring.iter().map(k).fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], x| [lo.min(x), hi.max(x)]);
                let s = fold(|p| p.0);
                Member::lying(key, (Role::Rim, Grain::Across), section, 1, [s, fold(|p| p.1)], zs(s))
            } else {
                // Along a skewed edge, in a frame of its own: only on a level region, so its heights are the same anywhere.
                let mut rim = Member::lying(key, (Role::Rim, Grain::Along), section, 1, [p, [0.0, th]], zs([r.a.0, r.a.0]));
                rim.frame = Some([origin + x * r.a.0 + y * r.a.1, x * r.u.0 + y * r.u.1]);
                rim
            };
            rim_members.push(member);
            rim_axes.push((r.a.1, r.u.1));
            rim_rings.push(ring);
            rim_corners.push(r.corners(th, p));
        }
    }
    let plies = f64::from(t.trimmers);
    let grow = plies * th;

    // The lines, each with where it runs: edge lines first, then trimmers, pairs under walls and the grid; a line that would
    // overlap one before it is left out.
    // What the lines run round: the rims, what else is framed, and each hole (its box grown by its trimmers and headers, where
    // it is trimmed).
    let cuts = |holes: &[Hole], trims: Option<usize>| -> Vec<Vec<P2>> {
        rim_rings
            .iter()
            .cloned()
            .chain(g.cuts.iter().cloned())
            .chain(
                holes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| Some(*i) != trims)
                    .map(|(_, h)| if h.trimmed { h.grown(grow) } else { h.ring.clone() }),
            )
            .collect()
    };
    let cover = |cuts: &[Vec<P2>], across: [f64; 2]| strip(&outer, cuts, across);
    let mut lines: Vec<Line> = Vec::new();
    for i in (0..n).filter(|i| along.get(*i).copied().unwrap_or(false) && on_edge.get(*i).copied().unwrap_or(false)) {
        let (a, b) = (pt(i), pt(i + 1));
        if a.dist(b) <= EPS {
            continue;
        }
        let drift = (b.1 - a.1).abs();
        if drift > TOL {
            warnings.push(format!(
                "{who}its edge {} runs {:.2}° off the {}: the edge {} along it is flush with it where it lies farthest in, {drift:.0} \
                 mm in from it at the other end; frame that wedge by hand if it matters",
                edge_no(i),
                (drift / a.dist(b)).asin().to_degrees(),
                words.many,
                words.one
            ));
        }
        // Flush with the edge where it lies farthest in, so the whole line is on the region.
        let across = if b.0 > a.0 { [a.1.max(b.1), a.1.max(b.1) + th] } else { [a.1.min(b.1) - th, a.1.min(b.1)] };
        let along = [a.0.min(b.0), a.0.max(b.0)];
        let mut line = Line {
            key: pre(&format!("edge/{}", edge_no(i))),
            kind: Kind::Edge,
            across,
            cover: cover(&cuts(&holes, None), across).into_iter().filter(|c| overlap(*c, along)).collect(),
        };
        // Along an edge in line with one before it: the same line, where that one runs.
        if let Some(o) = lines.iter().find(|o| o.kind == Kind::Edge && (o.across[0] - across[0]).abs() < EPS) {
            line.cover.retain(|c| !o.cover.iter().any(|d| overlap(*c, *d)));
            if line.cover.is_empty() {
                continue;
            }
        }
        if let Some(o) = lines.iter().find(|o| o.clashes(&line)) {
            warnings.push(format!(
                "{who}the edge {} along its edge {} would overlap {}: it's left out; the {} is narrower there than two {}",
                words.one,
                edge_no(i),
                o.key,
                words.host,
                words.many
            ));
            continue;
        }
        if !line.cover.is_empty() {
            lines.push(line);
        } else if along[1] - along[0] > th && cover(&[], across).iter().all(|c| !overlap(*c, along)) {
            warnings.push(format!(
                "{who}the edge {} along its edge {} is left out: the {} is narrower there than a {} is thick",
                words.one,
                edge_no(i),
                words.host,
                words.one
            ));
        }
    }
    // A hole is trimmed where its box, grown by its trimmers, lies inside the outline, clear of the rims, what else is framed
    // and the edge lines, and its trimmers clear of another's.
    for i in 0..holes.len() {
        let Some(h) = holes.get(i) else { continue };
        let band = [h.a[0] - grow, h.a[1] + grow];
        let reach = [h.s[0] - grow, h.s[1] + grow];
        let boxed = h.grown(grow);
        let inside_rims =
            within(&boxed, &outer) && !rim_corners.iter().any(|r| convex_overlap(r, &boxed)) && !g.cuts.iter().any(|c| meets(&boxed, c));
        let why = if !inside_rims || lines.iter().any(|l| overlap(l.across, band) && l.cover.iter().any(|c| overlap(*c, reach))) {
            Some(format!("it reaches the {}'s edge, or comes nearer it than its trimmers and headers are thick", words.host))
        } else {
            let into =
                holes.iter().enumerate().find(|(j, o)| *j != i && meets(&boxed, &if o.trimmed { o.grown(grow) } else { o.ring.clone() }));
            into.map(|(_, o)| format!("its box, grown by its trimmers and headers, runs into {}", o.name)).or_else(|| {
                holes
                    .iter()
                    .take(i)
                    .find(|o| o.trimmed && overlap([o.a[0] - grow, o.a[1] + grow], band))
                    .map(|o| format!("its trimmers would run into {}'s, which run the {}' whole length beside it", o.name, words.many))
            })
        };
        let Some(h) = holes.get_mut(i) else { continue };
        match why {
            Some(why) => warnings.push(format!(
                "{who}{} isn't trimmed: {why}; the {} it cuts end at it with nothing to bear them: move it or frame it by hand",
                h.name, words.many
            )),
            None => {
                h.trimmed = true;
                let (w, l) = (h.a[1] - h.a[0], h.s[1] - h.s[0]);
                if (geom::area(&h.ring).abs() - w * l).abs() > 1.0 {
                    warnings.push(format!(
                        "{who}{} isn't a rectangle square to the {}: it's framed by the box round it, {w:.0} across by {l:.0} \
                         along them",
                        h.name, words.many
                    ));
                }
            }
        }
    }
    for (i, h) in holes.iter().enumerate().filter(|(_, h)| h.trimmed) {
        for (side, sign, face) in [("right", -1.0, h.a[0]), ("left", 1.0, h.a[1])] {
            for ply in 0..t.trimmers {
                let near = face + sign * f64::from(ply) * th;
                let across = if sign < 0.0 { [near - th, near] } else { [near, near + th] };
                let cover = cover(&cuts(&holes, Some(i)), across);
                lines.push(Line { key: pre(&format!("{}/trimmer/{side}/{ply}", h.key)), kind: Kind::Trimmer, across, cover });
            }
        }
    }
    for (wid, wg, p, q) in &g.standing {
        let d = (*q - *p).unit();
        if d.1.abs() > PARALLEL.to_radians().sin() || !geom::contains(&outer, (*p + *q) * 0.5) {
            continue;
        }
        let mid = (p.1 + q.1) / 2.0;
        let pair: Vec<Line> = [[mid - th, mid], [mid, mid + th]]
            .into_iter()
            .enumerate()
            .map(|(ply, across)| Line {
                key: pre(&format!("under/{wg}/{ply}")),
                kind: Kind::Under,
                across,
                cover: cover(&cuts(&holes, None), across),
            })
            .collect();
        let reach = [p.0.min(q.0), p.0.max(q.0)];
        let over = holes.iter().find(|h| {
            let band = if h.trimmed { [h.a[0] - grow, h.a[1] + grow] } else { h.a };
            overlap(band, [mid - th, mid + th]) && overlap(h.s, reach)
        });
        // Over an edge line where its centre line is.
        let edge = pair
            .iter()
            .any(|l| lines.iter().any(|o| o.kind == Kind::Edge && o.clashes(l) && mid >= o.across[0] - EPS && mid <= o.across[1] + EPS));
        let clash = pair.iter().find_map(|l| lines.iter().find(|o| o.clashes(l)));
        if let Some(h) = over {
            warnings.push(format!(
                "{who}wall {wid} stands along the {} over {}: no pair of {} is put under it; move the wall or the opening, or \
                 frame under it by hand",
                words.many, h.name, words.many
            ));
        } else if edge {
            // Over an edge line, it stands on that.
        } else if let Some(o) = clash {
            warnings.push(format!(
                "{who}wall {wid} stands along the {} over {}: no pair of {} is put under it; move the wall or check that what \
                 is under it carries it",
                words.many, o.key, words.many
            ));
        } else {
            lines.extend(pair.into_iter().filter(|l| !l.cover.is_empty()));
        }
    }
    // The grid: a line centred at each station clear of the others.
    let grid_cuts = cuts(&holes, None);
    for (k, c) in stations(t.layout, [amin, amax], t.spacing, th) {
        let across = [c - th / 2.0, c + th / 2.0];
        let line = Line { key: pre(&format!("{}/{k}", words.one)), kind: Kind::Grid, across, cover: cover(&grid_cuts, across) };
        if !line.cover.is_empty() && !lines.iter().any(|o| o.clashes(&line)) {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        warnings.push(format!(
            "{who}no {} fits on the {}: it's narrower than a {} is thick, or its rims and holes take it all; frame it by hand",
            words.one, words.host, words.one
        ));
    }

    // What bears them.
    let supports = g.supports;
    let mut bearing: Vec<Guid> = Vec::new();
    for sup in supports {
        let bears = lines.iter().any(|l| {
            let [p, q] = sup.ends;
            let reach = [p.0.min(q.0), p.0.max(q.0)];
            let along =
                (p.1 - q.1).abs() < EPS && (p.1 - l.centre()).abs() <= sup.half + th / 2.0 && l.cover.iter().any(|c| overlap(*c, reach));
            let crossing =
                (g.bears)(sup, l.centre()).is_some_and(|s| l.cover.iter().any(|c| s > c[0] - th - sup.half && s < c[1] + th + sup.half));
            crossing || (along && (g.along)(sup))
        });
        if bears {
            bearing.push(sup.guid);
        }
    }

    // Each line in pieces: cut over the supports it crosses away from its ends that the splice takes; a grid line's (and, as
    // the region says, an edge line's) lapped where the type says.
    // Each piece placed: its line, and where it lies along and across.
    let mut placed: Vec<(usize, [f64; 2], [f64; 2])> = Vec::new();
    // Each line's pieces between what bears it, before any lap.
    let mut spans: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut longest: f64 = 0.0;
    let lap = t.lap;
    let laps_kind = |k: Kind| k == Kind::Grid || (g.lap_edges && k == Kind::Edge);
    for (li, line) in lines.iter().enumerate() {
        let c = line.centre();
        let mut butt: Vec<[f64; 2]> = Vec::new();
        // Each run's pieces as lapped, with the run.
        let mut laps: Vec<([f64; 2], Vec<[f64; 2]>)> = Vec::new();
        for iv in &line.cover {
            let mut over: Vec<f64> = supports
                .iter()
                .filter_map(|sup| {
                    let s = (g.bears)(sup, c)?;
                    let margin = sup.half.max(th);
                    (s > iv[0] + margin && s < iv[1] - margin).then_some(s)
                })
                .collect();
            over.sort_by(f64::total_cmp);
            over.dedup_by(|a, b| (*a - *b).abs() < th);
            let at = match g.splice {
                Splice::Every => over.clone(),
                Splice::Stock { longest: most, sec } => {
                    let reach = if laps_kind(line.kind) { lap } else { 0.0 };
                    spliced(&over, *iv, most, sec, reach, |len| {
                        warnings.push(format!(
                            "{who}{} has a piece {len:.0} mm along its slope: no splice over what bears it keeps every piece within \
                             the longest stock length, {most:.0} mm, so it is longer than any stock; add stock or a support under it",
                            line.key
                        ));
                    })
                }
            };
            let cuts: Vec<f64> = std::iter::once(iv[0]).chain(at.iter().copied()).chain(std::iter::once(iv[1])).collect();
            let pieces: Vec<[f64; 2]> = cuts.windows(2).map(|w| [w[0], w[1]]).collect();
            // Its clear span: between what bears each piece, its ends bearing on what they end on.
            for p in &pieces {
                let mut bears: Vec<[f64; 2]> = supports
                    .iter()
                    .filter_map(|sup| {
                        (g.bears)(sup, c).filter(|s| *s > p[0] - sup.half && *s < p[1] + sup.half).map(|s| [s - sup.half, s + sup.half])
                    })
                    .chain([[p[0], p[0]], [p[1], p[1]]])
                    .collect();
                bears.sort_by(|a, b| a[0].total_cmp(&b[0]));
                let mut reach = bears.first().map_or(p[0], |b| b[1]);
                for b in bears.iter().skip(1) {
                    longest = longest.max(b[0] - reach);
                    reach = reach.max(b[1]);
                }
            }
            if laps_kind(line.kind) && lap > 0.0 && !at.is_empty() {
                let k = at.len();
                // Over each support, the lap, or half the way to the next support where that is nearer, so the pieces in line
                // with each other never meet.
                let laps_at: Vec<f64> = (0..k)
                    .map(|j| {
                        let s = at.get(j).copied().unwrap_or(iv[0]);
                        let before = if j == 0 { f64::INFINITY } else { (s - at.get(j - 1).copied().unwrap_or(iv[0])) / 2.0 };
                        let after = at.get(j + 1).map_or(f64::INFINITY, |n| (n - s) / 2.0);
                        lap.min(before).min(after)
                    })
                    .collect();
                if let Some(short) = laps_at.iter().copied().filter(|l| *l < lap - EPS).reduce(f64::min) {
                    warnings.push(format!(
                        "{who}{} laps only {short:.0} mm past a support, not {lap}: what bears it is less than twice that apart there",
                        line.key
                    ));
                }
                laps.push((
                    *iv,
                    (0..=k)
                        .map(|j| {
                            let lap_at = |j: usize| laps_at.get(j).copied().unwrap_or(0.0);
                            let from = if j == 0 { iv[0] } else { (at.get(j - 1).copied().unwrap_or(iv[0]) - lap_at(j - 1)).max(iv[0]) };
                            let to = if j == k { iv[1] } else { (at.get(j).copied().unwrap_or(iv[1]) + lap_at(j)).min(iv[1]) };
                            [from, to]
                        })
                        .collect(),
                ));
            } else {
                laps.push((*iv, pieces.clone()));
            }
            butt.extend(pieces);
        }
        // Every other piece of a lapped line a thickness over, the way that clears every other member and stays on the region.
        let lapped = laps.iter().any(|(_, ps)| ps.len() > 1) && laps_kind(line.kind) && lap > 0.0;
        // Its pieces with every other one `shift` over, if they all fit there clear of the rest: each that ends where its run
        // does (on a rim, a header) ends where its own strip does, so on a skewed rim it still meets it.
        let beside = |shift: f64| -> Option<Vec<([f64; 2], f64)>> {
            let across = [line.across[0] + shift, line.across[1] + shift];
            let room = cover(&grid_cuts, across);
            let free = |p: [f64; 2]| {
                !placed.iter().any(|(_, s, a)| overlap(*s, p) && overlap(*a, across))
                    && !lines
                        .iter()
                        .enumerate()
                        .any(|(o, l)| o != li && overlap(l.across, across) && l.cover.iter().any(|c| overlap(*c, p)))
            };
            let mut out = Vec::new();
            for (iv, ps) in &laps {
                for (j, p) in ps.iter().enumerate() {
                    if j % 2 == 0 {
                        out.push((*p, 0.0));
                        continue;
                    }
                    let mid = (p[0] + p[1]) / 2.0;
                    let c = room.iter().find(|c| c[0] < mid && c[1] > mid)?;
                    let q = [if (p[0] - iv[0]).abs() < EPS { c[0] } else { p[0] }, if (p[1] - iv[1]).abs() < EPS { c[1] } else { p[1] }];
                    if !(c[0] <= q[0] + EPS && c[1] >= q[1] - EPS && free(q)) {
                        return None;
                    }
                    out.push((q, shift));
                }
            }
            Some(out)
        };
        let lapping = if lapped { beside(th).or_else(|| beside(-th)) } else { None };
        if lapped && lapping.is_none() {
            warnings.push(format!(
                "{who}{} can't lap over what bears it: there's no room beside it for the next piece; its pieces butt on the support",
                line.key
            ));
        }
        let pieces: Vec<([f64; 2], f64)> = lapping.unwrap_or_else(|| butt.iter().map(|p| (*p, 0.0)).collect());
        let role = if line.kind == Kind::Trimmer { Role::Trimmer } else { words.role };
        for (j, (p, sh)) in pieces.into_iter().enumerate() {
            let across = [line.across[0] + sh, line.across[1] + sh];
            if p[1] - p[0] < th - EPS {
                warnings.push(format!(
                    "{who}a piece of {} {:.0} mm long, shorter than it is thick, is left out: move what cuts it there",
                    line.key,
                    p[1] - p[0]
                ));
                continue;
            }
            // A floor's, cut over every support (a roof's rafters, spliced by the stock, are warned of as they are spliced).
            let every = matches!(g.splice, Splice::Every);
            if let Some(&most) = t.stock.last().filter(|most| every && p[1] - p[0] > **most + EPS) {
                warnings.push(format!(
                    "{who}{}/{j} is {:.0} mm long between what bears it, longer than the longest stock length, {most:.0} mm, so it is \
                     longer than any stock; add stock or a support under it",
                    line.key,
                    p[1] - p[0]
                ));
            }
            placed.push((li, p, across));
            out.members.push(Member::lying(format!("{}/{j}", line.key), (role, Grain::Along), section, 1, [p, across], zs(p)));
        }
        spans.push(butt);
    }

    // Each rim longer than the longest stock length, spliced over the lines that end on it (a joint backed by a joist's or a
    // rafter's end), into the fewest pieces that length holds ([`spliced`]), each keyed by the rim's key and its place, from 0;
    // where no splice over a line keeps every piece within it, a warning.
    let centres: Vec<f64> = lines.iter().map(Line::centre).collect();
    for (rim, (a, u)) in rim_members.into_iter().zip(rim_axes) {
        out.members.extend(spliced_rim(rim, (a, u), &centres, (th, t.stock.last().copied()), (who, words.one), warnings));
    }

    // Each trimmed hole's headers, between its trimmers at each end.
    for h in holes.iter().filter(|h| h.trimmed) {
        for (end, sign, face) in [("start", -1.0, h.s[0]), ("end", 1.0, h.s[1])] {
            for ply in 0..t.trimmers {
                let near = face + sign * f64::from(ply) * th;
                let s = if sign < 0.0 { [near - th, near] } else { [near, near + th] };
                out.members.push(Member::lying(
                    pre(&format!("{}/header/{end}/{ply}", h.key)),
                    (Role::Header, Grain::Across),
                    section,
                    1,
                    [s, h.a],
                    zs(s),
                ));
            }
        }
    }

    // Rows of blocking between neighbouring lines, dividing each span between what bears them evenly.
    if t.bridging > 0.0 {
        let mut order: Vec<usize> = (0..lines.len()).collect();
        order.sort_by(|a, b| {
            let c = |i: &usize| lines.get(*i).map_or(0.0, Line::centre);
            c(a).total_cmp(&c(b))
        });
        let pieces_of = |li: usize, s: [f64; 2]| placed.iter().filter(move |(l, p, _)| *l == li && overlap(*p, s)).map(|(_, _, a)| *a);
        let mut blocks: Vec<Member> = Vec::new();
        let mut touching = false;
        for pair in order.windows(2) {
            let &[l, r] = pair else { continue };
            let (Some(left), Some(right)) = (lines.get(l), lines.get(r)) else { continue };
            if right.across[0] - left.across[1] <= EPS {
                continue;
            }
            let mut row = 0;
            for p in spans.get(l).into_iter().flatten() {
                for q in spans.get(r).into_iter().flatten() {
                    let (lo, hi) = (p[0].max(q[0]), p[1].min(q[1]));
                    if hi - lo <= th {
                        continue;
                    }
                    let mut at: Vec<f64> =
                        supports.iter().filter_map(|sup| (g.bears)(sup, left.centre())).filter(|s| *s > lo && *s < hi).collect();
                    at.sort_by(f64::total_cmp);
                    let marks: Vec<f64> = std::iter::once(lo).chain(at).chain(std::iter::once(hi)).collect();
                    for w in marks.windows(2) {
                        let &[u, v] = w else { continue };
                        let rows = ((v - u) / t.bridging - EPS).ceil() as i64 - 1;
                        for k in 1..=rows {
                            let sr = u + (v - u) * k as f64 / (rows + 1) as f64;
                            let s = [sr - th / 2.0, sr + th / 2.0];
                            let al = pieces_of(l, s).map(|a| a[1]).fold(f64::NEG_INFINITY, f64::max);
                            let ar = pieces_of(r, s).map(|a| a[0]).fold(f64::INFINITY, f64::min);
                            if !ar.is_finite() || !al.is_finite() || ar - al <= EPS {
                                continue;
                            }
                            // Lines all but touching: no block between them.
                            if ar - al < 1.0 {
                                touching = true;
                                continue;
                            }
                            // Wholly on the region, clear of the rims and the holes.
                            if !cover(&grid_cuts, [al, ar]).iter().any(|c| c[0] <= s[0] + EPS && c[1] >= s[1] - EPS) {
                                continue;
                            }
                            blocks.push(Member::lying(
                                pre(&format!("blocking/{}/{row}", left.key.strip_prefix(g.prefix.as_str()).unwrap_or(&left.key))),
                                (Role::Blocking, Grain::Across),
                                section,
                                1,
                                [s, [al, ar]],
                                zs(s),
                            ));
                            row += 1;
                        }
                    }
                }
            }
        }
        out.members.extend(blocks);
        if touching {
            out.warnings.push(format!("{who}{} less than a millimetre apart get no blocking between them: space them wider", words.many));
        }
    }
    out.bearing = bearing;
    out.longest = longest;
    out.lines = lines.len();
    out
}
