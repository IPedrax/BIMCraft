//! Curtain walls: a wall whose type holds a grid pattern, with panels and mullions along it.
//!
//! Panels and mullions are system families. Their types live in [`Types`](crate::model::Types) as any other category's do.

use serde::{Deserialize, Serialize};

use bimcraft_i18n::ui_msg;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Value, json};

use crate::geom::P2;
use crate::ids::Guid;
use crate::library::Kind;
use crate::model::{Element, Location, MAX_COORD, Model, Wall, WallType};
use crate::walls::Axis;

/// The Generic Panel type a new curtain wall fills its cells with.
pub const GLAZED: &str = "Glazed";
/// The other Generic Panel type a project starts with.
pub const SPANDREL: &str = "Spandrel";
/// The mullion type a project starts with.
pub const RECTANGULAR_MULLION: &str = "50 x 150";
/// How far the starter mullion stands toward the wall's exterior from its grid line, mm.
const STARTER_OFFSET: f64 = 50.0;

/// A type of the Generic Panel family: a flat panel `thickness` thick, its centre `offset` mm from the grid's plane toward the
/// exterior.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurtainPanelType {
    pub thickness: f64,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub offset: f64,
}

/// A mullion's section. Only a rectangle for now; a drawn profile will be another kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum MullionProfile {
    Rectangular { width: f64, depth: f64 },
}

/// A type of a mullion family: its section, centred `offset_x` mm from its grid line along the wall's face and `offset_y` mm
/// from the grid's plane toward the exterior.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MullionType {
    pub profile: MullionProfile,
    #[serde(default)]
    pub offset_x: f64,
    #[serde(default)]
    pub offset_y: f64,
    #[serde(default)]
    pub material: String,
}

impl MullionType {
    /// How wide it is along the wall's face.
    pub fn width(&self) -> f64 {
        match self.profile {
            MullionProfile::Rectangular { width, .. } => width,
        }
    }

    /// How deep it is across the wall.
    pub fn depth(&self) -> f64 {
        match self.profile {
            MullionProfile::Rectangular { depth, .. } => depth,
        }
    }
}

/// The panels and the mullion a project starts with, those it doesn't have yet.
pub(crate) fn starter_types(m: &mut Model) {
    let word = |m: &Model, name: &str| {
        let material = m.language.word(name).to_string();
        if m.materials.contains_key(&material) { material } else { String::new() }
    };
    let (glass, metal) = (word(m, "Glass"), word(m, "Metal"));
    let panels = &mut m.types.curtain_panel;
    if panels.is_empty() {
        panels.insert(GLAZED.into(), CurtainPanelType { thickness: 24.0, material: glass, offset: 0.0 });
        panels.insert(SPANDREL.into(), CurtainPanelType { thickness: 24.0, material: metal.clone(), offset: 0.0 });
    }
    if m.types.mullion.is_empty() {
        let profile = MullionProfile::Rectangular { width: 50.0, depth: 150.0 };
        m.types
            .mullion
            .insert(RECTANGULAR_MULLION.into(), MullionType { profile, offset_x: 0.0, offset_y: STARTER_OFFSET, material: metal });
    }
    if !m.types.wall.contains_key(CURTAIN_WALL) {
        let way = || Direction { segments: vec![1500.0], fit: Fit::Flexible, justify: Justify::Start };
        let l = CurtainLayout::uniform(
            way(),
            way(),
            m.default_type("curtainpanel").map(String::from).as_deref(),
            m.default_type("mullion").map(String::from).as_deref(),
        );
        let thickness = thickness(m, &l);
        m.types.wall.insert(CURTAIN_WALL.into(), WallType { thickness, layers: Vec::new(), curtain: Some(l) });
    }
}

/// What's wrong with the project's panel and mullion types, its patterns and its curtain walls, saying what to fix.
pub(crate) fn check(m: &Model) -> Result<(), String> {
    let material = |who: &str, name: &str| {
        if !name.is_empty() && !m.materials.contains_key(name) {
            return Err(format!("{who}: material `{name}` isn't in the library; add it with `material`"));
        }
        Ok(())
    };
    let size = |who: &str, what: &str, v: f64| {
        if !(v.is_finite() && v > 0.0 && v <= MAX_SIZE) {
            return Err(format!("{who}: {what} {v} mm; it is more than 0 and at most {MAX_SIZE} mm"));
        }
        Ok(())
    };
    let offset = |who: &str, what: &str, v: f64| {
        if !(v.is_finite() && v.abs() <= MAX_SIZE) {
            return Err(format!("{who}: {what} {v} mm; it is at most {MAX_SIZE} mm either way"));
        }
        Ok(())
    };
    for (n, t) in &m.types.curtain_panel {
        let who = format!("curtain panel type `{n}`");
        size(&who, "thickness", t.thickness)?;
        offset(&who, "offset", t.offset)?;
        material(&who, &t.material)?;
    }
    for (n, t) in &m.types.mullion {
        let who = format!("mullion type `{n}`");
        size(&who, "width", t.width())?;
        size(&who, "depth", t.depth())?;
        offset(&who, "offset_x", t.offset_x)?;
        offset(&who, "offset_y", t.offset_y)?;
        material(&who, &t.material)?;
    }
    let mut any = false;
    for (n, t) in &m.types.wall {
        if let Some(l) = &t.curtain {
            check_pattern(m, n, l)?;
            any = true;
        }
    }
    if any {
        let mut taken = BTreeSet::new();
        for (id, e) in m.elements.iter() {
            match e {
                Element::Wall(w) => {
                    if let Some(l) = pattern(m, w) {
                        check_wall(m, *id, w, l)?;
                    }
                    check_panel_wall(m, *id, w, &mut taken)?;
                }
                Element::Door(o) | Element::Window(o) if m.wall(o.host).is_some_and(|w| pattern(m, w).is_some()) => {
                    return Err(format!(
                        "{} {id} is in a curtain wall: replace a panel with a wall first, and put it in that wall",
                        e.category()
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The most a panel's thickness, a mullion's width or depth, or an offset of one may be, mm.
pub const MAX_SIZE: f64 = 10_000.0;

// ---------- the pattern ----------

/// The wall type a project starts with for curtain walls.
pub const CURTAIN_WALL: &str = "Curtain Wall";
/// The fewest millimetres a segment of a pattern may be.
pub const MIN_SEGMENT: f64 = 10.0;
/// The most segments a pattern has each way.
pub const MAX_SEGMENTS: usize = 64;
/// The most cells a curtain wall has: past it, the wall is refused rather than built.
pub const MAX_CELLS: f64 = 250_000.0;
/// How narrow a panel may be unless a type says otherwise, mm.
pub const MIN_PANEL: f64 = 50.0;
/// How close two lengths are to be the same, mm.
const SAME: f64 = 1e-3;

/// What a wall's length does with its pattern: Fixed Grid draws it in the pattern's lengths; Last Grid Flexible lets the last
/// segment take what's left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    #[default]
    Flexible,
    Fixed,
}

/// Where a pattern is laid from: the start, the middle (whole modules centred, or a grid line on it) or the end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Justify {
    #[default]
    Start,
    Centre,
    CentreLine,
    End,
}

/// Which mullions run through where they cross at a butt joint, when neither is bigger: a deeper one, or of one depth a wider
/// one, runs through whatever this says ([`Bar::runs_through`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Continuous {
    #[default]
    Vertical,
    Horizontal,
}

/// How a mullion meets those it crosses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Join {
    #[default]
    Butt,
    Mitre,
}

fn is_butt(j: &Join) -> bool {
    *j == Join::Butt
}

/// A mullion on an edge of the pattern.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MullionPick {
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "is_butt")]
    pub join: Join,
}

/// A pattern along one way: its segments' sizes, repeated as far as the wall goes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Direction {
    pub segments: Vec<f64>,
    #[serde(default)]
    pub fit: Fit,
    #[serde(default)]
    pub justify: Justify,
}

/// The mullions round a curtain wall.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Borders {
    #[serde(default)]
    pub left: Option<MullionPick>,
    #[serde(default)]
    pub right: Option<MullionPick>,
    #[serde(default)]
    pub bottom: Option<MullionPick>,
    #[serde(default)]
    pub top: Option<MullionPick>,
}

/// A curtain wall type's pattern. `panels[row][column]` fills a cell; `vertical[row][column]` runs along the line at that
/// column's start, in that row; `horizontal[row][column]` runs along the line at that row's bottom, in that column. A cell or
/// an edge may hold nothing. The line that starts a module closes the one before it; the wall's own edges are its `borders`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurtainLayout {
    pub columns: Direction,
    pub rows: Direction,
    pub panels: Vec<Vec<Option<String>>>,
    pub vertical: Vec<Vec<Option<MullionPick>>>,
    pub horizontal: Vec<Vec<Option<MullionPick>>>,
    #[serde(default)]
    pub borders: Borders,
    #[serde(default)]
    pub continuous: Continuous,
    /// A cell that leaves a panel no more than this, mm, has none.
    #[serde(default = "min_panel")]
    pub min_panel: f64,
}

fn min_panel() -> f64 {
    MIN_PANEL
}

/// What one curtain wall does apart from its type.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurtainEdits {
    /// Remove Last Interior Grid: while the segment at an end is too small, the grid line before it goes and the next segment
    /// takes it in. The wall stays as long as it is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub absorb_columns: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub absorb_rows: bool,
    /// The parts of this wall that aren't as its pattern says, each with an id and a GUID of its own. One whose place isn't on
    /// the wall now sleeps, and wakes when the place comes back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartEdit>,
    /// The cells walls stand in ([`PanelOf`]): what the walls say, kept here so a layout doesn't look through the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub walled: Vec<(i64, i64)>,
}

impl CurtainEdits {
    fn is_empty(&self) -> bool {
        !self.absorb_columns && !self.absorb_rows && self.parts.is_empty() && self.walled.is_empty()
    }
}

/// One part given another type, or with none, none at all.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartEdit {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    pub at: Address,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
}

impl Default for Address {
    fn default() -> Self {
        Address::Cell { column: 0, row: 0 }
    }
}

impl crate::library::HasPart for PartEdit {
    fn ident(&mut self) -> (&mut crate::ids::Id, &mut crate::ids::Guid) {
        (&mut self.id, &mut self.guid)
    }
}

/// What makes a wall a curtain wall's panel: which wall and which cell. It stays fitted to the cell after every command
/// ([`sync`]), joins nothing and bounds no room.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelOf {
    pub host: Guid,
    pub column: i64,
    pub row: i64,
}

/// A cell a wall stands in, and what the mullions leave of it: along and up, from and to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hole {
    pub column: i64,
    pub row: i64,
    pub clear: [f64; 4],
}

/// A stretch of a wall one segment of its pattern covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub from: f64,
    pub to: f64,
    /// Which segment of the pattern it is.
    pub seg: usize,
    /// Which it is counting from where the pattern is laid from: what stays its name as the wall changes.
    pub index: i64,
    /// All of the segment; not what an end of the wall left of it.
    pub whole: bool,
}

/// Which kind of part a shape is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PartKind {
    Panel,
    Mullion,
}

/// One shape many parts of a wall may share: never shown as a type, it's what lets identical parts be drawn once.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub kind: PartKind,
    pub type_name: String,
    /// Its size along the wall, up it and across it, mm.
    pub size: [f64; 3],
    /// A mitred mullion's face on the wall, counter-clockwise, as (along, up) from its centre; none for a box.
    pub face: Option<Quad>,
}

/// Four corners on a wall's face.
pub type Quad = [[f64; 2]; 4];

impl Shape {
    /// Its triangles from its centre, as (along, across, up), turned outward: for a shape with a [`Shape::face`].
    pub fn tris(&self) -> Vec<[[f64; 3]; 3]> {
        let Some(q) = self.face else { return Vec::new() };
        let half = self.size[2] / 2.0;
        let at = |c: [f64; 2], across: f64| [c[0], across, c[1]];
        let (near, far) = (q.map(|c| at(c, -half)), q.map(|c| at(c, half)));
        let mut out = vec![[near[0], near[1], near[2]], [near[0], near[2], near[3]], [far[0], far[2], far[1]], [far[0], far[3], far[2]]];
        for i in 0..4 {
            let j = (i + 1) % 4;
            out.push([near[i], far[i], far[j]]);
            out.push([near[i], far[j], near[j]]);
        }
        out
    }
}

/// Where a part is on its pattern, its name while it has no id of its own: columns and rows by [`Span::index`]. A border is
/// an edge past the last index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "part", rename_all = "lowercase", deny_unknown_fields)]
pub enum Address {
    #[serde(rename = "panel")]
    Cell {
        column: i64,
        row: i64,
    },
    Vertical {
        column: i64,
        row: i64,
    },
    Horizontal {
        column: i64,
        row: i64,
    },
}

/// A panel or a mullion placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub shape: u32,
    /// Its centre: along the wall from its start, up from its base, and toward its exterior from the grid's plane
    /// ([`grid_plane`]).
    pub centre: [f64; 3],
    pub address: Address,
}

/// Which way a wall is too short or too low for what its pattern puts at an end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    Columns,
    Rows,
}

/// What a wall's panels and mullions are, worked out from its type and its size.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// The cells walls stand in.
    pub walls: Vec<Hole>,
    pub columns: Vec<Span>,
    pub rows: Vec<Span>,
    pub shapes: Vec<Shape>,
    pub parts: Vec<Part>,
    /// The ways an end's segment can't hold a panel.
    pub malformed: Vec<Way>,
}

impl Layout {
    /// How far along the wall a part reaches, from and to.
    pub fn along(&self, p: &Part) -> (f64, f64) {
        let half = self.shapes.get(p.shape as usize).map_or(0.0, |s| s.size[0] / 2.0);
        (p.centre[0] - half, p.centre[0] + half)
    }
}

/// A wall's type's pattern, if it's a curtain wall.
pub fn pattern<'a>(m: &'a Model, w: &Wall) -> Option<&'a CurtainLayout> {
    m.types.wall.get(&w.type_name).and_then(|t| t.curtain.as_ref())
}

/// What a wall too short or too low for its pattern's last segment is told. Agents and the desktop's message say the same.
pub const MALFORMED: &str = ui_msg!("Due to the defined wall length the last grid segment draws a malformed panel");

/// The length nearest `length` a Fixed Grid draws: a whole number of its segments, at least its first.
pub fn snap_length(d: &Direction, length: f64) -> f64 {
    let module: f64 = d.segments.iter().sum();
    if !(module > 0.0 && module.is_finite() && length.is_finite() && length > 0.0) {
        return length;
    }
    // Whole modules below it, and then each segment's end on the way past it.
    let mut at = (length / module).floor().max(0.0) * module;
    let mut best = if at > 0.0 { at } else { d.segments.first().copied().unwrap_or(length) };
    for size in &d.segments {
        at += size;
        if (at - length).abs() < (best - length).abs() {
            best = at;
        }
    }
    best
}

/// The walls whose segment at an end can't hold a panel, by id.
pub fn malformed_walls(m: &Model) -> Vec<u64> {
    if !m.types.wall.values().any(|t| t.curtain.is_some()) {
        return Vec::new();
    }
    let walls = m.elements.iter().filter_map(|(id, e)| match e {
        Element::Wall(w) if pattern(m, w).is_some() => Some((*id, w)),
        _ => None,
    });
    walls.filter(|(_, w)| !layout(m, w).malformed.is_empty()).map(|(id, _)| id).collect()
}

/// How many stretches a pattern makes of `length`, at most.
fn stretches(d: &Direction, length: f64) -> f64 {
    let module: f64 = d.segments.iter().sum();
    if module > 0.0 { (length / module + 2.0) * d.segments.len() as f64 } else { 0.0 }
}

/// How many cells wall `w` has, at most.
fn cells(m: &Model, l: &CurtainLayout, w: &Wall) -> f64 {
    stretches(&l.columns, Axis::of(w).length()) * stretches(&l.rows, m.own_top(w) - m.own_base(w))
}

/// A pattern's stretches over `length`: repeated from where it's justified, and cut by the ends. Nothing for a pattern or a
/// length that makes no sense, which `check` refuses.
pub fn divide(d: &Direction, length: f64) -> Vec<Span> {
    let module: f64 = d.segments.iter().sum();
    let good = d.segments.iter().all(|s| s.is_finite() && *s > 0.0);
    if !(good && module > 0.0 && length.is_finite() && length > SAME && stretches(d, length) <= MAX_CELLS) {
        return Vec::new();
    }
    let origin = match d.justify {
        Justify::Start => 0.0,
        Justify::End => length,
        Justify::Centre => (length - ((length + SAME) / module).floor().max(1.0) * module) / 2.0,
        Justify::CentreLine => length / 2.0,
    };
    let count = d.segments.len() as i64;
    let mut out = Vec::new();
    let mut k = (-origin / module).floor() as i64;
    loop {
        let mut at = origin + k as f64 * module;
        if at >= length - SAME {
            return out;
        }
        for (seg, size) in d.segments.iter().enumerate() {
            let (a, b) = (at, at + size);
            at = b;
            if b > SAME && a < length - SAME {
                let whole = a >= -SAME && b <= length + SAME;
                let from = if a <= SAME { 0.0 } else { a };
                let to = if b >= length - SAME { length } else { b };
                out.push(Span { from, to, seg, index: k * count + seg as i64, whole });
            }
        }
        k += 1;
    }
}

/// A mullion across a run of its line: what it is, from `lo` to `hi` square to its line, and how far it reaches across the wall.
#[derive(Clone, Copy)]
struct Bar<'a> {
    pick: &'a str,
    join: Join,
    lo: f64,
    hi: f64,
    across: (f64, f64),
    offset: f64,
    depth: f64,
}

impl Bar<'_> {
    fn of<'a>(m: &'a Model, name: Option<&'a str>, join: Join, at: At) -> Option<Bar<'a>> {
        let name = name?;
        let t = m.types.mullion.get(name)?;
        let (w, x) = (t.width(), t.offset_x);
        let lo = match at {
            At::Line(p) => p - w / 2.0,
            At::Start => 0.0,
            At::End(length) => length - w,
        } + x;
        let half = t.depth() / 2.0;
        Some(Bar { pick: name, join, lo, hi: lo + w, across: (t.offset_y - half, t.offset_y + half), offset: t.offset_y, depth: t.depth() })
    }

    fn middle(&self) -> f64 {
        (self.lo + self.hi) / 2.0
    }

    /// Whether it runs through `o` where they cross: being the deeper, of one depth the wider, and of one size if `tie`.
    fn runs_through(&self, o: &Bar, tie: bool) -> bool {
        let (depth, width) = (self.depth - o.depth, (self.hi - self.lo) - (o.hi - o.lo));
        if depth.abs() >= SAME {
            depth > 0.0
        } else if width.abs() >= SAME {
            width > 0.0
        } else {
            tie
        }
    }

    /// Whether a mitre can join it to `o`: both ask for one and they're the same size in the same place across the wall.
    fn mitres_with(&self, o: &Bar) -> bool {
        self.join == Join::Mitre
            && o.join == Join::Mitre
            && (self.hi - self.lo - (o.hi - o.lo)).abs() < SAME
            && (self.depth - o.depth).abs() < SAME
            && (self.offset - o.offset).abs() < SAME
    }
}

/// The diagonal two mullions meeting at a corner are cut on: its outer and inner points, as (along, up).
#[derive(Clone, Copy)]
struct Corner {
    outer: [f64; 2],
    inner: [f64; 2],
}

impl Corner {
    /// Where the diagonal is at `along`, one of the vertical bar's two sides.
    fn up_at(&self, along: f64) -> f64 {
        if (along - self.outer[0]).abs() < (along - self.inner[0]).abs() { self.outer[1] } else { self.inner[1] }
    }

    /// Where the diagonal is at `up`, one of the horizontal bar's two sides.
    fn along_at(&self, up: f64) -> f64 {
        if (up - self.outer[1]).abs() < (up - self.inner[1]).abs() { self.outer[0] } else { self.inner[0] }
    }
}

/// Where mullions meet in a mitre, by the lines they meet on: only at a corner, one vertical and one horizontal bar
/// and nothing else, so that a T or a crossing stays butt whatever its bars ask for.
fn corners(vertical: &Lines, horizontal: &Lines) -> HashMap<(usize, usize), Corner> {
    let mut out = HashMap::new();
    for (i, line) in vertical.iter().enumerate() {
        for (j, across) in horizontal.iter().enumerate() {
            let below = j.checked_sub(1).and_then(|r| line.get(r)).and_then(Option::as_ref);
            let above = line.get(j).and_then(Option::as_ref);
            let left = i.checked_sub(1).and_then(|c| across.get(c)).and_then(Option::as_ref);
            let right = across.get(i).and_then(Option::as_ref);
            let (v, goes_up) = match (below, above) {
                (Some(b), None) => (b, false),
                (None, Some(a)) => (a, true),
                _ => continue,
            };
            let (h, goes_right) = match (left, right) {
                (Some(l), None) => (l, false),
                (None, Some(r)) => (r, true),
                _ => continue,
            };
            if v.mitres_with(h) {
                let (out_u, in_u) = if goes_right { (v.lo, v.hi) } else { (v.hi, v.lo) };
                let (out_v, in_v) = if goes_up { (h.lo, h.hi) } else { (h.hi, h.lo) };
                out.insert((i, j), Corner { outer: [out_u, out_v], inner: [in_u, in_v] });
            }
        }
    }
    out
}

/// Where a mullion's line is: within the wall, or at one of its ends, inside which it stands.
#[derive(Clone, Copy)]
enum At {
    Line(f64),
    Start,
    End(f64),
}

/// `range` less the mullions that reach into it at `across`: each takes what's on its side.
fn narrowed<'a>(range: (f64, f64), across: (f64, f64), bars: impl Iterator<Item = &'a Bar<'a>>) -> (f64, f64) {
    let middle = (range.0 + range.1) / 2.0;
    let (mut lo, mut hi) = range;
    for b in bars {
        if b.across.0 < across.1 && across.0 < b.across.1 && b.lo < range.1 && range.0 < b.hi {
            if b.middle() <= middle {
                lo = lo.max(b.hi);
            } else {
                hi = hi.min(b.lo);
            }
        }
    }
    (lo, hi)
}

/// Mullions along lines `0..=spans.len()`, each a list of the runs across them.
type Lines<'a> = Vec<Vec<Option<Bar<'a>>>>;

/// How far what a mullion ending at a crossing crosses reaches, lengthwise of the mullion; none if any of it runs through the
/// mullion instead, which then stops there, or none is in its way across the wall.
fn past<'a, 'b: 'a>(b: &Bar, crossing: impl Iterator<Item = &'a Bar<'b>>, wins: impl Fn(&Bar) -> bool) -> Option<(f64, f64)> {
    let mut reach = None;
    for o in crossing.filter(|o| o.across.0 < b.across.1 && b.across.0 < o.across.1) {
        if wins(o) {
            return None;
        }
        let (lo, hi) = reach.unwrap_or((o.lo, o.hi));
        reach = Some((o.lo.min(lo), o.hi.max(hi)));
    }
    reach
}

/// Two mullions can't share a place: one on a line between that would run into its neighbour goes.
fn drop_clashes(lines: &mut Lines) {
    let last = lines.len().saturating_sub(1);
    for run in 0..lines.first().map_or(0, Vec::len) {
        for i in 1..last {
            let Some(b) = lines[i][run] else { continue };
            let before = lines[..i].iter().rev().find_map(|l| l[run]);
            let after = lines[last][run].filter(|_| i + 1 == last);
            if before.is_some_and(|p| b.lo < p.hi) || after.is_some_and(|n| n.lo < b.hi) {
                lines[i][run] = None;
            }
        }
    }
}

/// What a wall changes of its pattern: the types its edited parts take, by where they are, and the cells walls stand in.
#[derive(Default)]
struct Edits<'a> {
    parts: HashMap<Address, Option<&'a str>>,
    walled: Vec<(i64, i64)>,
}

impl<'a> Edits<'a> {
    fn of(e: Option<&'a CurtainEdits>, walled: &[(i64, i64)]) -> Self {
        let parts = e.map_or_else(HashMap::new, |e| e.parts.iter().map(|p| (p.at, p.type_name.as_deref())).collect());
        Edits { parts, walled: walled.to_vec() }
    }
}

/// A mullion cut to `ring`, its corners on the wall's face: centred on what bounds them.
fn cut_part(out: &mut Shapes, b: &Bar, ring: Quad, address: Address) -> Part {
    let bound = |axis: usize| ring.iter().fold((f64::MAX, f64::MIN), |(lo, hi), c| (lo.min(c[axis]), hi.max(c[axis])));
    let ((u0, u1), (v0, v1)) = (bound(0), bound(1));
    let centre = [(u0 + u1) / 2.0, (v0 + v1) / 2.0];
    let face = ring.map(|c| [c[0] - centre[0], c[1] - centre[1]]);
    let shape = out.of(PartKind::Mullion, b.pick, [u1 - u0, v1 - v0, b.depth], Some(face));
    Part { shape, centre: [centre[0], centre[1], b.offset], address }
}

/// What a wall `length` long and `height` tall is made of, its spans given.
fn build<'a>(
    m: &'a Model,
    l: &'a CurtainLayout,
    edits: &Edits<'a>,
    columns: Vec<Span>,
    rows: Vec<Span>,
    length: f64,
    height: f64,
) -> Built {
    let (nc, nr) = (columns.len(), rows.len());
    let index = |spans: &[Span], i: usize| spans.get(i).map_or_else(|| spans.last().map_or(0, |s| s.index + 1), |s| s.index);
    let bar = |at: Address, p: Option<&'a MullionPick>, place: At| {
        let name = edits.parts.get(&at).copied().unwrap_or(p.map(|x| x.type_name.as_str()));
        Bar::of(m, name, p.map_or(Join::Butt, |x| x.join), place)
    };
    let mut vertical: Lines = Vec::with_capacity(nc + 1);
    for i in 0..=nc {
        let mut runs = Vec::with_capacity(nr);
        for r in &rows {
            let (p, place) = match columns.get(i) {
                _ if i == 0 => (l.borders.left.as_ref(), At::Start),
                Some(c) => (l.vertical.get(r.seg).and_then(|x| x.get(c.seg)).and_then(Option::as_ref), At::Line(c.from)),
                None => (l.borders.right.as_ref(), At::End(length)),
            };
            runs.push(bar(Address::Vertical { column: index(&columns, i), row: r.index }, p, place));
        }
        vertical.push(runs);
    }
    let mut horizontal: Lines = Vec::with_capacity(nr + 1);
    for j in 0..=nr {
        let mut runs = Vec::with_capacity(nc);
        for c in &columns {
            let (p, place) = match rows.get(j) {
                _ if j == 0 => (l.borders.bottom.as_ref(), At::Start),
                Some(r) => (l.horizontal.get(r.seg).and_then(|x| x.get(c.seg)).and_then(Option::as_ref), At::Line(r.from)),
                None => (l.borders.top.as_ref(), At::End(height)),
            };
            runs.push(bar(Address::Horizontal { column: c.index, row: index(&rows, j) }, p, place));
        }
        horizontal.push(runs);
    }
    drop_clashes(&mut vertical);
    drop_clashes(&mut horizontal);
    let mitres = corners(&vertical, &horizontal);

    // Those near line `i`'s side: a span's mullions are on its own lines or, one gone, the next.
    let near = |lines: &'_ [Vec<Option<Bar<'_>>>], i: usize| -> std::ops::RangeInclusive<usize> {
        i.saturating_sub(1)..=(i + 2).min(lines.len().saturating_sub(1))
    };
    let mut out = Shapes::default();
    let mut parts = Vec::new();
    let mut walls = Vec::new();

    // Vertical mullions.
    for (i, line) in vertical.iter().enumerate() {
        for (r, bar) in line.iter().enumerate() {
            let Some(b) = bar else { continue };
            let span = (rows[r].from, rows[r].to);
            let cols = i.saturating_sub(1)..=i.min(nc.saturating_sub(1));
            let bars: Vec<&Bar> = cols
                .flat_map(|c| near(&horizontal, r).filter_map(|j| horizontal[j][c].as_ref()).collect::<Vec<_>>())
                .filter(|h| h.runs_through(b, l.continuous == Continuous::Horizontal))
                .collect();
            let (mut v0, mut v1) = narrowed(span, b.across, bars.iter().copied());
            // Ending at a crossing it runs through, on to the far face of what it crosses.
            let tie = l.continuous == Continuous::Horizontal;
            let across = |j: usize| -> Vec<&Bar> {
                (i.saturating_sub(1)..=i.min(nc.saturating_sub(1))).filter_map(|c| horizontal.get(j)?.get(c)?.as_ref()).collect()
            };
            if (r == 0 || line[r - 1].is_none())
                && let Some((lo, _)) = past(b, across(r).into_iter(), |h| h.runs_through(b, tie))
            {
                v0 = v0.min(lo);
            }
            if line.get(r + 1).is_none_or(Option::is_none)
                && let Some((_, hi)) = past(b, across(r + 1).into_iter(), |h| h.runs_through(b, tie))
            {
                v1 = v1.max(hi);
            }
            let (low, high) = (mitres.get(&(i, r)), mitres.get(&(i, r + 1)));
            let at = |along: f64| (low.map_or(v0, |c| c.up_at(along)), high.map_or(v1, |c| c.up_at(along)));
            let (left, right) = (at(b.lo), at(b.hi));
            let mitred = (low.is_some() || high.is_some()) && left.0 < left.1 - SAME && right.0 < right.1 - SAME;
            let address = Address::Vertical { column: index(&columns, i), row: rows[r].index };
            if mitred {
                let ring = [[b.lo, left.0], [b.hi, right.0], [b.hi, right.1], [b.lo, left.1]];
                parts.push(cut_part(&mut out, b, ring, address));
            } else if v1 - v0 > SAME {
                let shape = out.of(PartKind::Mullion, b.pick, [b.hi - b.lo, v1 - v0, b.depth], None);
                parts.push(Part { shape, centre: [b.middle(), (v0 + v1) / 2.0, b.offset], address });
            }
        }
    }
    // Horizontal mullions.
    for (j, line) in horizontal.iter().enumerate() {
        for (c, bar) in line.iter().enumerate() {
            let Some(b) = bar else { continue };
            let span = (columns[c].from, columns[c].to);
            let rows_near = j.saturating_sub(1)..=j.min(nr.saturating_sub(1));
            let bars: Vec<&Bar> = rows_near
                .flat_map(|r| near(&vertical, c).filter_map(|i| vertical[i][r].as_ref()).collect::<Vec<_>>())
                .filter(|v| v.runs_through(b, l.continuous == Continuous::Vertical))
                .collect();
            let (mut u0, mut u1) = narrowed(span, b.across, bars.iter().copied());
            let tie = l.continuous == Continuous::Vertical;
            let across = |i: usize| -> Vec<&Bar> {
                (j.saturating_sub(1)..=j.min(nr.saturating_sub(1))).filter_map(|r| vertical.get(i)?.get(r)?.as_ref()).collect()
            };
            if (c == 0 || line[c - 1].is_none())
                && let Some((lo, _)) = past(b, across(c).into_iter(), |v| v.runs_through(b, tie))
            {
                u0 = u0.min(lo);
            }
            if line.get(c + 1).is_none_or(Option::is_none)
                && let Some((_, hi)) = past(b, across(c + 1).into_iter(), |v| v.runs_through(b, tie))
            {
                u1 = u1.max(hi);
            }
            let (first, last) = (mitres.get(&(c, j)), mitres.get(&(c + 1, j)));
            let at = |up: f64| (first.map_or(u0, |x| x.along_at(up)), last.map_or(u1, |x| x.along_at(up)));
            let (under, over) = (at(b.lo), at(b.hi));
            let mitred = (first.is_some() || last.is_some()) && under.0 < under.1 - SAME && over.0 < over.1 - SAME;
            let address = Address::Horizontal { column: columns[c].index, row: index(&rows, j) };
            if mitred {
                let ring = [[under.0, b.lo], [under.1, b.lo], [over.1, b.hi], [over.0, b.hi]];
                parts.push(cut_part(&mut out, b, ring, address));
            } else if u1 - u0 > SAME {
                let shape = out.of(PartKind::Mullion, b.pick, [u1 - u0, b.hi - b.lo, b.depth], None);
                parts.push(Part { shape, centre: [(u0 + u1) / 2.0, b.middle(), b.offset], address });
            }
        }
    }
    // Panels, and what's too small to hold one.
    let (mut short_columns, mut short_rows, mut too_small) = (Vec::new(), Vec::new(), false);
    for (r, row) in rows.iter().enumerate() {
        for (c, column) in columns.iter().enumerate() {
            let by_columns = near(&vertical, c).filter_map(|i| vertical[i][r].as_ref()).collect::<Vec<_>>();
            let by_rows = near(&horizontal, r).filter_map(|j| horizontal[j][c].as_ref()).collect::<Vec<_>>();
            // A wall stands between the mullions, however deep it is.
            if edits.walled.contains(&(column.index, row.index)) {
                let across = (f64::MIN, f64::MAX);
                let (u0, u1) = narrowed((column.from, column.to), across, by_columns.into_iter());
                let (v0, v1) = narrowed((row.from, row.to), across, by_rows.into_iter());
                if u1 - u0 > l.min_panel && v1 - v0 > l.min_panel {
                    walls.push(Hole { column: column.index, row: row.index, clear: [u0, u1, v0, v1] });
                }
                continue;
            }
            let at = Address::Cell { column: column.index, row: row.index };
            let name = match edits.parts.get(&at) {
                Some(name) => *name,
                None => l.panels.get(row.seg).and_then(|x| x.get(column.seg)).and_then(|n| n.as_deref()),
            };
            let Some((name, t)) = name.and_then(|n| m.types.curtain_panel.get(n).map(|t| (n, t))) else { continue };
            let across = (t.offset - t.thickness / 2.0, t.offset + t.thickness / 2.0);
            let (u0, u1) = narrowed((column.from, column.to), across, by_columns.into_iter());
            let (v0, v1) = narrowed((row.from, row.to), across, by_rows.into_iter());
            let (narrow, low) = (u1 - u0 <= l.min_panel, v1 - v0 <= l.min_panel);
            if narrow || low {
                if narrow {
                    if column.whole {
                        too_small = true
                    } else if !short_columns.contains(&c) {
                        short_columns.push(c)
                    }
                }
                if low {
                    if row.whole {
                        too_small = true
                    } else if !short_rows.contains(&r) {
                        short_rows.push(r)
                    }
                }
                continue;
            }
            let shape = out.of(PartKind::Panel, name, [u1 - u0, v1 - v0, t.thickness], None);
            let address = Address::Cell { column: column.index, row: row.index };
            parts.push(Part { shape, centre: [(u0 + u1) / 2.0, (v0 + v1) / 2.0, t.offset], address });
        }
    }
    Built { columns, rows, shapes: out.shapes, parts, walls, short_columns, short_rows, too_small }
}

struct Built {
    columns: Vec<Span>,
    rows: Vec<Span>,
    shapes: Vec<Shape>,
    parts: Vec<Part>,
    walls: Vec<Hole>,
    short_columns: Vec<usize>,
    short_rows: Vec<usize>,
    too_small: bool,
}

/// What makes two shapes one: kind, type, and size and face to within [`SAME`].
type ShapeKey = (PartKind, String, [i64; 3], Option<[[i64; 2]; 4]>);

/// The shapes of a wall's parts, each kept once: what makes a thousand identical panels one.
#[derive(Default)]
struct Shapes {
    shapes: Vec<Shape>,
    known: HashMap<ShapeKey, u32>,
}

impl Shapes {
    fn of(&mut self, kind: PartKind, name: &str, size: [f64; 3], face: Option<Quad>) -> u32 {
        let rounded = |x: f64| (x / SAME).round() as i64;
        let key = (kind, name.to_string(), size.map(rounded), face.map(|q| q.map(|c| c.map(rounded))));
        if let Some(&s) = self.known.get(&key) {
            return s;
        }
        let s = self.shapes.len() as u32;
        self.shapes.push(Shape { kind, type_name: name.to_string(), size, face });
        self.known.insert(key, s);
        s
    }
}

/// The spans with the too-small ones at the ends taken in by their neighbours. Whether any was.
fn absorbed(spans: &mut Vec<Span>, short: &[usize]) -> bool {
    let mut any = false;
    for &i in short.iter().rev() {
        let n = spans.len();
        if n >= 2 && i + 1 == n {
            let to = spans[i].to;
            spans.truncate(i);
            if let Some(s) = spans.last_mut() {
                s.to = to;
                s.whole = false;
            }
            any = true;
        } else if n >= 2 && i == 0 {
            let from = spans[0].from;
            spans.remove(0);
            spans[0].from = from;
            spans[0].whole = false;
            any = true;
        }
    }
    any
}

/// What wall `w` is made of; nothing if it isn't a curtain wall, or isn't one that can be built.
pub fn layout(m: &Model, w: &Wall) -> Layout {
    let walled = w.curtain.as_ref().map_or(&[][..], |e| &e.walled[..]);
    layout_with(m, w, walled)
}

/// As [`layout`], with walls standing in the cells `walled`.
fn layout_with(m: &Model, w: &Wall, walled: &[(i64, i64)]) -> Layout {
    let Some(l) = pattern(m, w) else { return Layout::default() };
    if cells(m, l, w) > MAX_CELLS {
        return Layout::default();
    }
    let (length, height) = (Axis::of(w).length(), m.own_top(w) - m.own_base(w));
    let (mut columns, mut rows) = (divide(&l.columns, length), divide(&l.rows, height));
    if columns.is_empty() || rows.is_empty() {
        return Layout::default();
    }
    let edits = Edits::of(w.curtain.as_ref(), walled);
    let (absorb_columns, absorb_rows) = w.curtain.as_ref().map_or((false, false), |e| (e.absorb_columns, e.absorb_rows));
    let mut built = build(m, l, &edits, columns.clone(), rows.clone(), length, height);
    let again_columns = absorb_columns && absorbed(&mut columns, &built.short_columns);
    let again_rows = absorb_rows && absorbed(&mut rows, &built.short_rows);
    if again_columns || again_rows {
        built = build(m, l, &edits, columns, rows, length, height);
    }
    let mut malformed = Vec::new();
    if !built.short_columns.is_empty() {
        malformed.push(Way::Columns);
    }
    if !built.short_rows.is_empty() {
        malformed.push(Way::Rows);
    }
    Layout { walls: built.walls, columns: built.columns, rows: built.rows, shapes: built.shapes, parts: built.parts, malformed }
}

/// Where the grid's plane lies from the wall's location line, toward the exterior.
pub fn grid_plane(m: &Model, w: &Wall) -> f64 {
    let front = pattern(m, w).and_then(|l| envelope(m, l)).map_or(0.0, |e| e.0);
    crate::walls::faces(m, w).0 - front
}

/// How far the parts a pattern uses reach from the grid's plane: toward the exterior, and toward the interior. What makes its
/// thickness, their sum. None when it uses none.
pub fn envelope(m: &Model, l: &CurtainLayout) -> Option<(f64, f64)> {
    let panels = l.panels.iter().flatten().flatten().filter_map(|n| m.types.curtain_panel.get(n)).map(|t| (t.offset, t.thickness));
    let mullions = l.picks().filter_map(|p| m.types.mullion.get(&p.type_name)).map(|t| (t.offset_y, t.depth()));
    panels.chain(mullions).fold(None, |e: Option<(f64, f64)>, (offset, size)| {
        let (front, back) = (offset + size / 2.0, size / 2.0 - offset);
        Some(e.map_or((front, back), |(f, b)| (f.max(front), b.max(back))))
    })
}

impl CurtainEdits {
    /// The types its edited parts name.
    pub(crate) fn names_mut(&mut self, f: &mut dyn FnMut(&Kind, &mut String)) {
        let (panel, mullion) = (Kind::Type("curtainpanel".into()), Kind::Type("mullion".into()));
        for e in &mut self.parts {
            let kind = if matches!(e.at, Address::Cell { .. }) { &panel } else { &mullion };
            if let Some(n) = &mut e.type_name {
                f(kind, n);
            }
        }
    }
}

/// How wide a place with no mullion in it is marked, mm.
const MARK: f64 = 50.0;

/// Which place a part is, by what a command calls it.
pub fn address(part: &str, column: i64, row: i64) -> Option<Address> {
    match part {
        "panel" => Some(Address::Cell { column, row }),
        "vertical" => Some(Address::Vertical { column, row }),
        "horizontal" => Some(Address::Horizontal { column, row }),
        _ => None,
    }
}

/// Where the line from `a` through `b` meets `w`'s grid plane: along the wall and up from its base.
pub fn on_plane(m: &Model, w: &Wall, a: [f64; 3], b: [f64; 3]) -> Option<(f64, f64)> {
    let axis = Axis::of(w);
    let (o, u) = (axis.at(0.0, grid_plane(m, w)), axis.tangent(0.0));
    let n = u.perp();
    let off = |p: [f64; 3]| (P2(p[0], p[1]) - o).dot(n);
    let (da, db) = (off(a), off(b));
    if (da - db).abs() < 1e-9 {
        return None;
    }
    let t = da / (da - db);
    let at = |k: usize| a[k] + (b[k] - a[k]) * t;
    Some(((P2(at(0), at(1)) - o).dot(u), at(2) - m.own_base(w)))
}

/// What marks a place on a plan: its part's or its extent's corners, from above.
pub fn plan_ring(m: &Model, w: &Wall, at: Address) -> Option<[P2; 4]> {
    let l = layout(m, w);
    let [u0, u1, ..] = l.extent(at)?;
    let part = l.parts.iter().find(|p| p.address == at);
    let shape = part.and_then(|p| l.shapes.get(p.shape as usize));
    let (middle, half) = match (part, shape) {
        (Some(p), Some(s)) => (p.centre[2], s.size[2] / 2.0),
        _ => (0.0, MARK / 2.0),
    };
    let (axis, plane) = (Axis::of(w), grid_plane(m, w));
    let c = |along: f64, side: f64| axis.at(along, plane + middle + side);
    Some([c(u0, -half), c(u1, -half), c(u1, half), c(u0, half)])
}

/// What marks a place in 3D: its extent's corners on the wall's grid plane.
pub fn face_ring(m: &Model, w: &Wall, at: Address) -> Option<[[f64; 3]; 4]> {
    let [u0, u1, v0, v1] = layout(m, w).extent(at)?;
    let (axis, plane, base) = (Axis::of(w), grid_plane(m, w), m.own_base(w));
    let c = |along: f64, up: f64| {
        let q = axis.at(along, plane);
        [q.0, q.1, base + up]
    };
    Some([c(u0, v0), c(u1, v0), c(u1, v1), c(u0, v1)])
}

impl Layout {
    /// What's at a point of the wall, along it and up from its base: the mullions' places within `reach` of it, then its cell.
    pub fn at(&self, along: f64, up: f64, reach: f64) -> Vec<Address> {
        let within = |spans: &[Span], x: f64| {
            spans.iter().position(|s| s.from <= x && x < s.to).or_else(|| spans.last().filter(|s| x == s.to).map(|_| spans.len() - 1))
        };
        let (Some(c), Some(r)) = (within(&self.columns, along), within(&self.rows, up)) else { return Vec::new() };
        let (column, row) = (&self.columns[c], &self.rows[r]);
        let next = |spans: &[Span], i: usize, s: &Span| spans.get(i + 1).map_or(s.index + 1, |n| n.index);
        let mut out = Vec::new();
        if (along - column.from).abs() <= reach {
            out.push(Address::Vertical { column: column.index, row: row.index });
        }
        if (column.to - along).abs() <= reach {
            out.push(Address::Vertical { column: next(&self.columns, c, column), row: row.index });
        }
        if (up - row.from).abs() <= reach {
            out.push(Address::Horizontal { column: column.index, row: row.index });
        }
        if (row.to - up).abs() <= reach {
            out.push(Address::Horizontal { column: column.index, row: next(&self.rows, r, row) });
        }
        out.push(Address::Cell { column: column.index, row: row.index });
        out
    }

    /// What a place takes up: along, then up, from and to. Its part or the wall in it, or with neither, its cell or its stretch
    /// of line.
    pub fn extent(&self, at: Address) -> Option<[f64; 4]> {
        if !self.has(at) {
            return None;
        }
        if let Some(p) = self.parts.iter().find(|p| p.address == at)
            && let Some(s) = self.shapes.get(p.shape as usize)
        {
            let [da, dv, _] = s.size.map(|x| x / 2.0);
            return Some([p.centre[0] - da, p.centre[0] + da, p.centre[1] - dv, p.centre[1] + dv]);
        }
        let span = |spans: &'_ [Span], i: i64| spans.iter().find(|s| s.index == i).map(|s| (s.from, s.to));
        let line = |spans: &'_ [Span], i: i64| span(spans, i).map(|s| s.0).or_else(|| spans.last().map(|s| s.to));
        let half = MARK / 2.0;
        Some(match at {
            Address::Cell { column, row } => {
                if let Some(h) = self.walls.iter().find(|h| (h.column, h.row) == (column, row)) {
                    return Some(h.clear);
                }
                let ((u0, u1), (v0, v1)) = (span(&self.columns, column)?, span(&self.rows, row)?);
                [u0, u1, v0, v1]
            }
            Address::Vertical { column, row } => {
                let (x, (v0, v1)) = (line(&self.columns, column)?, span(&self.rows, row)?);
                [x - half, x + half, v0, v1]
            }
            Address::Horizontal { column, row } => {
                let ((u0, u1), y) = (span(&self.columns, column)?, line(&self.rows, row)?);
                [u0, u1, y - half, y + half]
            }
        })
    }

    /// Whether the place is on the wall now, a part there or none.
    pub fn has(&self, at: Address) -> bool {
        let (columns, rows) = (&self.columns, &self.rows);
        let on = |spans: &[Span], i: i64| spans.iter().any(|s| s.index == i);
        let past = |spans: &[Span], i: i64| spans.last().is_some_and(|s| s.index + 1 == i);
        match at {
            Address::Cell { column, row } => on(columns, column) && on(rows, row),
            Address::Vertical { column, row } => (on(columns, column) || past(columns, column)) && on(rows, row),
            Address::Horizontal { column, row } => on(columns, column) && (on(rows, row) || past(rows, row)),
        }
    }
}

impl CurtainLayout {
    /// The mullions it picks, of its edges and its borders.
    pub fn picks(&self) -> impl Iterator<Item = &MullionPick> {
        let edges = self.vertical.iter().chain(&self.horizontal).flatten().flatten();
        let b = &self.borders;
        edges.chain([&b.left, &b.right, &b.bottom, &b.top].into_iter().flatten())
    }

    /// Each panel's and mullion's type it names, given to `f` with its kind, which may put another in its place.
    pub(crate) fn names_mut(&mut self, f: &mut dyn FnMut(&Kind, &mut String)) {
        let (panel, mullion) = (Kind::Type("curtainpanel".into()), Kind::Type("mullion".into()));
        for n in self.panels.iter_mut().flatten().flatten() {
            f(&panel, n);
        }
        let b = &mut self.borders;
        let edges = self.vertical.iter_mut().chain(self.horizontal.iter_mut()).flatten().flatten();
        for p in edges.chain([&mut b.left, &mut b.right, &mut b.bottom, &mut b.top].into_iter().flatten()) {
            f(&mullion, &mut p.type_name);
        }
    }

    /// A grid of `columns` and `rows`, `panel` in every cell and `mullion` on every edge.
    fn uniform(columns: Direction, rows: Direction, panel: Option<&str>, mullion: Option<&str>) -> CurtainLayout {
        let (nc, nr) = (columns.segments.len(), rows.segments.len());
        let pick = || mullion.map(|n| MullionPick { type_name: n.to_string(), join: Join::Butt });
        CurtainLayout {
            columns,
            rows,
            panels: vec![vec![panel.map(String::from); nc]; nr],
            vertical: vec![vec![pick(); nc]; nr],
            horizontal: vec![vec![pick(); nc]; nr],
            borders: Borders { left: pick(), right: pick(), bottom: pick(), top: pick() },
            continuous: Continuous::Vertical,
            min_panel: MIN_PANEL,
        }
    }
}

/// A pattern from a `type` command's params: as a project holds it, with `panel` and `mullion` to fill what it leaves out
/// (the project's first unless they're given; null for none).
pub(crate) fn from_params(m: &Model, v: &Value) -> Result<CurtainLayout, String> {
    let mut o = v.as_object().cloned().ok_or("`curtain` is {columns: {segments, fit?, justify?}, rows: {…}, panel?, mullion?, panels?, vertical?, horizontal?, borders?, continuous?, min_panel?}")?;
    let name = |key: &str, category: &str, o: &mut serde_json::Map<String, Value>| match o.remove(key) {
        None => Ok(m.default_type(category).map(String::from)),
        Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => Err(format!("`{key}` is a type's name or null, not {other}")),
    };
    let (panel, mullion) = (name("panel", "curtainpanel", &mut o)?, name("mullion", "mullion", &mut o)?);
    let direction = |key: &str, o: &serde_json::Map<String, Value>| -> Result<Direction, String> {
        let d: Direction =
            serde_json::from_value(o.get(key).cloned().ok_or_else(|| format!("`curtain` needs `{key}`: {{segments: [mm, …]}}"))?)
                .map_err(|e| format!("`{key}`: {e}"))?;
        if d.segments.len() > MAX_SEGMENTS {
            return Err(format!("`{key}` has {} segments; a pattern has at most {MAX_SEGMENTS} each way", d.segments.len()));
        }
        Ok(d)
    };
    let (columns, rows) = (direction("columns", &o)?, direction("rows", &o)?);
    let filled = CurtainLayout::uniform(columns, rows, panel.as_deref(), mullion.as_deref());
    let defaults = serde_json::to_value(&filled).map_err(|e| e.to_string())?;
    for key in ["panels", "vertical", "horizontal"] {
        if !o.contains_key(key) {
            o.insert(key.to_string(), defaults[key].clone());
        }
    }
    let borders = o.entry("borders").or_insert_with(|| json!({}));
    let Some(given) = borders.as_object_mut() else { return Err("`borders` is {left?, right?, bottom?, top?}".into()) };
    for key in ["left", "right", "bottom", "top"] {
        if !given.contains_key(key) {
            given.insert(key.to_string(), defaults["borders"][key].clone());
        }
    }
    serde_json::from_value(Value::Object(o)).map_err(|e| format!("`curtain`: {e}"))
}

/// Whether a curtain wall type's thickness isn't what its panels and mullions make it.
pub(crate) fn out_of_date(m: &Model) -> bool {
    m.types.wall.values().any(|t| t.curtain.as_ref().is_some_and(|l| (t.thickness - thickness(m, l)).abs() > 1e-9))
        || !panel_plan(m).is_empty()
}

fn thickness(m: &Model, l: &CurtainLayout) -> f64 {
    envelope(m, l).map_or(0.0, |(front, back)| front + back)
}

/// Each curtain wall type's thickness made what its panels and mullions make it.
pub(crate) fn sync(m: &mut Model) -> Vec<u64> {
    let worked: Vec<(String, f64)> =
        m.types.wall.iter().filter_map(|(n, t)| t.curtain.as_ref().map(|l| (n.clone(), thickness(m, l)))).collect();
    for (n, t) in worked {
        if let Some(x) = m.types.wall.get_mut(&n) {
            x.thickness = t;
        }
    }
    let plan = panel_plan(m);
    for (id, fitted) in plan.fits {
        if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
            *w = fitted;
        }
    }
    for id in &plan.gone {
        m.elements.remove(*id);
    }
    for (id, walled) in plan.walled {
        if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
            let mut e = w.curtain.take().unwrap_or_default();
            e.walled = walled;
            w.curtain = (!e.is_empty()).then_some(e);
        }
    }
    plan.gone
}

/// A command's reply, saying which walls went with their cells.
pub(crate) fn warn_of_lost(mut v: serde_json::Value, lost: &[u64]) -> serde_json::Value {
    if let (false, Some(o)) = (lost.is_empty(), v.as_object_mut()) {
        let says = format!("the cells of walls {lost:?} are no longer on their curtain walls, so those walls were removed");
        let warning = match o.get("warning").and_then(serde_json::Value::as_str) {
            Some(was) => format!("{was}; {says}"),
            None => says,
        };
        o.insert("warning".into(), warning.into());
    }
    v
}

/// What the walls standing in cells need done, after each command.
#[derive(Default)]
struct PanelPlan {
    /// Walls not as their cells say, as they should be.
    fits: Vec<(u64, Wall)>,
    /// Walls whose cells went, with nothing on them.
    gone: Vec<u64>,
    /// Curtain walls whose `walled` is out of date, and what it should be.
    walled: Vec<(u64, Vec<(i64, i64)>)>,
}

impl PanelPlan {
    fn is_empty(&self) -> bool {
        self.fits.is_empty() && self.gone.is_empty() && self.walled.is_empty()
    }
}

fn panel_plan(m: &Model) -> PanelPlan {
    let mut plan = PanelPlan::default();
    let mut hosts: BTreeMap<Guid, Vec<(u64, &Wall, PanelOf)>> = BTreeMap::new();
    let mut holding = Vec::new();
    for (id, e) in m.elements.iter() {
        let Element::Wall(w) = e else { continue };
        if let Some(p) = w.panel_of {
            hosts.entry(p.host).or_default().push((*id, w, p));
        }
        if w.curtain.as_ref().is_some_and(|c| !c.walled.is_empty()) {
            holding.push(*id);
        }
    }
    if hosts.is_empty() && holding.is_empty() {
        return plan;
    }
    let held: BTreeSet<Guid> = if hosts.is_empty() { BTreeSet::new() } else { m.elements.values().flat_map(Element::refs).collect() };
    for (guid, walls) in &hosts {
        let Some(host_id) = m.elements.id(*guid) else { continue };
        let Some(host) = m.wall(host_id).filter(|h| pattern(m, h).is_some()) else { continue };
        let mut walled: Vec<(i64, i64)> = walls.iter().map(|(_, _, p)| (p.column, p.row)).collect();
        walled.sort_unstable();
        walled.dedup();
        let laid = layout_with(m, host, &walled);
        let (axis, plane) = (Axis::of(host), grid_plane(m, host));
        let mut kept = Vec::new();
        for (id, w, p) in walls {
            match laid.walls.iter().find(|h| (h.column, h.row) == (p.column, p.row)) {
                Some(h) => {
                    let [u0, u1, v0, v1] = h.clear;
                    let fitted = Wall {
                        level: host.level,
                        start: axis.at(u0, plane),
                        end: axis.at(u1, plane),
                        top_level: None,
                        height: v1 - v0,
                        base_offset: host.base_offset + v0,
                        top_offset: 0.0,
                        arc: 0.0,
                        location: Location::Centre,
                        attach_top: None,
                        attach_base: None,
                        room_bounding: false,
                        curtain: None,
                        ..(*w).clone()
                    };
                    if !same_fit(&fitted, w) {
                        plan.fits.push((*id, fitted));
                    }
                    kept.push((p.column, p.row));
                }
                // With something on it, the wall stays for `check` to refuse the command.
                None if held.contains(&m.guid(*id)) => kept.push((p.column, p.row)),
                None => plan.gone.push(*id),
            }
        }
        kept.sort_unstable();
        kept.dedup();
        if host.curtain.as_ref().map_or(&[][..], |e| &e.walled[..]) != kept {
            plan.walled.push((host_id, kept));
        }
    }
    // A curtain wall whose last wall went.
    for id in holding {
        if !hosts.contains_key(&m.guid(id)) {
            plan.walled.push((id, Vec::new()));
        }
    }
    plan
}

/// Whether two walls stand in the same place, closer than a part of a mm that a float's rounding reaches.
fn same_fit(a: &Wall, b: &Wall) -> bool {
    let near = |x: f64, y: f64| (x - y).abs() < 1e-6;
    a.level == b.level
        && a.start.dist(b.start) < 1e-6
        && a.end.dist(b.end) < 1e-6
        && a.top_level == b.top_level
        && near(a.height, b.height)
        && near(a.base_offset, b.base_offset)
        && near(a.top_offset, b.top_offset)
        && near(a.arc, b.arc)
        && a.location == b.location
        && a.attach_top == b.attach_top
        && a.attach_base == b.attach_base
        && a.room_bounding == b.room_bounding
        && a.curtain == b.curtain
}

/// What's wrong with curtain wall type `n`'s pattern, saying what to fix.
fn check_pattern(m: &Model, n: &str, l: &CurtainLayout) -> Result<(), String> {
    let who = format!("wall type `{n}`");
    for (way, d) in [("columns", &l.columns), ("rows", &l.rows)] {
        if d.segments.is_empty() || d.segments.len() > MAX_SEGMENTS {
            return Err(format!("{who}: its {way} have {} segments; a pattern has from 1 to {MAX_SEGMENTS} each way", d.segments.len()));
        }
        if let Some(s) = d.segments.iter().find(|s| !(s.is_finite() && (MIN_SEGMENT..=MAX_COORD).contains(*s))) {
            return Err(format!("{who}: a segment of its {way} is {s} mm; it is from {MIN_SEGMENT} to {MAX_COORD} mm"));
        }
    }
    let (nc, nr) = (l.columns.segments.len(), l.rows.segments.len());
    let square = |grid: Vec<usize>| grid.len() == nr && grid.iter().all(|c| *c == nc);
    let sized = [
        ("panels", square(l.panels.iter().map(Vec::len).collect())),
        ("vertical", square(l.vertical.iter().map(Vec::len).collect())),
        ("horizontal", square(l.horizontal.iter().map(Vec::len).collect())),
    ];
    if let Some((key, _)) = sized.iter().find(|(_, ok)| !ok) {
        return Err(format!("{who}: `{key}` holds a row for each of its {nr} rows, of a place for each of its {nc} columns"));
    }
    if let Some(p) = l.panels.iter().flatten().flatten().find(|p| !m.types.curtain_panel.contains_key(*p)) {
        return Err(format!("{who}: there is no curtain panel type `{p}`; make it with `type`"));
    }
    if let Some(p) = l.picks().find(|p| !m.types.mullion.contains_key(&p.type_name)) {
        return Err(format!("{who}: there is no mullion type `{}`; make it with `type`", p.type_name));
    }
    if !(l.min_panel.is_finite() && (0.0..=MAX_SIZE).contains(&l.min_panel)) {
        return Err(format!("{who}: min_panel {} mm; it is from 0 to {MAX_SIZE} mm", l.min_panel));
    }
    if envelope(m, l).is_none() {
        return Err(format!("{who}: its pattern needs a panel or a mullion somewhere"));
    }
    // Three whole modules each way hold every meeting of its segments with each other and the borders.
    let whole = |d: &Direction| Direction { segments: d.segments.clone(), fit: d.fit, justify: Justify::Start };
    let (cs, rs) = (whole(&l.columns), whole(&l.rows));
    let (w, h) = (cs.segments.iter().sum::<f64>() * 3.0, rs.segments.iter().sum::<f64>() * 3.0);
    if build(m, l, &Edits::default(), divide(&cs, w), divide(&rs, h), w, h).too_small {
        return Err(format!(
            "{who}: a segment of its pattern is too small to hold a panel between its mullions; make it larger or take a mullion off"
        ));
    }
    Ok(())
}

/// What curtain wall `id` can't be yet, or is too large to be.
/// What's wrong with a wall standing in a cell. `taken`: the cells seen so far.
fn check_panel_wall(m: &Model, id: u64, w: &Wall, taken: &mut BTreeSet<(Guid, i64, i64)>) -> Result<(), String> {
    let Some(p) = w.panel_of else { return Ok(()) };
    if m.types.wall.get(&w.type_name).is_some_and(|t| t.curtain.is_some()) {
        return Err(format!("wall {id} stands in a curtain wall's cell: it takes a plain wall type, not `{}`", w.type_name));
    }
    let Some(host) = m.wall(p.host).filter(|h| pattern(m, h).is_some()) else {
        return Err(format!("wall {id} stands in a panel's place, and {} isn't a curtain wall; delete the wall first", p.host));
    };
    if !taken.insert((p.host, p.column, p.row)) {
        return Err(format!("wall {id}: column {}, row {} of its curtain wall already has a wall", p.column, p.row));
    }
    if !layout(m, host).walls.iter().any(|h| (h.column, h.row) == (p.column, p.row)) {
        return Err(format!(
            "this would lose column {}, row {} of curtain wall {}, and wall {id} there holds a door, a window or a tag; \
             delete what it holds first",
            p.column,
            p.row,
            m.elements.id(p.host).unwrap_or_default()
        ));
    }
    Ok(())
}

fn check_wall(m: &Model, id: u64, w: &Wall, l: &CurtainLayout) -> Result<(), String> {
    for e in w.curtain.iter().flat_map(|c| &c.parts) {
        let Some(name) = &e.type_name else { continue };
        let known = match e.at {
            Address::Cell { .. } => m.types.curtain_panel.contains_key(name),
            _ => m.types.mullion.contains_key(name),
        };
        if !known {
            let family = if matches!(e.at, Address::Cell { .. }) { "curtain panel" } else { "mullion" };
            return Err(format!("curtain wall {id}: an edited part names `{name}`, which isn't a {family} type; add it with `type`"));
        }
    }
    if w.arc != 0.0 {
        return Err(format!("curtain wall {id}: a curved curtain wall isn't supported yet; draw it straight"));
    }
    if w.attach_top.is_some() || w.attach_base.is_some() {
        return Err(format!("curtain wall {id}: attaching its top or base to a roof or floor isn't supported yet; detach it"));
    }
    if w.panel_of.is_some() {
        return Err(format!("wall {id} stands in a curtain wall's cell, and can't be a curtain wall itself; give it a plain type"));
    }
    if w.params.contains_key(crate::massing::FACE) {
        return Err(format!("curtain wall {id}: walls by face can't be curtain walls yet; draw it, or give it a plain type"));
    }
    if !matches!(w.location, Location::ExteriorFinish | Location::InteriorFinish) {
        return Err(format!("curtain wall {id}: its location line is its exterior or interior face (exterior-finish or interior-finish)"));
    }
    if cells(m, l, w) > MAX_CELLS {
        return Err(format!("curtain wall {id}: it would have over {MAX_CELLS} cells; make it smaller or its pattern's segments larger"));
    }
    Ok(())
}
