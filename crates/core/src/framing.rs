//! Wood framing: a framed wall's plates, studs, kings, jacks, headers, sills, cripples and blocking, worked out from the
//! wall, its doors and windows, the walls it meets and its framing type whenever they are asked for, and never stored, as a
//! railing's balusters are ([`crate::stairs::railing_parts`]). Moving a window, changing a height, attaching the top to a roof
//! or changing the framing type changes the members with nothing to regenerate.
//!
//! The members stand in the wall type's structure layer (the thickest, where it has several): their depth across the wall is
//! that layer's thickness, so a 2x4 wall is one with an 89 mm structure layer. In the wall's own frame `s` runs along its
//! location line from its start, `across` to its left (as `walls::boundaries` measures) and `z` up, absolute. The framed
//! length is the structure layer's along the wall: at a free end, square; at a square corner of two walls with a structure
//! layer each, the wall of the lower GUID (the one whose compound core runs through, `walls/compound.rs`) runs to the other's
//! far structure face and the other stops at its near one, so the two never overlap or leave a gap; at any other join, where
//! the layer's shorter edge ends. A key names each member for as long as what it belongs to stays: grid studs by their
//! station from the layout's origin, an opening's members by its GUID.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::commands::bad;
use crate::engine::Session;
use crate::geom::{self, P2};
use crate::model::{Element, Function, Guid, Id, MAX_COORD, Model, TOL, Wall};
use crate::walls::{self, Axis, Plane, Prism};

/// The framing type `frame` makes when the project has none: 2x4 studs 16 inches apart.
pub const DEFAULT_TYPE: &str = "2x4 @ 406";
/// The most grid stations a framed wall may have: one longer than this many `spacing`s is refused.
pub const MAX_STATIONS: f64 = 1024.0;
const EPS: f64 = 1e-6;

/// Where a wall's grid of studs is measured from: its framed length's start, its end or its middle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Start,
    End,
    Centre,
}

/// What a wall that runs through an L corner adds: `three_stud`, a stud beside its end stud (the butting wall's end stud is
/// the third); `two_stud` (California), none; `none`, none either.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    #[default]
    ThreeStud,
    TwoStud,
    None,
}

/// What a wall adds where another butts into its side: `studs`, one each side of the other's structure layer, backing for
/// its end stud; or `none`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tee {
    #[default]
    Studs,
    None,
}

/// A header over a rough opening as wide as `up_to` mm or narrower (any, without it): `plies` pieces `depth` mm deep on
/// edge, side by side across the wall; or, `flat`, laid flat as wide as the structure layer, `plies` of them stacked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_to: Option<f64>,
    pub depth: f64,
    #[serde(default = "one")]
    pub plies: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flat: bool,
}

fn one() -> u32 {
    1
}

impl Header {
    /// How high it stands over the rough opening.
    fn height(&self) -> f64 {
        if self.flat { self.depth * f64::from(self.plies) } else { self.depth }
    }
}

/// A framing type: how a wall is framed (all mm). Every field has its default, so a type named alone is 2x4 framing 16
/// inches on centre for a bearing wall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FramingType {
    /// The grid's stud centres apart: 406.4 (16"), 609.6 (24").
    pub spacing: f64,
    pub layout: Layout,
    /// A stud's or a plate's thickness, a 2x's 38 (1.5").
    pub thickness: f64,
    /// What the members are made of: a material of the library; none, the structure layer's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    pub bottom_plates: u32,
    pub top_plates: u32,
    /// Each side of an opening: full-height studs, and the jacks (trimmers) under its header inside them.
    pub kings: u32,
    pub jacks: u32,
    /// What a window's and a door's rough opening adds to its width (half each side) and its height (on top).
    pub window_rough: [f64; 2],
    pub door_rough: [f64; 2],
    /// The first whose `up_to` holds the rough opening is its header (none does: the last).
    pub headers: Vec<Header>,
    /// Under a window's rough opening.
    pub sill_plates: u32,
    pub corner: Corner,
    pub tee: Tee,
    /// Heights over the wall's base of rows of flat blocking between full-height studs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocking: Vec<f64>,
    /// The stock lengths a cut list takes members from, shortest first.
    pub stock: Vec<f64>,
}

impl Default for FramingType {
    fn default() -> Self {
        FramingType {
            spacing: 406.4,
            layout: Layout::Start,
            thickness: 38.0,
            material: None,
            bottom_plates: 1,
            top_plates: 2,
            kings: 1,
            jacks: 1,
            window_rough: [25.0, 13.0],
            door_rough: [50.0, 41.0],
            headers: vec![
                Header { up_to: Some(1250.0), depth: 184.0, plies: 2, flat: false },
                Header { up_to: None, depth: 235.0, plies: 2, flat: false },
            ],
            sill_plates: 1,
            corner: Corner::ThreeStud,
            tee: Tee::Studs,
            blocking: Vec::new(),
            stock: vec![2440.0, 3050.0, 3660.0, 4270.0, 4880.0, 6100.0],
        }
    }
}

/// What a member is, the word schedules and IFC name it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    BottomPlate,
    TopPlate,
    Stud,
    King,
    Jack,
    Cripple,
    Header,
    Sill,
    Blocking,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::BottomPlate => "bottom_plate",
            Role::TopPlate => "top_plate",
            Role::Stud => "stud",
            Role::King => "king",
            Role::Jack => "jack",
            Role::Cripple => "cripple",
            Role::Header => "header",
            Role::Sill => "sill",
            Role::Blocking => "blocking",
        }
    }

    /// Whether it stands upright: its cut length is up it, not along the wall.
    pub fn upright(self) -> bool {
        matches!(self, Role::Stud | Role::King | Role::Jack | Role::Cripple)
    }
}

/// One member, in its host's frame: from `s[0]` to `s[1]` along it and `across[0]` to `across[1]` across it, its plumb
/// sides rising from a bottom at `bottom[0]` (at `s[0]`) and `bottom[1]` (at `s[1]`) to a top at `top[0]` and `top[1]`,
/// each straight between (a bevelled stud's top, a sloped plate).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Member {
    pub key: String,
    pub role: Role,
    /// One ply's section as sawn: [38, 89] for a 2x4, [38, 184] for each ply of a 2-2x8 header.
    pub section: [f64; 2],
    pub plies: u32,
    /// Its cut length: up an upright one's longest edge, along a lying one's.
    pub length: f64,
    pub s: [f64; 2],
    pub across: [f64; 2],
    pub bottom: [f64; 2],
    pub top: [f64; 2],
}

impl Member {
    /// Where a level plane at height `z` passes through it: from where to where along its host. None where it doesn't.
    pub fn cut_at(&self, z: f64) -> Option<[f64; 2]> {
        // Where a straight height from `a` (at 0) to `b` (at 1) lies under `z`, as fractions of the way.
        let under = |a: f64, b: f64| match (a < z, b < z) {
            (true, true) => Some([0.0, 1.0]),
            (false, false) => None,
            (first, _) => {
                let t = (z - a) / (b - a);
                Some(if first { [0.0, t] } else { [t, 1.0] })
            }
        };
        let [b0, b1] = under(self.bottom[0], self.bottom[1])?;
        // Its top over `z` is that top mirrored about `z` under it.
        let [t0, t1] = under(2.0 * z - self.top[0], 2.0 * z - self.top[1])?;
        let (from, to) = (b0.max(t0), b1.min(t1));
        let along = |t: f64| self.s[0] + (self.s[1] - self.s[0]) * t;
        (to - from > EPS).then(|| [along(from), along(to)])
    }

    /// From `z`: its bottom and its top.
    fn new(key: String, role: Role, section: [f64; 2], plies: u32, s: [f64; 2], across: [f64; 2], z: [[f64; 2]; 2]) -> Self {
        let [bottom, top] = z;
        let length = if role.upright() {
            (top[0] - bottom[0]).max(top[1] - bottom[1])
        } else {
            let run = s[1] - s[0];
            run.hypot(top[1] - top[0]).max(run.hypot(bottom[1] - bottom[0]))
        };
        Member { key, role, section, plies, length, s, across, bottom, top }
    }
}

/// A host's members, and the frame they're in.
#[derive(Clone, Debug, PartialEq)]
pub struct Framing {
    pub host: Guid,
    pub type_name: String,
    /// The structure layer they stand in.
    pub layer: Guid,
    /// What they're made of: the framing type's material, else the layer's.
    pub material: String,
    /// `s` 0 and `across` 0: the start of the wall's location line.
    pub origin: P2,
    pub x_axis: P2,
    /// The framing type's stock lengths.
    pub stock: Vec<f64>,
    pub members: Vec<Member>,
}

/// A line of a cut list: `count` members alike.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Cut {
    pub role: Role,
    pub section: [f64; 2],
    pub plies: u32,
    /// To the millimetre.
    pub length: f64,
    pub stock: Option<f64>,
    pub count: usize,
}

impl Framing {
    /// The way `across` runs: to the left of `x_axis`.
    pub fn depth_axis(&self) -> P2 {
        self.x_axis.perp()
    }

    /// The point `s` along the host and `across` to its left, in plan.
    pub fn at(&self, s: f64, across: f64) -> P2 {
        self.origin + self.x_axis * s + self.depth_axis() * across
    }

    /// Member `m`'s solid: its plan rectangle, counter-clockwise, between its bottom and top planes.
    pub fn prism(&self, m: &Member) -> Prism {
        let plane = |z: [f64; 2]| {
            let k = (z[1] - z[0]) / (m.s[1] - m.s[0]).max(EPS);
            let g = self.x_axis * k;
            Plane { z: z[0] - k * m.s[0] - g.dot(self.origin), g }
        };
        let [s0, s1] = m.s;
        let [a0, a1] = m.across;
        let ring = vec![self.at(s0, a0), self.at(s1, a0), self.at(s1, a1), self.at(s0, a1)];
        Prism { ring, bottom: plane(m.bottom), top: plane(m.top), curve: None }
    }

    /// The members alike by role, section, plies and length to the millimetre, each with the stock length it is cut from.
    pub fn cut_list(&self) -> Vec<Cut> {
        let mut rows: BTreeMap<(Role, [i64; 2], u32, i64), usize> = BTreeMap::new();
        for m in &self.members {
            let mm = |x: f64| x.round() as i64;
            *rows.entry((m.role, [mm(m.section[0]), mm(m.section[1])], m.plies, mm(m.length))).or_default() += 1;
        }
        rows.into_iter()
            .map(|((role, section, plies, length), count)| {
                let length = length as f64;
                Cut { role, section: section.map(|x| x as f64), plies, length, stock: stock(length, &self.stock), count }
            })
            .collect()
    }
}

/// The shortest of `stock` a member `length` long is cut from; none when it is longer than all of them (spliced).
pub fn stock(length: f64, stock: &[f64]) -> Option<f64> {
    stock.iter().copied().find(|s| *s >= length - EPS)
}

/// The thickest structure layer of wall `w`'s type (the first of the thickest): its index and where it lies across the
/// wall, `[interior side, exterior side]` to the left of its location line.
pub(crate) fn structure(m: &Model, w: &Wall) -> Option<(usize, [f64; 2])> {
    let t = m.types.wall.get(&w.type_name)?;
    let (k, _) = t
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.function == Function::Structure)
        .max_by(|(i, a), (j, b)| a.thickness.total_cmp(&b.thickness).then(j.cmp(i)))?;
    let b = walls::boundaries(m, w);
    Some((k, [*b.get(k + 1)?, *b.get(k)?]))
}

/// The material wall `w`'s framing type names for its members, if it names one: else they're its structure layer's.
pub(crate) fn material<'a>(m: &'a Model, w: &Wall) -> Option<&'a str> {
    m.types.framing.get(w.framing.as_ref()?)?.material.as_deref()
}

/// A framing type's own rules, which `Model::check` asks of each.
pub(crate) fn check_type(m: &Model, name: &str, t: &FramingType) -> Result<(), String> {
    let who = format!("framing type `{name}`");
    let within = |what: &str, x: f64, lo: f64, hi: f64| {
        if x.is_finite() && (lo..=hi).contains(&x) { Ok(()) } else { Err(format!("{who}: {what} {x} isn't between {lo} and {hi}")) }
    };
    let count = |what: &str, n: u32, lo: u32, hi: u32| {
        if (lo..=hi).contains(&n) { Ok(()) } else { Err(format!("{who}: {n} {what}; it has {lo} to {hi}")) }
    };
    within("thickness (mm)", t.thickness, 10.0, 300.0)?;
    within("spacing (mm)", t.spacing, 0.0, 3000.0)?;
    if t.spacing <= t.thickness {
        return Err(format!("{who}: its studs {} mm apart must be farther apart than they are thick ({} mm)", t.spacing, t.thickness));
    }
    count("bottom plates", t.bottom_plates, 0, 4)?;
    count("top plates", t.top_plates, 0, 4)?;
    count("sill plates", t.sill_plates, 0, 4)?;
    count("kings", t.kings, 0, 3)?;
    count("jacks", t.jacks, 0, 3)?;
    for (what, [w, h]) in [("window_rough", t.window_rough), ("door_rough", t.door_rough)] {
        within(&format!("{what} width (mm)"), w, 0.0, 500.0)?;
        within(&format!("{what} height (mm)"), h, 0.0, 500.0)?;
    }
    if t.headers.is_empty() || t.headers.len() > 8 {
        return Err(format!("{who}: {} headers; it has 1 to 8", t.headers.len()));
    }
    for (i, h) in t.headers.iter().enumerate() {
        within(&format!("header {i} depth (mm)"), h.depth, 1.0, 1000.0)?;
        count(&format!("plies in header {i}"), h.plies, 1, 3)?;
        if let Some(u) = h.up_to {
            within(&format!("header {i} up_to (mm)"), u, 0.0, MAX_COORD)?;
        }
    }
    if t.blocking.len() > 8 {
        return Err(format!("{who}: {} rows of blocking; it has 8 at most", t.blocking.len()));
    }
    for &h in &t.blocking {
        within("blocking height (mm)", h, 0.0, 100_000.0)?;
    }
    if t.stock.len() > 16 {
        return Err(format!("{who}: {} stock lengths; it has 16 at most", t.stock.len()));
    }
    for &s in &t.stock {
        within("stock length (mm)", s, 1.0, 100_000.0)?;
    }
    if t.stock.windows(2).any(|w| w[1] <= w[0]) {
        return Err(format!("{who}: its stock lengths go shortest first, each longer than the last"));
    }
    if let Some(mat) = &t.material
        && !m.materials.contains_key(mat)
    {
        return Err(format!("{who}: material `{mat}` isn't in the library; add it with `material`"));
    }
    Ok(())
}

/// What a framed wall must be, which `Model::check` asks of each: straight, of a type with a structure layer its headers fit,
/// of a known framing type, and no longer than [`MAX_STATIONS`] studs. Nothing an opening or a join does is refused: what
/// doesn't fit is left unframed.
pub(crate) fn check(m: &Model, w: &Wall) -> Result<(), String> {
    let Some(name) = &w.framing else { return Ok(()) };
    let t = m.types.framing.get(name).ok_or_else(|| {
        let names: Vec<&str> = m.types.framing.keys().map(String::as_str).collect();
        format!("framing type `{name}` doesn't exist (there are: {}); make it with `type` {{category: framing, name}}", names.join(", "))
    })?;
    if w.arc != 0.0 {
        return Err(format!("only a straight wall is framed, and this one turns {}°: straighten it or `unframe` it", w.arc));
    }
    let (_, [lo, hi]) = structure(m, w).ok_or_else(|| {
        format!(
            "its type `{}` has no structure layer to frame: give the type a layer of function structure (`type` {{category: wall, \
             layers}}) or `unframe` it",
            w.type_name
        )
    })?;
    let length = Axis::of(w).length();
    if length / t.spacing > MAX_STATIONS {
        return Err(format!(
            "framing type `{name}` puts more than {MAX_STATIONS} studs along its {length:.0} mm; frame a shorter wall or space them wider"
        ));
    }
    let depth = hi - lo;
    if let Some(h) = t.headers.iter().find(|h| !h.flat && f64::from(h.plies) * t.thickness > depth + EPS) {
        return Err(format!(
            "framing type `{name}`'s header of {} plies {} mm thick is wider than the {depth} mm structure layer of wall type `{}`: \
             use fewer plies or a deeper layer",
            h.plies, t.thickness, w.type_name
        ));
    }
    Ok(())
}

/// A wall's base or top along its structure layer: straight pieces `[from, to, z at from, z at to]`.
struct Profile(Vec<[f64; 4]>);

impl Profile {
    /// Its height `s` along the wall, and how steeply it rises there.
    fn at(&self, s: f64) -> (f64, f64) {
        let Some(p) = self.0.iter().find(|p| s <= p[1] + EPS).or(self.0.last()) else { return (0.0, 0.0) };
        let k = (p[3] - p[2]) / (p[1] - p[0]).max(EPS);
        (p[2] + k * (s - p[0]), k)
    }

    /// Where its pieces meet between `s0` and `s1`, and those two.
    fn stations(&self, s0: f64, s1: f64) -> impl Iterator<Item = f64> + '_ {
        [s0, s1].into_iter().chain(self.0.iter().map(|p| p[0]).filter(move |s| *s > s0 && *s < s1))
    }
}

/// An opening's rough opening, in the wall's frame.
struct Rough {
    guid: Guid,
    door: bool,
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
}

/// Where one of this wall's ends is framed to, and whether it runs through an L corner there.
fn end(m: &Model, id: Id, w: &Wall, k: usize, mid: f64, at_end: bool) -> (f64, bool) {
    let axis = Axis::of(w);
    let u = axis.tangent(0.0);
    let along = |q: P2| (q - w.start).dot(u);
    let (others, tee) = walls::joined(m, id, at_end);
    if let ([other], false) = (others.as_slice(), tee)
        && let Some(o) = m.wall(*other).filter(|o| o.arc == 0.0)
        && let Some((_, band)) = structure(m, o)
    {
        let uo = (o.end - o.start).unit();
        if u.dot(uo).abs() <= 0.05 {
            let p = axis.at(0.0, mid);
            let faces: Vec<f64> = band
                .iter()
                .filter_map(|off| {
                    let q = o.start + uo.perp() * *off;
                    geom::line_intersection(p, p + u, q, q + uo).map(along)
                })
                .collect();
            if let [x, y] = faces[..] {
                let through = m.guid(id) < m.guid(*other);
                // The far face is the one past the corner, away from this wall's body.
                let (far, near) = if at_end { (x.max(y), x.min(y)) } else { (x.min(y), x.max(y)) };
                return (if through { far } else { near }, through);
            }
        }
    }
    // Elsewhere, where the structure layer's shorter edge ends.
    let lines = walls::layer_geometry(m, id, None).lines;
    let ends = [k, k + 1].map(|i| {
        lines.get(i).and_then(|l| if at_end { l.last() } else { l.first() }).map_or(if at_end { axis.length() } else { 0.0 }, |q| along(*q))
    });
    (if at_end { ends[0].min(ends[1]) } else { ends[0].max(ends[1]) }, false)
}

/// The walls ending on this one's side, each with where its structure layer (else its whole thickness) crosses this
/// wall's: by their GUIDs, `[from, to]` along it.
fn tees(m: &Model, id: Id, w: &Wall, mid: f64) -> Vec<(Guid, [f64; 2])> {
    let axis = Axis::of(w);
    let (u, len) = (axis.tangent(0.0), axis.length());
    let (ext, int) = walls::faces(m, w);
    let Some(reach) = walls::reach(m, w).filter(|_| w.join) else { return Vec::new() };
    let widest = m.types.wall.values().map(|t| t.thickness).filter(|t| t.is_finite()).fold(0.0, f64::max);
    let (lo, hi) = walls::heights(m, w);
    let mut out = Vec::new();
    for other in m.lookup().walls_near_at(&[(reach, lo, hi)], widest + TOL) {
        let Some(o) = m.wall(other).filter(|o| other != id && o.join && o.arc == 0.0 && walls::stacked(m, w, o)) else { continue };
        let on_side = |node: P2| {
            let (s, off) = axis.project(node);
            s > TOL && s < len - TOL && (int - TOL..=ext + TOL).contains(&off) && axis.distance(node, off) <= TOL
        };
        if !(on_side(o.start) || on_side(o.end)) {
            continue;
        }
        let uo = (o.end - o.start).unit();
        let band = structure(m, o).map_or_else(|| <[f64; 2]>::from(walls::faces(m, o)), |(_, b)| b);
        let p = axis.at(0.0, mid);
        let cross: Vec<f64> = band
            .iter()
            .filter_map(|off| {
                let q = o.start + uo.perp() * *off;
                geom::line_intersection(p, p + u, q, q + uo).map(|c| (c - w.start).dot(u))
            })
            .collect();
        if let [x, y] = cross[..] {
            out.push((m.guid(other), [x.min(y), x.max(y)]));
        }
    }
    out.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]).then(a.0.cmp(&b.0)));
    out
}

/// Wall `id`'s framing: none when it isn't a framed wall; an error when it can't be framed, as `Model::check` says.
pub fn wall(m: &Model, id: Id) -> Result<Option<Framing>, String> {
    let Some(w) = m.wall(id) else { return Ok(None) };
    let Some(name) = &w.framing else { return Ok(None) };
    check(m, w)?;
    let (Some(t), Some((k, across))) = (m.types.framing.get(name), structure(m, w)) else { return Ok(None) };
    let layer = m.types.wall.get(&w.type_name).and_then(|wt| wt.layers.get(k));
    let axis = Axis::of(w);
    let mid = (across[0] + across[1]) / 2.0;
    let mut out = Framing {
        host: m.guid(id),
        type_name: name.clone(),
        layer: layer.map(|l| l.guid).unwrap_or_default(),
        material: t.material.clone().or_else(|| layer.map(|l| l.material.clone())).unwrap_or_default(),
        origin: w.start,
        x_axis: axis.tangent(0.0),
        stock: t.stock.clone(),
        members: Vec::new(),
    };
    let ((a, through_start), (b, through_end)) = (end(m, id, w, k, mid, false), end(m, id, w, k, mid, true));
    if b - a <= EPS {
        return Ok(Some(out));
    }
    let th = t.thickness;
    let depth = across[1] - across[0];
    let section = [th, depth];
    let base = Profile(walls::profile(m, w, false, mid, a, b));
    let top = Profile(walls::profile(m, w, true, mid, a, b));
    // A plate's plumb height on a slope, so that it is `th` thick square to it.
    let plumb = |k: f64| th * k.hypot(1.0);
    // Where the studs stand on the bottom plates and stop under the top ones.
    let floor = |s: f64| {
        let (z, k) = base.at(s);
        z + f64::from(t.bottom_plates) * plumb(k)
    };
    let ceiling = |s: f64| {
        let (z, k) = top.at(s);
        z - f64::from(t.top_plates) * plumb(k)
    };
    let highest_floor = |s0: f64, s1: f64| base.stations(s0, s1).map(floor).fold(f64::NEG_INFINITY, f64::max);
    let lowest_ceiling = |s0: f64, s1: f64| top.stations(s0, s1).map(ceiling).fold(f64::INFINITY, f64::min);
    let own_base = m.own_base(w);
    let members = &mut out.members;

    // The openings' rough openings, along the wall.
    let guid = m.guid(id);
    let mut roughs: Vec<Rough> = m
        .lookup()
        .referrers(&guid)
        .filter_map(|o| {
            let e = m.elements.get(o)?;
            let (Element::Door(op) | Element::Window(op)) = e else { return None };
            if op.host != guid {
                return None;
            }
            let dims = m.opening_dims(e).ok()?;
            let door = matches!(e, Element::Door(_));
            let [rw, rh] = if door { t.door_rough } else { t.window_rough };
            let half = (dims.width + rw) / 2.0;
            let sill = own_base + op.sill;
            Some(Rough {
                guid: m.guid(o),
                door,
                left: op.offset - half,
                right: op.offset + half,
                bottom: if door { own_base } else { sill },
                top: sill + dims.height + rh,
            })
        })
        .collect();
    roughs.sort_by(|x, y| x.left.total_cmp(&y.left).then(x.guid.cmp(&y.guid)));

    // The plates: the bottom ones cut across each door, each piece named by what it starts after; the top ones in a piece
    // for each slope of the top.
    let mut pieces: Vec<(String, f64, f64)> = Vec::new();
    let (mut from, mut after) = (a, "start".to_string());
    for r in roughs.iter().filter(|r| r.door && r.right > a && r.left < b) {
        if r.left - from > EPS {
            pieces.push((after.clone(), from, r.left));
        }
        if r.right > from {
            (from, after) = (r.right, r.guid.to_string());
        }
    }
    if b - from > EPS {
        pieces.push((after, from, b));
    }
    for (after, p0, p1) in pieces {
        let parts = base.0.iter().filter(|q| q[1] > p0 + EPS && q[0] < p1 - EPS);
        for (j, q) in parts.enumerate() {
            let (s0, s1) = (q[0].max(p0), q[1].min(p1));
            let k = (q[3] - q[2]) / (q[1] - q[0]).max(EPS);
            for ply in 0..t.bottom_plates {
                let bottom = [s0, s1].map(|s| q[2] + k * (s - q[0]) + f64::from(ply) * plumb(k));
                let key = format!("bottom_plate/{ply}/{after}/{j}");
                members.push(Member::new(key, Role::BottomPlate, section, 1, [s0, s1], across, [bottom, bottom.map(|z| z + plumb(k))]));
            }
        }
    }
    for (i, q) in top.0.iter().enumerate() {
        let k = (q[3] - q[2]) / (q[1] - q[0]).max(EPS);
        for ply in 0..t.top_plates {
            let drop = f64::from(ply + 1) * plumb(k);
            let bottom = [q[2] - drop, q[3] - drop];
            // A wall too low for its plates keeps those that clear the bottom ones.
            if bottom[0] < floor(q[0]) - EPS || bottom[1] < floor(q[1]) - EPS {
                continue;
            }
            let key = format!("top_plate/{ply}/{i}");
            members.push(Member::new(key, Role::TopPlate, section, 1, [q[0], q[1]], across, [bottom, bottom.map(|z| z + plumb(k))]));
        }
    }

    // An upright member `c` along, from `lo` up to `hi` at each of its sides; none shorter than it is thick.
    let upright = |members: &mut Vec<Member>, key: String, role: Role, c: f64, lo: &dyn Fn(f64) -> f64, hi: &dyn Fn(f64) -> f64| {
        let s = [c - th / 2.0, c + th / 2.0];
        let (bottom, top) = (s.map(lo), s.map(hi));
        if top[0] - bottom[0] >= th && top[1] - bottom[1] >= th {
            members.push(Member::new(key, role, section, 1, s, across, [bottom, top]));
        }
    };
    // The full-height studs, which blocking runs between, and every upright the grid must keep clear of.
    let mut full: Vec<(f64, String)> = Vec::new();
    let mut taken: Vec<f64> = Vec::new();
    let clear = |taken: &[f64], c: f64| taken.iter().all(|v| (c - v).abs() >= th - EPS);
    let stud = |members: &mut Vec<Member>, full: &mut Vec<(f64, String)>, taken: &mut Vec<f64>, key: String, c: f64| {
        upright(members, key.clone(), Role::Stud, c, &floor, &ceiling);
        full.push((c, key));
        taken.push(c);
    };
    if b - a >= 2.0 * th {
        stud(members, &mut full, &mut taken, "stud/start".into(), a + th / 2.0);
        stud(members, &mut full, &mut taken, "stud/end".into(), b - th / 2.0);
    } else if b - a >= th {
        stud(members, &mut full, &mut taken, "stud/start".into(), a + th / 2.0);
    }
    if t.corner == Corner::ThreeStud {
        for (through, c, key) in [(through_start, a + 1.5 * th, "corner/start"), (through_end, b - 1.5 * th, "corner/end")] {
            if through && c >= a + th / 2.0 && c <= b - th / 2.0 && clear(&taken, c) {
                stud(members, &mut full, &mut taken, key.into(), c);
            }
        }
    }

    // Each opening that fits, the first along the wall first: kings, jacks, a header on the jacks and a window's sill.
    let (kings, jacks) = (f64::from(t.kings), f64::from(t.jacks));
    let mut framed: Vec<(&Rough, [f64; 2], &Header)> = Vec::new();
    for r in &roughs {
        let span = [r.left - (jacks + kings) * th, r.right + (jacks + kings) * th];
        let width = r.right - r.left;
        let Some(header) = t.headers.iter().find(|h| h.up_to.is_none_or(|u| u >= width - EPS)).or(t.headers.last()) else { continue };
        let ends = [r.left - jacks * th, r.right + jacks * th];
        let sill_under = r.bottom - f64::from(t.sill_plates) * th;
        let fits = span[0] >= a - EPS
            && span[1] <= b + EPS
            && taken.iter().all(|v| v + th / 2.0 <= span[0] + EPS || v - th / 2.0 >= span[1] - EPS)
            && framed.iter().all(|(_, o, _)| o[1] <= span[0] + EPS || o[0] >= span[1] - EPS)
            && r.top + header.height() <= lowest_ceiling(ends[0], ends[1]) + EPS
            && r.top > highest_floor(ends[0], ends[1]) + EPS
            && (r.door || sill_under >= highest_floor(r.left, r.right) - EPS);
        if !fits {
            continue;
        }
        let g = r.guid;
        for (side, sign, edge) in [("left", -1.0, r.left), ("right", 1.0, r.right)] {
            for i in 0..t.kings {
                let c = edge + sign * ((jacks + f64::from(i) + 0.5) * th);
                let key = format!("{g}/king/{side}/{i}");
                upright(members, key.clone(), Role::King, c, &floor, &ceiling);
                full.push((c, key));
            }
            for i in 0..t.jacks {
                let c = edge + sign * ((f64::from(i) + 0.5) * th);
                upright(members, format!("{g}/jack/{side}/{i}"), Role::Jack, c, &floor, &|_| r.top);
            }
        }
        let (h_across, h_section) = if header.flat {
            (across, [header.depth, depth])
        } else {
            let half = f64::from(header.plies) * th / 2.0;
            ([mid - half, mid + half], [th, header.depth])
        };
        let (z0, z1) = (r.top, r.top + header.height());
        members.push(Member::new(format!("{g}/header"), Role::Header, h_section, header.plies, ends, h_across, [[z0; 2], [z1; 2]]));
        if !r.door {
            for i in 0..t.sill_plates {
                let z = r.bottom - f64::from(i) * th;
                members.push(Member::new(
                    format!("{g}/sill/{i}"),
                    Role::Sill,
                    section,
                    1,
                    [r.left, r.right],
                    across,
                    [[z - th; 2], [z; 2]],
                ));
            }
        }
        taken.extend((0..t.kings + t.jacks).flat_map(|i| {
            let d = (f64::from(i) + 0.5) * th;
            [r.left - d, r.right + d]
        }));
        framed.push((r, span, header));
    }
    // Where the grid can't stand a stud: an opening not framed, in its rough opening.
    let in_rough = |c: f64| roughs.iter().any(|r| c + th / 2.0 > r.left + EPS && c - th / 2.0 < r.right - EPS);

    // Backing each side of a wall that butts into this one's side.
    if t.tee == Tee::Studs {
        for (other, [p0, p1]) in tees(m, id, w, mid) {
            for (j, c) in [(0, p0 - th / 2.0), (1, p1 + th / 2.0)] {
                let inside = c >= a + th / 2.0 - EPS && c <= b - th / 2.0 + EPS;
                let in_span = framed.iter().any(|(_, s, _)| c + th / 2.0 > s[0] + EPS && c - th / 2.0 < s[1] - EPS);
                if inside && !in_span && !in_rough(c) && clear(&taken, c) {
                    stud(members, &mut full, &mut taken, format!("tee/{other}/{j}"), c);
                }
            }
        }
    }

    // The grid: a stud at each station clear of the others, and inside a framed rough opening a cripple over its header
    // and, a window's, one under its sill.
    let length = b - a;
    let n = (length / t.spacing + EPS).floor() as i64;
    let stations: Box<dyn Iterator<Item = (i64, f64)>> = match t.layout {
        Layout::Start => Box::new((0..=n).map(|i| (i, a + i as f64 * t.spacing))),
        Layout::End => Box::new((0..=n).map(|i| (i, b - i as f64 * t.spacing))),
        Layout::Centre => {
            let half = (length / 2.0 / t.spacing + EPS).floor() as i64;
            Box::new((-half..=half).map(|i| (i, (a + b) / 2.0 + i as f64 * t.spacing)))
        }
    };
    let mut grid: Vec<(i64, f64)> = stations.filter(|(_, c)| *c >= a + th / 2.0 - EPS && *c <= b - th / 2.0 + EPS).collect();
    grid.sort_by(|x, y| x.1.total_cmp(&y.1));
    for (i, c) in grid {
        if let Some((r, _, header)) = framed.iter().find(|(_, s, _)| c > s[0] - th / 2.0 && c < s[1] + th / 2.0) {
            if c >= r.left + th / 2.0 - EPS && c <= r.right - th / 2.0 + EPS {
                let over = r.top + header.height();
                upright(members, format!("{}/cripple/top/{i}", r.guid), Role::Cripple, c, &|_| over, &ceiling);
                if !r.door {
                    let under = r.bottom - f64::from(t.sill_plates) * th;
                    upright(members, format!("{}/cripple/bottom/{i}", r.guid), Role::Cripple, c, &floor, &|_| under);
                }
            }
            continue;
        }
        if !in_rough(c) && clear(&taken, c) {
            stud(members, &mut full, &mut taken, format!("stud/{i}"), c);
        }
    }

    // Rows of flat blocking between neighbouring full-height studs, none across a rough opening.
    full.sort_by(|x, y| x.0.total_cmp(&y.0));
    for (row, h) in t.blocking.iter().enumerate() {
        let (z0, z1) = (own_base + h - th / 2.0, own_base + h + th / 2.0);
        for pair in full.windows(2) {
            let [(l, key), (r, _)] = pair else { continue };
            let (s0, s1) = (l + th / 2.0, r - th / 2.0);
            if s1 - s0 <= EPS || roughs.iter().any(|o| s0 < o.right && s1 > o.left) {
                continue;
            }
            if z0 < highest_floor(s0, s1) - EPS || z1 > lowest_ceiling(s0, s1) + EPS {
                continue;
            }
            members.push(Member::new(format!("blocking/{row}/{key}"), Role::Blocking, section, 1, [s0, s1], across, [[z0; 2], [z1; 2]]));
        }
    }
    Ok(Some(out))
}

/// To the hundredth of a millimetre.
fn mm(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn member_json(f: &Framing, m: &Member) -> Value {
    json!({
        "key": m.key, "role": m.role, "section": m.section, "plies": m.plies, "length": mm(m.length),
        "stock": stock(m.length, &f.stock), "s": m.s.map(mm), "across": m.across.map(mm),
        "z": [mm(m.bottom[0]), mm(m.bottom[1]), mm(m.top[0]), mm(m.top[1])],
    })
}

/// The walls `ids` names: refused for anything else.
fn walls_of(c: &str, m: &Model, p: &Value) -> Result<Vec<Id>, String> {
    let ids = crate::commands::ids_param(c, m, p)?;
    if let Some((id, e)) = ids.iter().filter_map(|id| Some((*id, m.elements.get(*id)?))).find(|(_, e)| !matches!(e, Element::Wall(_))) {
        return Err(bad(c, format!("{} {id} isn't a wall: only walls are framed", e.category())));
    }
    Ok(ids)
}

/// `frame`: walls given a framing type, the project's default one (made, if it has none) unless `type` names another.
pub(crate) fn frame(s: &mut Session, p: &Value) -> Result<Value, String> {
    let c = "frame";
    crate::commands::only(c, p, &["id", "ids", "type"])?;
    let ids = walls_of(c, &s.model, p)?;
    let given = crate::commands::string(c, p, "type")?;
    if let Some(n) = given.as_ref().filter(|n| !s.model.types.framing.contains_key(*n)) {
        let names: Vec<&str> = s.model.types.framing.keys().map(String::as_str).collect();
        return Err(bad(
            c,
            format!("there is no framing type `{n}` (there are: {}); make it with `type` {{category: framing, name}}", names.join(", ")),
        ));
    }
    let m = s.model_mut();
    if given.is_none() && m.types.framing.is_empty() {
        m.types.framing.insert(DEFAULT_TYPE.into(), FramingType::default());
    }
    let name = given.or_else(|| m.default_type("framing").map(String::from)).unwrap_or_else(|| DEFAULT_TYPE.into());
    for id in &ids {
        if let Some(Element::Wall(w)) = m.elements.get_mut(*id) {
            w.framing = Some(name.clone());
        }
    }
    let mut total = 0;
    for id in &ids {
        total += wall(m, *id).map_err(|e| bad(c, format!("wall {id}: {e}")))?.map_or(0, |f| f.members.len());
    }
    Ok(json!({"ids": ids, "type": name, "members": total}))
}

/// `unframe`: walls framed no more.
pub(crate) fn unframe(s: &mut Session, p: &Value) -> Result<Value, String> {
    let c = "unframe";
    crate::commands::only(c, p, &["id", "ids"])?;
    let ids = walls_of(c, &s.model, p)?;
    let m = s.model_mut();
    for id in &ids {
        if let Some(Element::Wall(w)) = m.elements.get_mut(*id) {
            w.framing = None;
        }
    }
    Ok(json!({"ids": ids}))
}

/// `framing`: a framed host's members and its cut list.
pub(crate) fn query(s: &mut Session, p: &Value) -> Result<Value, String> {
    let c = "framing";
    crate::commands::only(c, p, &["id"])?;
    let m = &s.model;
    let ids = walls_of(c, m, p)?;
    let [id] = ids[..] else { return Err(bad(c, "`id`, one wall, is required")) };
    let f =
        wall(m, id).map_err(|e| bad(c, format!("wall {id}: {e}")))?.ok_or_else(|| bad(c, format!("wall {id} isn't framed: `frame` it")))?;
    let cuts: Vec<Value> = f
        .cut_list()
        .into_iter()
        .map(|r| json!({"role": r.role, "section": r.section, "plies": r.plies, "length": r.length, "stock": r.stock, "count": r.count}))
        .collect();
    Ok(json!({
        "units": "mm", "host": f.host, "id": id, "type": f.type_name, "layer": f.layer, "material": f.material,
        "origin": [f.origin.0, f.origin.1], "x_axis": [f.x_axis.0, f.x_axis.1], "depth_axis": [f.depth_axis().0, f.depth_axis().1],
        "members": f.members.iter().map(|m| member_json(&f, m)).collect::<Vec<_>>(), "cut_list": cuts,
    }))
}
