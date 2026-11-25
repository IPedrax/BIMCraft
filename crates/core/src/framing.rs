//! Wood framing: a framed wall's plates, studs, kings, jacks, headers, sills, cripples and blocking, worked out from the
//! wall, its doors and windows, the walls it meets and its framing type whenever they are asked for, and never stored, as a
//! railing's balusters are ([`crate::stairs::railing_parts`]). Moving a window, changing a height, attaching the top to a roof
//! or changing the framing type changes the members with nothing to regenerate. A framed floor's joists, rims, trimmers,
//! headers and blocking are worked out the same way ([`floor`]), and a framed roof's rafters, ridge boards, rims, trimmers,
//! headers and blocking slope by slope by the floor's rules ([`roof`]), all in the same [`Member`]s and [`Framing`].
//!
//! The members stand in the wall type's structure layer (the thickest, where it has several): their depth across the wall is
//! that layer's thickness, so a 2x4 wall is one with an 89 mm structure layer. In the wall's own frame `s` runs along its
//! location line from its start, `across` to its left (as `walls::boundaries` measures) and `z` up, absolute. The framed
//! length is the structure layer's along the wall: at a free end, square; at a square corner of two walls with a structure
//! layer each, the wall of the lower GUID (the one whose compound core runs through, `walls/compound.rs`) runs to the other's
//! far structure face and the other stops at its near one, so the two never overlap or leave a gap; butting into another's
//! side, at that one's near structure face; at any other join, where the layer's shorter edge ends. Where two framed walls
//! with double top plates meet square at one height, the upper plates lap (`laps`). A key names each member for as long as
//! what it belongs to stays: grid studs by their station from the layout's origin, an opening's members by its GUID.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::commands::bad;
use crate::engine::Session;
use crate::geom::{self, P2};
use crate::model::{Element, Function, Guid, Id, MAX_COORD, Model, TOL, Wall};
use crate::walls::{self, Axis, Plane, Prism};

pub mod floor;
pub mod roof;

/// The framing type `frame` makes when the project has none: 2x4 studs 16 inches apart.
pub const DEFAULT_TYPE: &str = "2x4 @ 406";
/// The most grid stations a framed wall may have: one longer than this many `spacing`s is refused.
pub const MAX_STATIONS: f64 = 1024.0;
/// How far apart, mm, the splices of two top plates are at least, as IRC R602.3.2 has a double top plate's end joints offset
/// (24 in).
pub const TOP_SPLICES: f64 = 610.0;
/// The shortest cripple, in thicknesses (114 mm for a 2x, 4 1/2 in): one shorter splits as it is toe-nailed at both ends, so a
/// gap over a header or under a sill shorter than this all along is filled with flat blocking instead.
const CRIPPLE: f64 = 3.0;
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
    /// A floor's: a rim joist along each edge of it its joists end on; a roof's, a rim (sub-fascia) along each eave its
    /// rafters end on.
    pub rim: bool,
    /// A floor's or a roof's: how far each piece of a joist or a rafter runs past the centre line of a support it is cut
    /// over, its pieces then side by side, every other one a thickness over; 0, they butt on it.
    pub lap: f64,
    /// A floor's: the longest a joist runs between rows of solid blocking, the rows dividing each span evenly; 0, none.
    pub bridging: f64,
    /// A floor's: plies of trimmer each side of an opening and of header across each end of it.
    pub trimmers: u32,
    /// A floor's: a pair of joists under each wall standing on it along its joists.
    pub under_partitions: bool,
    /// A roof's: a ridge board where two of its slopes meet at a ridge, the rafters stopping at its faces.
    pub ridge: bool,
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
            rim: true,
            lap: 0.0,
            bridging: 0.0,
            trimmers: 2,
            under_partitions: true,
            ridge: true,
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
    Joist,
    Rim,
    Trimmer,
    Rafter,
    Ridge,
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
            Role::Joist => "joist",
            Role::Rim => "rim",
            Role::Trimmer => "trimmer",
            Role::Rafter => "rafter",
            Role::Ridge => "ridge",
        }
    }

    /// Whether it stands upright: its cut length is up it, not along the wall.
    pub fn upright(self) -> bool {
        matches!(self, Role::Stud | Role::King | Role::Jack | Role::Cripple)
    }
}

/// Which way a member's cut length runs: up it (a stud), along its frame's `s` (a plate, a joist, later a rafter, sloped
/// with its bottom and top), or across it (a floor's rim, a header between trimmers, blocking between joists).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Grain {
    Up,
    Along,
    Across,
}

/// One member, in its host's frame (or its own, `frame`): from `s[0]` to `s[1]` along it and `across[0]` to `across[1]`
/// across it, its plumb sides rising from a bottom at `bottom[0]` (at `s[0]`) and `bottom[1]` (at `s[1]`) to a top at
/// `top[0]` and `top[1]`, each straight between (a bevelled stud's top, a sloped plate, later a rafter).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Member {
    pub key: String,
    pub role: Role,
    /// One ply's section as sawn: [38, 89] for a 2x4, [38, 184] for each ply of a 2-2x8 header.
    pub section: [f64; 2],
    pub plies: u32,
    pub grain: Grain,
    /// Its cut length: up an upright one's longest edge, along a lying one's, across one lying across.
    pub length: f64,
    pub s: [f64; 2],
    pub across: [f64; 2],
    pub bottom: [f64; 2],
    pub top: [f64; 2],
    /// Its own origin and `s` axis in plan, where it doesn't lie square to its host's frame (a rim along a floor's skewed
    /// edge, every member of a roof, each in its slope's frame): its `s` and `across` are then measured in it, `across` to
    /// the left of its axis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<[P2; 2]>,
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

    /// A wall's: upright by its role, else lying along the wall. From `z`: its bottom and its top.
    fn new(key: String, role: Role, section: [f64; 2], plies: u32, s: [f64; 2], across: [f64; 2], z: [[f64; 2]; 2]) -> Self {
        let grain = if role.upright() { Grain::Up } else { Grain::Along };
        Member::lying(key, (role, grain), section, plies, [s, across], z)
    }

    /// One whose length runs `grain`, in its host's frame: from `place` (its `s` and its `across`) and `z` (its bottom and its
    /// top).
    pub(crate) fn lying(
        key: String,
        (role, grain): (Role, Grain),
        section: [f64; 2],
        plies: u32,
        [s, across]: [[f64; 2]; 2],
        z: [[f64; 2]; 2],
    ) -> Self {
        let [bottom, top] = z;
        let length = match grain {
            Grain::Up => (top[0] - bottom[0]).max(top[1] - bottom[1]),
            Grain::Along => {
                let run = s[1] - s[0];
                run.hypot(top[1] - top[0]).max(run.hypot(bottom[1] - bottom[0]))
            }
            Grain::Across => across[1] - across[0],
        };
        Member { key, role, section, plies, grain, length, s, across, bottom, top, frame: None }
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
    /// `s` 0 and `across` 0: a wall's, the start of its location line; a floor's, its outline's first corner; a roof's, its
    /// first slope's (each of its members is in its own slope's frame).
    pub origin: P2,
    /// A wall's along it; a floor's the way its joists run; a roof's up its first slope.
    pub x_axis: P2,
    /// The framing type's stock lengths.
    pub stock: Vec<f64>,
    pub members: Vec<Member>,
    /// What couldn't be framed as the type says: an opening left unframed, a header made shallower or left out.
    pub warnings: Vec<String>,
    /// A floor's or a roof's: the beams and walls that bear its joists or rafters. A wall's: none.
    pub supports: Vec<Guid>,
    /// A floor's or a roof's: the longest clear span of a joist or a rafter between what bears it, in plan, mm. A wall's: none.
    pub span: Option<f64>,
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

    /// Where member `m`'s `s` and `across` are measured from, and the way its `s` runs: its own frame, else its host's.
    pub fn frame_of(&self, m: &Member) -> [P2; 2] {
        m.frame.unwrap_or([self.origin, self.x_axis])
    }

    /// Member `m`'s plan rectangle, counter-clockwise.
    pub fn ring(&self, m: &Member) -> Vec<P2> {
        let [origin, x] = self.frame_of(m);
        let at = |s: f64, a: f64| origin + x * s + x.perp() * a;
        let ([s0, s1], [a0, a1]) = (m.s, m.across);
        vec![at(s0, a0), at(s1, a0), at(s1, a1), at(s0, a1)]
    }

    /// Member `m`'s solid: its plan rectangle, counter-clockwise, between its bottom and top planes.
    pub fn prism(&self, m: &Member) -> Prism {
        let [origin, x] = self.frame_of(m);
        let plane = |z: [f64; 2]| {
            let k = (z[1] - z[0]) / (m.s[1] - m.s[0]).max(EPS);
            let g = x * k;
            Plane { z: z[0] - k * m.s[0] - g.dot(origin), g }
        };
        Prism { ring: self.ring(m), bottom: plane(m.bottom), top: plane(m.top), curve: None }
    }

    /// The members alike by role, section, plies and length to the millimetre, each with the stock length it is cut from.
    pub fn cut_list(&self) -> Vec<Cut> {
        // Each row's count and its longest member as cut, which its stock length is chosen by (2440.3 isn't cut from 2440).
        let mut rows: BTreeMap<CutKey, (usize, f64)> = BTreeMap::new();
        for m in &self.members {
            let mm = |x: f64| x.round() as i64;
            let row = rows.entry((m.role, [mm(m.section[0]), mm(m.section[1])], m.plies, mm(m.length))).or_insert((0, 0.0));
            *row = (row.0 + 1, row.1.max(m.length));
        }
        rows.into_iter()
            .map(|((role, section, plies, length), (count, longest))| Cut {
                role,
                section: section.map(|x| x as f64),
                plies,
                length: length as f64,
                stock: stock(longest, &self.stock),
                count,
            })
            .collect()
    }
}

/// A cut list row's members alike: role, section and length to the millimetre, and plies.
type CutKey = (Role, [i64; 2], u32, i64);

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

/// The material host `id`'s framing type names for its members (a wall's, a floor's or a roof's), if it names one: else they're its
/// structure layer's.
pub(crate) fn material_of(m: &Model, id: Id) -> Option<&str> {
    match m.elements.get(id)? {
        Element::Wall(w) => material(m, w),
        Element::Floor(f) => floor::material(m, f),
        Element::Roof(r) => roof::material(m, r),
        _ => None,
    }
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
    within("lap (mm)", t.lap, 0.0, 2000.0)?;
    if t.bridging != 0.0 {
        within("bridging (mm; 0 for none)", t.bridging, 300.0, 100_000.0)?;
    }
    count("trimmers", t.trimmers, 1, 3)?;
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

/// Of two profiles over one run, the lower (with `low`) or the higher at every point: pieces split where the two cross.
fn envelope(p: &[[f64; 4]], q: &[[f64; 4]], low: bool) -> Vec<[f64; 4]> {
    // A profile's height at `s`, on its piece that holds `inside` (a point of the span asked about, not its end).
    let z = |pr: &[[f64; 4]], inside: f64, s: f64| {
        let Some(piece) = pr.iter().find(|x| inside <= x[1]).or(pr.last()) else { return f64::NAN };
        piece[2] + (piece[3] - piece[2]) * (s - piece[0]) / (piece[1] - piece[0]).max(EPS)
    };
    let mut cuts: Vec<f64> = p.iter().chain(q).flat_map(|x| [x[0], x[1]]).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|x, y| (*x - *y).abs() < EPS);
    let mut out: Vec<[f64; 4]> = Vec::new();
    let mut push = |a: f64, b: f64| {
        let inside = (a + b) / 2.0;
        let (za, zb) = (z(p, inside, a), z(p, inside, b));
        let (wa, wb) = (z(q, inside, a), z(q, inside, b));
        let first = if low { (za + zb) <= (wa + wb) } else { (za + zb) >= (wa + wb) };
        let piece = if first { [a, b, za, zb] } else { [a, b, wa, wb] };
        if piece.iter().any(|x| !x.is_finite()) {
            return;
        }
        match out.last_mut() {
            Some(last)
                if (last[3] - piece[2]).abs() < EPS
                    && ((last[3] - last[2]) / (last[1] - last[0]) - (piece[3] - piece[2]) / (b - a)).abs() < 1e-9 =>
            {
                [last[1], last[3]] = [b, piece[3]];
            }
            _ => out.push(piece),
        }
    };
    for pair in cuts.windows(2) {
        let &[a, b] = pair else { continue };
        if b - a <= EPS {
            continue;
        }
        let inside = (a + b) / 2.0;
        let (da, db) = (z(p, inside, a) - z(q, inside, a), z(p, inside, b) - z(q, inside, b));
        if da * db < 0.0 {
            let c = a + (b - a) * da / (da - db);
            push(a, c);
            push(c, b);
        } else {
            push(a, b);
        }
    }
    out
}

/// An opening's rough opening, in the wall's frame.
struct Rough {
    guid: Guid,
    id: Id,
    door: bool,
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
}

/// An opening framed: its rough opening, the span of its kings, its header, and whether the gaps over the header and under
/// its sill are filled flat, with no cripples.
struct FramedOpening<'a> {
    rough: &'a Rough,
    span: [f64; 2],
    header: Option<Header>,
    flat: [bool; 2],
}

/// Where one of a wall's ends is framed to.
struct End {
    /// Where its members end.
    at: f64,
    /// Whether it runs through an L corner there.
    through: bool,
    /// Running through an L corner: where the other wall's near structure face crosses it, the inside corner.
    inside: Option<f64>,
    /// Where its upper top plate ends: where it laps another's (`laps`), past `at` to the other's far structure face when it
    /// butts, short of it at the other's near one when it runs through; else `at`.
    lap: f64,
}

/// Whether framed walls `w` and `o` lap their upper top plates where they meet, as IRC R602.3.2 has a double top plate do: both
/// framed with two top plates or more of one thickness, neither attached to a roof or floor, their tops at one height.
fn laps(m: &Model, w: &Wall, o: &Wall) -> bool {
    let plates = |x: &Wall| x.framing.as_ref().and_then(|n| m.types.framing.get(n)).filter(|t| t.top_plates >= 2).map(|t| t.thickness);
    let same = matches!((plates(w), plates(o)), (Some(p), Some(q)) if (p - q).abs() < EPS);
    same && w.attach_top.is_none() && o.attach_top.is_none() && (m.own_top(w) - m.own_top(o)).abs() < EPS
}

/// Where one of this wall's ends is framed to: at a square L corner of two walls with a structure layer each, the far or the
/// near structure face of the other; where it butts into another's side, that one's near structure face; elsewhere where its
/// own structure layer's shorter edge ends.
fn end(m: &Model, id: Id, w: &Wall, k: usize, across: [f64; 2], at_end: bool) -> End {
    let axis = Axis::of(w);
    let u = axis.tangent(0.0);
    let along = |q: P2| (q - w.start).dot(u);
    let mid = (across[0] + across[1]) / 2.0;
    // Where the lines `offs` to the left of wall `o` cross this one's at `off`, along it.
    let crossings = |o: &Wall, offs: &[f64; 2], off: f64| -> Vec<f64> {
        let uo = (o.end - o.start).unit();
        let p = axis.at(0.0, off);
        offs.iter()
            .filter_map(|o_off| {
                let q = o.start + uo.perp() * *o_off;
                geom::line_intersection(p, p + u, q, q + uo).map(along)
            })
            .collect()
    };
    // The near and far of two places along: the far one past the other wall, away from this wall's body.
    let near_far = |x: f64, y: f64| if at_end { (x.min(y), x.max(y)) } else { (x.max(y), x.min(y)) };
    let (others, tee) = walls::joined(m, id, at_end);
    if let [other] = others.as_slice()
        && let Some(o) = m.wall(*other).filter(|o| o.arc == 0.0)
        && let Some((_, band)) = structure(m, o)
    {
        let square = u.dot((o.end - o.start).unit()).abs() <= 0.05;
        let lap = square && laps(m, w, o);
        if !tee && square {
            if let [x, y] = crossings(o, &band, mid)[..] {
                let (near, far) = near_far(x, y);
                let through = m.guid(id) < m.guid(*other);
                return if through {
                    End { at: far, through, inside: Some(near), lap: if lap { near } else { far } }
                } else {
                    End { at: near, through, inside: None, lap: if lap { far } else { near } }
                };
            }
        } else if tee {
            // Its end on the other's near structure face, where both its own structure layer's edges reach it.
            let edges: Vec<(f64, f64)> = across
                .iter()
                .filter_map(|off| match crossings(o, &band, *off)[..] {
                    [x, y] => Some(near_far(x, y)),
                    _ => None,
                })
                .collect();
            if let [(n0, f0), (n1, f1)] = edges[..] {
                let at = if at_end { n0.min(n1) } else { n0.max(n1) };
                let far = if at_end { f0.min(f1) } else { f0.max(f1) };
                return End { at, through: false, inside: None, lap: if lap { far } else { at } };
            }
        }
    }
    // Elsewhere, where the structure layer's shorter edge ends.
    let lines = walls::layer_geometry(m, id, None).lines;
    let ends = [k, k + 1].map(|i| {
        lines.get(i).and_then(|l| if at_end { l.last() } else { l.first() }).map_or(if at_end { axis.length() } else { 0.0 }, |q| along(*q))
    });
    let at = if at_end { ends[0].min(ends[1]) } else { ends[0].max(ends[1]) };
    End { at, through: false, inside: None, lap: at }
}

/// The walls ending on this one's side, each with where its structure layer (else its whole thickness) crosses this
/// wall's: by their GUIDs, `[from, to]` along it, and whether its upper top plate laps over this one's (square to it, and
/// `laps`).
fn tees(m: &Model, id: Id, w: &Wall, mid: f64) -> Vec<(Guid, [f64; 2], bool)> {
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
            let lap = u.dot(uo).abs() <= 0.05 && structure(m, o).is_some() && laps(m, w, o);
            out.push((m.guid(other), [x.min(y), x.max(y)], lap));
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
        warnings: Vec::new(),
        supports: Vec::new(),
        span: None,
    };
    let (start, finish) = (end(m, id, w, k, across, false), end(m, id, w, k, across, true));
    let (a, b) = (start.at, finish.at);
    if b - a <= EPS {
        return Ok(Some(out));
    }
    let th = t.thickness;
    let depth = across[1] - across[0];
    let section = [th, depth];
    // The base and top over the structure layer: the higher base and the lower top of its two faces, so that no member
    // rises out of the wall where a roof slopes across it. The top as far as an upper top plate laps past the ends.
    let faces = |top: bool, s0: f64, s1: f64| {
        let [p, q] = across.map(|off| walls::profile(m, w, top, off, s0, s1));
        envelope(&p, &q, top)
    };
    let base = Profile(faces(false, a, b));
    let top = Profile(faces(true, a.min(start.lap), b.max(finish.lap)));
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
    let lowest_floor = |s0: f64, s1: f64| base.stations(s0, s1).map(floor).fold(f64::INFINITY, f64::min);
    let highest_ceiling = |s0: f64, s1: f64| top.stations(s0, s1).map(ceiling).fold(f64::NEG_INFINITY, f64::max);
    // The shortest a cripple is (`CRIPPLE`).
    let short = CRIPPLE * th;
    let own_base = m.own_base(w);
    let members = &mut out.members;
    let warnings = &mut out.warnings;

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
                id: o,
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
    // Each with its ply, spliced where longer than the longest stock once the studs they may be spliced over stand.
    let mut plates: Vec<(u32, Member)> = Vec::new();
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
                plates
                    .push((ply, Member::new(key, Role::BottomPlate, section, 1, [s0, s1], across, [bottom, bottom.map(|z| z + plumb(k))])));
            }
        }
    }
    // The upper one laps the walls it meets (`laps`): it runs past a butting wall's end and stops short of a wall's that runs
    // through, and it is cut round a wall butting into this one's side, whose own upper plate runs over the lower one here.
    let lapped: Vec<(Guid, [f64; 2])> = if t.top_plates >= 2 {
        tees(m, id, w, mid).into_iter().filter(|(_, _, lap)| *lap).map(|(g, span, _)| (g, span)).collect()
    } else {
        Vec::new()
    };
    for (i, q) in top.0.iter().enumerate() {
        let k = (q[3] - q[2]) / (q[1] - q[0]).max(EPS);
        for ply in 0..t.top_plates {
            let (from, to) = if ply == 0 { (start.lap, finish.lap) } else { (a, b) };
            let (s0, s1) = (q[0].max(from), q[1].min(to));
            let mut spans: Vec<(Option<Guid>, f64, f64)> = Vec::new();
            let (mut at, mut after) = (s0, None);
            for (g, [c0, c1]) in lapped.iter().filter(|(_, [c0, c1])| ply == 0 && *c0 < s1 - EPS && *c1 > s0 + EPS) {
                if c0 - at > EPS {
                    spans.push((after, at, *c0));
                }
                if *c1 > at {
                    (at, after) = (*c1, Some(*g));
                }
            }
            spans.push((after, at, s1));
            let drop = f64::from(ply + 1) * plumb(k);
            let under = |s: f64| q[2] + k * (s - q[0]) - drop;
            for (after, p0, p1) in spans {
                if p1 - p0 <= EPS {
                    continue;
                }
                let bottom = [under(p0), under(p1)];
                // A wall too low for its plates keeps those that clear the bottom ones.
                if bottom[0] < floor(p0) - EPS || bottom[1] < floor(p1) - EPS {
                    continue;
                }
                let key = after.map_or_else(|| format!("top_plate/{ply}/{i}"), |g| format!("top_plate/{ply}/{i}/{g}"));
                plates.push((ply, Member::new(key, Role::TopPlate, section, 1, [p0, p1], across, [bottom, bottom.map(|z| z + plumb(k))])));
            }
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
        // Its face on the inside corner, where the other wall's structure begins, for the lining to be nailed to; never
        // nearer the end than beside the end stud.
        let (c0, c1) = (
            start.inside.map_or(a + 1.5 * th, |n| (n - th / 2.0).max(a + 1.5 * th)),
            finish.inside.map_or(b - 1.5 * th, |n| (n + th / 2.0).min(b - 1.5 * th)),
        );
        for (through, c, key) in [(start.through, c0, "corner/start"), (finish.through, c1, "corner/end")] {
            if through && c >= a + th / 2.0 && c <= b - th / 2.0 && clear(&taken, c) {
                stud(members, &mut full, &mut taken, key.into(), c);
            }
        }
    }

    // Each opening that fits, the first along the wall first: kings, jacks, a header on the jacks and a window's sill.
    let (kings, jacks) = (f64::from(t.kings), f64::from(t.jacks));
    let mut framed: Vec<FramedOpening> = Vec::new();
    for r in &roughs {
        let what = if r.door { "door" } else { "window" };
        let span = [r.left - (jacks + kings) * th, r.right + (jacks + kings) * th];
        let width = r.right - r.left;
        let Some(wanted) = t.headers.iter().find(|h| h.up_to.is_none_or(|u| u >= width - EPS)).or(t.headers.last()) else { continue };
        let ends = [r.left - jacks * th, r.right + jacks * th];
        let sill_under = r.bottom - f64::from(t.sill_plates) * th;
        let why = if span[0] < a - EPS || span[1] > b + EPS {
            Some("its kings and jacks would run past the framed end of the wall".to_string())
        } else if !taken.iter().all(|v| v + th / 2.0 <= span[0] + EPS || v - th / 2.0 >= span[1] - EPS) {
            Some("its kings and jacks would stand on a corner's or the wall's end studs".to_string())
        } else if let Some(o) = framed.iter().find(|o| o.span[1] > span[0] + EPS && o.span[0] < span[1] - EPS) {
            Some(format!("its kings and jacks would overlap those of {} {}", if o.rough.door { "door" } else { "window" }, o.rough.id))
        } else if r.top <= highest_floor(ends[0], ends[1]) + EPS {
            Some("it stands below the bottom plates".to_string())
        } else if r.bottom > lowest_ceiling(ends[0], ends[1]) - EPS {
            Some(format!("its rough opening is in the top plates (it starts {:.0} mm up)", r.bottom - own_base))
        } else if !r.door && sill_under < highest_floor(r.left, r.right) - EPS {
            Some(format!("its sill would be under the bottom plates (its rough opening starts {:.0} mm up)", r.bottom - own_base))
        } else {
            None
        };
        if let Some(why) = why {
            warnings.push(format!("{what} {} isn't framed: {why}; move it or change its size", r.id));
            continue;
        }
        // The header, as deep as the type says when it fits under the top plates; shallower, or none, when it doesn't.
        let ceiling_over = lowest_ceiling(ends[0], ends[1]);
        let room = ceiling_over - r.top;
        let header = if wanted.height() <= room + EPS {
            Some(wanted.clone())
        } else if room >= th - EPS {
            let cut = if wanted.flat {
                let plies = ((room / wanted.depth + EPS).floor() as u32).max(1);
                Header { depth: (room / f64::from(plies)).min(wanted.depth), plies, ..wanted.clone() }
            } else {
                Header { depth: room, ..wanted.clone() }
            };
            warnings.push(format!(
                "{what} {}: only {room:.0} mm between its rough opening and the top plates, less than its {:.0} mm header; \
                 the header is cut to {:.0} mm: lower the opening or check the header's span",
                r.id,
                wanted.height(),
                cut.height()
            ));
            Some(cut)
        } else {
            warnings.push(format!(
                "{what} {}: its rough opening reaches the top plates (its head at {:.0} mm, their underside at {ceiling_over:.0} mm): \
                 no room for a header; lower the opening or make the wall taller",
                r.id,
                r.top - own_base
            ));
            None
        };
        // A head into the top plates is framed up to them.
        let r_top = r.top.min(ceiling_over);
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
                upright(members, format!("{g}/jack/{side}/{i}"), Role::Jack, c, &floor, &|_| r_top);
            }
        }
        if let Some(header) = &header {
            let (h_across, h_section) = if header.flat {
                (across, [header.depth, depth])
            } else {
                let half = f64::from(header.plies) * th / 2.0;
                ([mid - half, mid + half], [th, header.depth])
            };
            let (z0, z1) = (r_top, r_top + header.height());
            members.push(Member::new(format!("{g}/header"), Role::Header, h_section, header.plies, ends, h_across, [[z0; 2], [z1; 2]]));
        }
        // A gap too short for a cripple all along, over the header or under a window's sill: as many flats as fit, stacked on the
        // header or on the bottom plates, as long as it or the sill. Where it is long enough anywhere (under a sloped top), the
        // cripples stand where they are long enough.
        let flats = |members: &mut Vec<Member>, at: &str, s: [f64; 2], z0: f64, room: f64| {
            for i in 0..((room / th + EPS).floor().max(0.0) as u32) {
                let z = z0 + f64::from(i) * th;
                members.push(Member::new(format!("{g}/blocking/{at}/{i}"), Role::Blocking, section, 1, s, across, [[z; 2], [z + th; 2]]));
            }
        };
        let over = header.as_ref().map(|h| r_top + h.height());
        let top_flat = over.is_some_and(|over| highest_ceiling(ends[0], ends[1]) - over < short - EPS);
        if let Some(over) = over.filter(|_| top_flat) {
            flats(members, "top", ends, over, lowest_ceiling(ends[0], ends[1]) - over);
        }
        let under = r.bottom - f64::from(t.sill_plates) * th;
        let bottom_flat = !r.door && under - lowest_floor(r.left, r.right) < short - EPS;
        if bottom_flat {
            let on = highest_floor(r.left, r.right);
            flats(members, "bottom", [r.left, r.right], on, under - on);
        }
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
        framed.push(FramedOpening { rough: r, span, header, flat: [top_flat, bottom_flat] });
    }
    // Where the grid can't stand a stud: an opening not framed, in its rough opening.
    let in_rough = |c: f64| roughs.iter().any(|r| c + th / 2.0 > r.left + EPS && c - th / 2.0 < r.right - EPS);

    // Backing each side of a wall that butts into this one's side; the grid stands no stud between the two.
    let mut backed: Vec<[f64; 2]> = Vec::new();
    if t.tee == Tee::Studs {
        for (other, [p0, p1], _) in tees(m, id, w, mid) {
            let mut any = false;
            for (j, c) in [(0, p0 - th / 2.0), (1, p1 + th / 2.0)] {
                let inside = c >= a + th / 2.0 - EPS && c <= b - th / 2.0 + EPS;
                let in_span = framed.iter().any(|o| c + th / 2.0 > o.span[0] + EPS && c - th / 2.0 < o.span[1] - EPS);
                if inside && !in_span && !in_rough(c) && clear(&taken, c) {
                    stud(members, &mut full, &mut taken, format!("tee/{other}/{j}"), c);
                    any = true;
                }
            }
            if any {
                backed.push([p0 - th / 2.0, p1 + th / 2.0]);
            }
        }
    }
    let between_backing = |c: f64| backed.iter().any(|[l, r]| c > l + EPS && c < r - EPS);

    // The grid: a stud at each station clear of the others, and inside a framed rough opening a cripple over its header
    // and, a window's, one under its sill, where it is no shorter than `short` and the gap isn't filled flat.
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
        if let Some(FramedOpening { rough: r, header, flat: [top_flat, bottom_flat], .. }) =
            framed.iter().find(|o| c > o.span[0] - th / 2.0 && c < o.span[1] + th / 2.0)
        {
            let long = |lo: &dyn Fn(f64) -> f64, hi: &dyn Fn(f64) -> f64| {
                [c - th / 2.0, c + th / 2.0].iter().all(|s| hi(*s) - lo(*s) >= short - EPS)
            };
            if c >= r.left + th / 2.0 - EPS && c <= r.right - th / 2.0 + EPS {
                if let Some(header) = header.as_ref().filter(|_| !top_flat) {
                    let over = r.top + header.height();
                    if long(&|_| over, &ceiling) {
                        upright(members, format!("{}/cripple/top/{i}", r.guid), Role::Cripple, c, &|_| over, &ceiling);
                    }
                }
                if !r.door && !bottom_flat {
                    let under = r.bottom - f64::from(t.sill_plates) * th;
                    if long(&floor, &|_| under) {
                        upright(members, format!("{}/cripple/bottom/{i}", r.guid), Role::Cripple, c, &floor, &|_| under);
                    }
                }
            }
            continue;
        }
        if !in_rough(c) && !between_backing(c) && clear(&taken, c) {
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
            // None in a bay narrower than a stud is thick: a sliver no one could nail.
            if s1 - s0 < th - EPS || roughs.iter().any(|o| s0 < o.right && s1 > o.left) {
                continue;
            }
            if z0 < highest_floor(s0, s1) - EPS || z1 > lowest_ceiling(s0, s1) + EPS {
                continue;
            }
            members.push(Member::new(format!("blocking/{row}/{key}"), Role::Blocking, section, 1, [s0, s1], across, [[z0; 2], [z1; 2]]));
        }
    }

    // A plate longer than the longest stock length, spliced over full-height studs (so never over an opening), into the
    // fewest pieces that length holds ([`spliced`]); the two top plates' splices `TOP_SPLICES` apart, the lower one's
    // spliced first. Each piece keyed by the plate's key and its place, from 0. Where none holds it, a warning.
    let posts: Vec<f64> = full.iter().map(|(c, _)| *c).collect();
    let mut order: Vec<usize> = (0..plates.len()).collect();
    order.sort_by_key(|i| plates.get(*i).map(|(ply, p)| (p.role == Role::TopPlate, std::cmp::Reverse(*ply))));
    let mut cut: Vec<Vec<f64>> = vec![Vec::new(); plates.len()];
    let mut joints: Vec<(u32, f64)> = Vec::new();
    for i in order {
        let (Some((ply, p)), Some(&longest)) = (plates.get(i), t.stock.last()) else { continue };
        let run = p.s[1] - p.s[0];
        if p.length <= longest + EPS || run <= EPS {
            continue;
        }
        let topped = p.role == Role::TopPlate;
        let apart = |c: f64| !topped || joints.iter().all(|(q, j)| q == ply || (c - j).abs() >= TOP_SPLICES - EPS);
        let over: Vec<f64> = posts.iter().copied().filter(|c| *c > p.s[0] + th - EPS && *c < p.s[1] - th + EPS && apart(*c)).collect();
        let at = spliced(&over, p.s, longest, p.length / run, 0.0, |len| {
            warnings.push(format!(
                "{} has a piece {len:.0} mm long: no splice over a full-height stud{} keeps every piece within the longest stock \
                 length, {longest:.0} mm, so it is longer than any stock; add stock, or a stud where it can be spliced",
                p.key,
                if topped { format!(" {TOP_SPLICES:.0} mm from the other top plates' splices") } else { String::new() }
            ));
        });
        if topped {
            joints.extend(at.iter().map(|c| (*ply, *c)));
        }
        if let Some(c) = cut.get_mut(i) {
            *c = at;
        }
    }
    let mut spliced_plates: Vec<Member> = Vec::new();
    for ((_, p), at) in plates.into_iter().zip(cut) {
        if at.is_empty() {
            spliced_plates.push(p);
            continue;
        }
        let ends: Vec<f64> = std::iter::once(p.s[0]).chain(at).chain(std::iter::once(p.s[1])).collect();
        let z = |e: [f64; 2], x: f64| e[0] + (e[1] - e[0]) * (x - p.s[0]) / (p.s[1] - p.s[0]);
        for (n, w) in ends.windows(2).enumerate() {
            let s = [w.first().copied().unwrap_or(p.s[0]), w.last().copied().unwrap_or(p.s[1])];
            let place = [s.map(|x| z(p.bottom, x)), s.map(|x| z(p.top, x))];
            spliced_plates.push(Member::new(format!("{}/{n}", p.key), p.role, p.section, p.plies, s, p.across, place));
        }
    }
    members.splice(0..0, spliced_plates);
    Ok(Some(out))
}

/// Where a run `iv` along `s` is spliced over supports `over` (along it, in order, each clear of its ends), a piece's length
/// taken along its slope (`sec` times its plan length) with its laps (each piece running `reach` past the support it ends
/// over, the next starting `reach` before it): nowhere where it is no longer than `longest`; else into the fewest pieces no
/// longer than that, and of those splits the one whose longest piece is shortest, ties going to the one whose splices lie
/// nearest its start, the first splice first. Where no split keeps every piece within `longest`, the split whose longest
/// piece is shortest, in the fewest pieces, ties alike, and `too_long` is told the length along its slope of each piece longer
/// than `longest`. A dynamic programme over the supports, O(n²).
pub(crate) fn spliced(over: &[f64], iv: [f64; 2], longest: f64, sec: f64, reach: f64, mut too_long: impl FnMut(f64)) -> Vec<f64> {
    // The places a piece runs between: the run's start (0), each support, the run's end (`last`).
    let last = over.len() + 1;
    let place = |k: usize| if k == 0 { iv[0] } else { over.get(k - 1).copied().unwrap_or(iv[1]) };
    // A piece from place `a` to place `b`, along its slope, its laps within the run.
    let piece = |a: usize, b: usize| {
        let from = if a == 0 { iv[0] } else { (place(a) - reach).max(iv[0]) };
        let to = if b == last { iv[1] } else { (place(b) + reach).min(iv[1]) };
        (to - from) * sec
    };
    // From each place to the end: the shortest the longest piece can be, in as many pieces as it takes.
    let mut least = vec![f64::INFINITY; last + 1];
    if let Some(l) = least.get_mut(last) {
        *l = 0.0;
    }
    for a in (0..last).rev() {
        let most = (a + 1..=last).map(|b| piece(a, b).max(least.get(b).copied().unwrap_or(f64::INFINITY))).fold(f64::INFINITY, f64::min);
        if let Some(l) = least.get_mut(a) {
            *l = most;
        }
    }
    let first = least.first().copied().unwrap_or(f64::INFINITY);
    let cap = if first <= longest + EPS { longest } else { first };
    // From each place to the end with no piece longer than `cap`: the fewest pieces, and of those the shortest longest one.
    let mut best: Vec<Option<(usize, f64)>> = vec![None; last + 1];
    if let Some(b) = best.get_mut(last) {
        *b = Some((0, 0.0));
    }
    for a in (0..last).rev() {
        let mut here: Option<(usize, f64)> = None;
        for b in a + 1..=last {
            let len = piece(a, b);
            if len > cap + EPS {
                break;
            }
            let Some(Some((count, most))) = best.get(b).copied() else { continue };
            let it = (count + 1, most.max(len));
            if here.is_none_or(|(c, m)| it.0 < c || (it.0 == c && it.1 < m - EPS)) {
                here = Some(it);
            }
        }
        if let Some(h) = best.get_mut(a) {
            *h = here;
        }
    }
    let Some(Some((count, most))) = best.first().copied() else {
        too_long(piece(0, last));
        return Vec::new();
    };
    // The fewest pieces from each place to the end, none longer than `most`; then from the start, each piece to the nearest
    // place from which the rest still takes no more pieces than are left.
    let mut fewest: Vec<Option<usize>> = vec![None; last + 1];
    if let Some(f) = fewest.get_mut(last) {
        *f = Some(0);
    }
    for a in (0..last).rev() {
        let n =
            (a + 1..=last).take_while(|b| piece(a, *b) <= most + EPS).filter_map(|b| fewest.get(b).copied().flatten()).min().map(|n| n + 1);
        if let Some(f) = fewest.get_mut(a) {
            *f = n;
        }
    }
    let mut chosen: Vec<f64> = Vec::new();
    let (mut a, mut left) = (0, count);
    while a != last && left > 0 {
        let next =
            (a + 1..=last).take_while(|b| piece(a, *b) <= most + EPS).find(|b| fewest.get(*b).copied().flatten().is_some_and(|n| n < left));
        let Some(b) = next else { break };
        let len = piece(a, b);
        if len > longest + EPS {
            too_long(len);
        }
        if b != last {
            chosen.push(place(b));
        }
        (a, left) = (b, left - 1);
    }
    chosen
}

/// To the hundredth of a millimetre.
fn mm(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn member_json(f: &Framing, m: &Member) -> Value {
    let mut v = json!({
        "key": m.key, "role": m.role, "section": m.section, "plies": m.plies, "grain": m.grain, "length": mm(m.length),
        "stock": stock(m.length, &f.stock), "s": m.s.map(mm), "across": m.across.map(mm),
        "z": [mm(m.bottom[0]), mm(m.bottom[1]), mm(m.top[0]), mm(m.top[1])],
    });
    if let Some([origin, x]) = m.frame {
        v["frame"] = json!({"origin": [origin.0, origin.1], "x_axis": [x.0, x.1]});
    }
    v
}

/// The walls, floors and roofs `ids` names: refused for anything else.
fn hosts_of(c: &str, m: &Model, p: &Value) -> Result<Vec<Id>, String> {
    let ids = crate::commands::ids_param(c, m, p)?;
    if let Some((id, e)) = ids
        .iter()
        .filter_map(|id| Some((*id, m.elements.get(*id)?)))
        .find(|(_, e)| !matches!(e, Element::Wall(_) | Element::Floor(_) | Element::Roof(_)))
    {
        return Err(bad(c, format!("{} {id} isn't a wall, a floor or a roof: only walls, floors and roofs are framed", e.category())));
    }
    Ok(ids)
}

/// Host `id`'s framing: a wall's, a floor's or a roof's; none for anything else or what isn't framed.
pub fn of(m: &Model, id: Id) -> Result<Option<Framing>, String> {
    match m.elements.get(id) {
        Some(Element::Wall(_)) => wall(m, id),
        Some(Element::Floor(_)) => floor::floor(m, id),
        Some(Element::Roof(_)) => roof::roof(m, id),
        _ => Ok(None),
    }
}

/// What a host is called in a message: `wall 100002`, `floor 100003`, `roof 100004`.
fn named(m: &Model, id: Id) -> String {
    format!("{} {id}", m.elements.get(id).map_or("element", Element::category))
}

/// `frame`: walls, floors and roofs given a framing type, the project's default one (made, if it has none) unless `type`
/// names another; floors their joists' direction too, where `angle` is given (a roof's rafters run up its slopes).
pub(crate) fn frame(s: &mut Session, p: &Value) -> Result<Value, String> {
    let c = "frame";
    crate::commands::only(c, p, &["id", "ids", "type", "angle"])?;
    let ids = hosts_of(c, &s.model, p)?;
    let given = crate::commands::string(c, p, "type")?;
    if let Some(n) = given.as_ref().filter(|n| !s.model.types.framing.contains_key(*n)) {
        let names: Vec<&str> = s.model.types.framing.keys().map(String::as_str).collect();
        return Err(bad(
            c,
            format!("there is no framing type `{n}` (there are: {}); make it with `type` {{category: framing, name}}", names.join(", ")),
        ));
    }
    let angle = match p.get("angle") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_f64()
                .filter(|a| a.is_finite() && a.abs() <= 3600.0)
                .ok_or_else(|| bad(c, "`angle` is the way a floor's joists run, degrees from +x (east), a number from -3600 to 3600"))?,
        ),
    };
    let m = s.model_mut();
    if given.is_none() && m.types.framing.is_empty() {
        m.types.framing.insert(DEFAULT_TYPE.into(), FramingType::default());
    }
    let name = given.or_else(|| m.default_type("framing").map(String::from)).unwrap_or_else(|| DEFAULT_TYPE.into());
    for id in &ids {
        match m.elements.get_mut(*id) {
            Some(Element::Wall(w)) => w.framing = Some(name.clone()),
            Some(Element::Floor(f)) => {
                f.framing = Some(name.clone());
                if angle.is_some() {
                    f.framing_angle = angle;
                }
            }
            Some(Element::Roof(r)) => r.framing = Some(name.clone()),
            _ => {}
        }
    }
    let (mut total, mut warnings) = (0, Vec::new());
    for id in &ids {
        let who = named(m, *id);
        if let Some(f) = of(m, *id).map_err(|e| bad(c, format!("{who}: {e}")))? {
            total += f.members.len();
            warnings.extend(f.warnings.into_iter().map(|w| format!("{who}: {w}")));
        }
    }
    Ok(json!({"ids": ids, "type": name, "members": total, "warnings": warnings}))
}

/// `unframe`: walls, floors and roofs framed no more.
pub(crate) fn unframe(s: &mut Session, p: &Value) -> Result<Value, String> {
    let c = "unframe";
    crate::commands::only(c, p, &["id", "ids"])?;
    let ids = hosts_of(c, &s.model, p)?;
    let m = s.model_mut();
    for id in &ids {
        match m.elements.get_mut(*id) {
            Some(Element::Wall(w)) => w.framing = None,
            Some(Element::Floor(f)) => f.framing = None,
            Some(Element::Roof(r)) => r.framing = None,
            _ => {}
        }
    }
    Ok(json!({"ids": ids}))
}

/// `framing`: a framed host's members and its cut list.
pub(crate) fn query(s: &mut Session, p: &Value) -> Result<Value, String> {
    let c = "framing";
    crate::commands::only(c, p, &["id"])?;
    let m = &s.model;
    let ids = hosts_of(c, m, p)?;
    let [id] = ids[..] else { return Err(bad(c, "`id`, one wall, floor or roof, is required")) };
    let who = named(m, id);
    let f = of(m, id).map_err(|e| bad(c, format!("{who}: {e}")))?.ok_or_else(|| bad(c, format!("{who} isn't framed: `frame` it")))?;
    let cuts: Vec<Value> = f
        .cut_list()
        .into_iter()
        .map(|r| json!({"role": r.role, "section": r.section, "plies": r.plies, "length": r.length, "stock": r.stock, "count": r.count}))
        .collect();
    let mut out = json!({
        "units": "mm", "host": f.host, "id": id, "type": f.type_name, "layer": f.layer, "material": f.material,
        "origin": [f.origin.0, f.origin.1], "x_axis": [f.x_axis.0, f.x_axis.1], "depth_axis": [f.depth_axis().0, f.depth_axis().1],
        "members": f.members.iter().map(|m| member_json(&f, m)).collect::<Vec<_>>(), "cut_list": cuts, "warnings": f.warnings,
    });
    if let Some(span) = f.span {
        let supports: Vec<Id> = f.supports.iter().filter_map(|g| m.elements.id(*g)).collect();
        out["supports"] = json!(supports);
        out["span"] = json!(mm(span));
    }
    match m.elements.get(id) {
        Some(Element::Floor(_)) => out["angle"] = json!(mm(f.x_axis.1.atan2(f.x_axis.0).to_degrees())),
        // A roof's slopes, each with its frame (its members' `frame`) and how steep it is.
        Some(Element::Roof(r)) => {
            let faces = roof::faces(m, r).unwrap_or_default();
            out["faces"] = json!(
                faces
                    .iter()
                    .map(
                        |f| json!({"index": f.index, "origin": [f.origin.0, f.origin.1], "x_axis": [f.x.0, f.x.1], "slope": mm(f.slope())})
                    )
                    .collect::<Vec<_>>()
            );
        }
        _ => {}
    }
    Ok(out)
}
