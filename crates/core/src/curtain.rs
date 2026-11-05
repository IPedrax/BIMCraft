//! Curtain walls: a wall whose type holds a grid pattern, with panels and mullions along it.
//!
//! Panels and mullions are system families. Their types live in [`Types`](crate::model::Types) as any other category's do.

use serde::{Deserialize, Serialize};

use bimcraft_i18n::ui_msg;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
/// The corner mullion type a new curtain wall takes: a mullion on the corner's diagonal.
pub const RECTANGULAR_CORNER: &str = "50 x 150 45°";
/// The other corner mullion type a project starts with: a square post.
pub const SQUARE_CORNER: &str = "150 x 150";
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
/// from the grid's plane toward the exterior. A corner mullion's `offset_y` is from the plane of the wall that holds it and its
/// `offset_x` from the other's, and `rotation` turns it, degrees, from the one wall toward the other: only that family turns.
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
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
    /// Add half the corner's turn to `rotation`, so with none given it stays square to the corner's diagonal at any angle: a wall
    /// turning 60° (120° between the walls) adds 30°.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_rotation: bool,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
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
        m.types.mullion.insert(
            RECTANGULAR_MULLION.into(),
            MullionType { profile, offset_x: 0.0, offset_y: STARTER_OFFSET, material: metal, rotation: 0.0, auto_rotation: false },
        );
    }
    corner_types(m);
    if !m.types.wall.contains_key(CURTAIN_WALL) {
        let way = || Direction { segments: vec![1500.0], fit: Fit::Flexible, justify: Justify::Start };
        let l = CurtainLayout::uniform(
            way(),
            way(),
            m.default_type("curtainpanel").map(String::from).as_deref(),
            m.default_type("mullion").map(String::from).as_deref(),
        );
        let l = CurtainLayout { corner: m.default_type("cornermullion").map(String::from), ..l };
        let thickness = thickness(m, &l);
        m.types.wall.insert(CURTAIN_WALL.into(), WallType { thickness, layers: Vec::new(), curtain: Some(l) });
    }
}

/// The corner mullions a project starts with, if it has none.
pub(crate) fn corner_types(m: &mut Model) {
    if !m.types.corner_mullion.is_empty() {
        return;
    }
    let metal = m.language.word("Metal").to_string();
    let metal = if m.materials.contains_key(&metal) { metal } else { String::new() };
    let post = |width: f64, depth: f64, rotation: f64| MullionType {
        profile: MullionProfile::Rectangular { width, depth },
        offset_x: STARTER_OFFSET,
        offset_y: STARTER_OFFSET,
        material: metal.clone(),
        rotation,
        auto_rotation: false,
    };
    m.types.corner_mullion.insert(RECTANGULAR_CORNER.into(), post(50.0, 150.0, 45.0));
    m.types.corner_mullion.insert(SQUARE_CORNER.into(), post(150.0, 150.0, 0.0));
}

/// What's wrong with the project's panel and mullion types and its patterns, saying what to fix.
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
        if t.rotation != 0.0 {
            return Err(format!("{who}: a rotation is a corner mullion's; make it with `category: cornermullion`"));
        }
        if t.auto_rotation {
            return Err(format!("{who}: auto_rotation is a corner mullion's; make it with `category: cornermullion`"));
        }
    }
    for (n, t) in &m.types.corner_mullion {
        let who = format!("corner mullion type `{n}`");
        size(&who, "width", t.width())?;
        size(&who, "depth", t.depth())?;
        offset(&who, "offset_x", t.offset_x)?;
        offset(&who, "offset_y", t.offset_y)?;
        material(&who, &t.material)?;
        if !(t.rotation.is_finite() && t.rotation.abs() <= 180.0) {
            return Err(format!("{who}: rotation {} degrees; it is from -180 to 180", t.rotation));
        }
    }
    for (n, t) in &m.types.wall {
        if let Some(l) = &t.curtain {
            check_pattern(m, n, l)?;
        }
    }
    Ok(())
}

/// What's wrong with element `id` as a curtain wall, a wall standing in a cell, or a door or a window in a curtain wall.
/// `taken`: the cells seen so far.
pub(crate) fn check_element(m: &Model, id: u64, e: &Element, taken: &mut BTreeSet<(Guid, i64, i64)>) -> Result<(), String> {
    match e {
        Element::Wall(w) => {
            if let Some(l) = pattern(m, w) {
                check_wall(m, id, w, l)?;
            }
            check_panel_wall(m, id, w, taken)
        }
        Element::Door(o) | Element::Window(o) if m.wall(o.host).is_some_and(|w| pattern(m, w).is_some()) => {
            Err(format!("{} {id} is in a curtain wall: replace a panel with a wall first, and put it in that wall", e.category()))
        }
        _ => Ok(()),
    }
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
/// The most levels a wall's anchored rows start again at; past them its rows run on.
const MAX_ANCHORS: usize = 256;
/// How far apart the rows of two storeys are numbered, more than a storey has rows.
const STOREY_ROWS: i64 = 1_000_000;

/// What a wall's length does with its pattern: Fixed Grid draws it in the pattern's lengths; Last Grid Flexible lets the last
/// segment take what's left; Fit Evenly stretches or shrinks whole modules, their segments in proportion, to fill it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    #[default]
    Flexible,
    Fixed,
    Evenly,
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
    /// The corner mullion where a wall of this type meets another curtain wall; none leaves their borders.
    #[serde(default)]
    pub corner: Option<String>,
    /// The rows start their pattern again at each level a wall passes, a grid line on the level.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub anchor_rows: bool,
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
    /// The grid lines this wall has by hand: they stay where they are when its pattern changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grids: Vec<Grid>,
    /// Where the levels a wall with anchored rows passes stand, mm from its base, lowest first: what the levels say, kept here so
    /// a layout doesn't look through the model, and so whatever is kept of this wall knows when one moves ([`sync`] keeps it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<f64>,
    /// The segments of its grid lines this wall has had taken off, each by the place of its mullion ([`Address::Vertical`] or
    /// [`Address::Horizontal`]): the cells either side make one panel. See [`Merged`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<Address>,
}

impl CurtainEdits {
    fn is_empty(&self) -> bool {
        !self.absorb_columns
            && !self.absorb_rows
            && self.parts.is_empty()
            && self.walled.is_empty()
            && self.grids.is_empty()
            && self.levels.is_empty()
            && self.removed.is_empty()
    }

    /// What's on column or row `from` of grid way `way`, moved to `to`: its edited parts and the cells walls stand in.
    pub(crate) fn renamed(&mut self, way: GridWay, from: i64, to: i64) {
        for p in &mut self.parts {
            p.at = p.at.renamed(way, from, to);
        }
        for a in &mut self.removed {
            *a = a.renamed(way, from, to);
        }
        for (column, row) in &mut self.walled {
            let index = if way == GridWay::Vertical { column } else { row };
            if *index == from {
                *index = to;
            }
        }
    }
}

/// Which way a grid line runs: a vertical one divides a wall's columns, a horizontal one its rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GridWay {
    #[default]
    Vertical,
    Horizontal,
}

/// A grid line a wall has by hand, the "manual" kind: `at` mm from the wall's start or base, whatever its pattern does. Its
/// column or row is [`MANUAL_LINE`] and its `number`. One that was a line of the pattern says which in `moved`, which stays
/// away, and `seg`, what lies past it. One added has the mullion of the pattern's first edge, A1.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grid {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    pub way: GridWay,
    pub at: f64,
    pub number: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seg: Option<usize>,
}

impl crate::library::HasPart for Grid {
    fn ident(&mut self) -> (&mut crate::ids::Id, &mut crate::ids::Guid) {
        (&mut self.id, &mut self.guid)
    }
}

/// Where the columns and rows of a wall's manual grids are numbered from: past any a pattern reaches.
pub const MANUAL_LINE: i64 = 1_000_000_000;
/// The most grid lines a wall has by hand.
pub const MAX_GRIDS: usize = 1024;

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
    /// Whether the line at its start is a manual grid, not one its pattern put there.
    pub manual: bool,
    /// Whether that line was added by hand, so takes the mullion of the pattern's A1.
    pub added: bool,
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
    /// A horizontal mullion mitred round a corner of two walls: its outline from above, counter-clockwise, as (along, across) from
    /// its centre; none for a box. It stands `size[1]` high.
    pub plan: Option<Quad>,
    /// Whether its type is of the Corner Mullion family.
    pub corner: bool,
}

/// Four corners on a wall's face.
pub type Quad = [[f64; 2]; 4];

impl Shape {
    /// The material its type names, of whichever family it's a type of.
    pub fn material_in<'a>(&self, m: &'a Model) -> Option<&'a str> {
        let t = &m.types;
        match self.kind {
            PartKind::Panel => t.curtain_panel.get(&self.type_name).map(|x| x.material.as_str()),
            PartKind::Mullion if self.corner => t.corner_mullion.get(&self.type_name).map(|x| x.material.as_str()),
            PartKind::Mullion => t.mullion.get(&self.type_name).map(|x| x.material.as_str()),
        }
    }

    /// Whether it's cut, so drawn from its [`Shape::tris`], not a box.
    pub fn is_cut(&self) -> bool {
        self.face.is_some() || self.plan.is_some()
    }

    /// Its triangles from its centre, as (along, across, up), turned outward: for a shape with a [`Shape::face`] or a [`Shape::plan`].
    pub fn tris(&self) -> Vec<[[f64; 3]; 3]> {
        if let Some(q) = self.plan {
            let half = self.size[1] / 2.0;
            let (low, high) = (q.map(|c| [c[0], c[1], -half]), q.map(|c| [c[0], c[1], half]));
            let mut out =
                vec![[high[0], high[1], high[2]], [high[0], high[2], high[3]], [low[0], low[2], low[1]], [low[0], low[3], low[2]]];
            for i in 0..4 {
                let j = (i + 1) % 4;
                out.push([low[i], low[j], high[j]]);
                out.push([low[i], high[j], high[i]]);
            }
            return out;
        }
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
    /// The corner mullion at the wall's start (`column` 0) or end (1), on the wall of the two that holds it.
    Corner {
        column: i64,
        row: i64,
    },
    /// A vertical grid line: `column` is the column it starts, and `row` 0. No part, what moves its mullions.
    #[serde(rename = "vertical-grid")]
    VerticalGrid {
        column: i64,
        row: i64,
    },
    /// A horizontal grid line: `row` is the row it starts, and `column` 0.
    #[serde(rename = "horizontal-grid")]
    HorizontalGrid {
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
    /// How far it's turned in plan from the wall's run, radians: a corner mullion's; none for the rest.
    pub turn: f64,
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
    /// Whether this wall holds the corner at its start and at its end, a mullion there now or none.
    pub corners: [bool; 2],
    /// The segments taken off its grid lines, by their mullions' places, while they make a merged panel.
    pub gone: HashSet<Address>,
    /// The cells merged, each with the cell whose name their one panel goes by.
    pub merged: HashMap<(i64, i64), (i64, i64)>,
}

impl Layout {
    /// How far along the wall a part reaches, from and to.
    pub fn along(&self, p: &Part) -> (f64, f64) {
        let half = self.shapes.get(p.shape as usize).map_or(0.0, |s| s.size[0] / 2.0);
        (p.centre[0] - half, p.centre[0] + half)
    }
}

/// What's drawn of a run of a wall's parts: the mullions of one line that run on through their crossings, which are one thing to
/// look at, though each stretch stays a part to pick and to type apart. Any other part is a piece by itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    /// The first of its parts, and the shape its kind and material come from.
    pub first: usize,
    /// Its size and centre: as [`Shape::size`] and [`Part::centre`], over all of it.
    pub size: [f64; 3],
    pub centre: [f64; 3],
}

impl Layout {
    /// What a drawing or the 3D scene draws of this wall, in the order of its parts.
    pub fn pieces(&self) -> Vec<Piece> {
        let mut out: Vec<Piece> = Vec::with_capacity(self.parts.len());
        for (i, p) in self.parts.iter().enumerate() {
            let Some(s) = self.shapes.get(p.shape as usize) else { continue };
            if let Some(last) = out.last_mut().filter(|last| self.runs_on(last, p, s)) {
                let way = usize::from(matches!(p.address, Address::Vertical { .. }));
                let (lo, hi) = (last.centre[way] - last.size[way] / 2.0, p.centre[way] + s.size[way] / 2.0);
                last.size[way] = hi - lo;
                last.centre[way] = (lo + hi) / 2.0;
                continue;
            }
            out.push(Piece { first: i, size: s.size, centre: p.centre });
        }
        out
    }

    /// Whether part `p` of shape `s` carries on from where piece `last` ends: a mullion of the same type and section, on the same line.
    fn runs_on(&self, last: &Piece, p: &Part, s: &Shape) -> bool {
        let Some(before) = self.parts.get(last.first) else { return false };
        let Some(was) = self.shapes.get(before.shape as usize) else { return false };
        let way = match (before.address, p.address) {
            (Address::Vertical { column: a, .. }, Address::Vertical { column: b, .. }) if a == b => 1,
            (Address::Horizontal { row: a, .. }, Address::Horizontal { row: b, .. }) if a == b => 0,
            _ => return false,
        };
        let same = |a: f64, b: f64| (a - b).abs() < SAME;
        let across = 1 - way;
        !s.is_cut()
            && !was.is_cut()
            && p.turn == 0.0
            && before.turn == 0.0
            && s.type_name == was.type_name
            && same(s.size[across], was.size[across])
            && same(s.size[2], was.size[2])
            && same(p.centre[across], last.centre[across])
            && same(p.centre[2], last.centre[2])
            && same(last.centre[way] + last.size[way] / 2.0, p.centre[way] - s.size[way] / 2.0)
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
    let by_hand = |way: GridWay| w.curtain.as_ref().map_or(0, |e| e.grids.iter().filter(|g| g.way == way).count()) as f64;
    // A storey's cut rows come on top of what the height alone would make.
    let storeys = w.curtain.as_ref().filter(|_| l.anchor_rows).map_or(0, |e| e.levels.len().min(MAX_ANCHORS)) as f64;
    (stretches(&l.columns, Axis::of(w).length()) + by_hand(GridWay::Vertical))
        * (stretches(&l.rows, m.own_top(w) - m.own_base(w)) + storeys * 2.0 * l.rows.segments.len() as f64 + by_hand(GridWay::Horizontal))
}

/// A pattern's stretches over `length`: repeated from where it's justified and cut by the ends, or fitted evenly. Nothing for a pattern or a
/// length that makes no sense, which `check` refuses.
pub fn divide(d: &Direction, length: f64) -> Vec<Span> {
    let module: f64 = d.segments.iter().sum();
    let good = d.segments.iter().all(|s| s.is_finite() && *s > 0.0);
    if !(good && module > 0.0 && length.is_finite() && length > SAME && stretches(d, length) <= MAX_CELLS) {
        return Vec::new();
    }
    if d.fit == Fit::Evenly {
        return evenly(d, module, length);
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
                out.push(Span { from, to, seg, index: k * count + seg as i64, whole, manual: false, added: false });
            }
        }
        k += 1;
    }
}

/// Wall `w`'s rows over its `height`: the pattern's, started again at each level it holds if the pattern is anchored to them.
fn rows_of(l: &CurtainLayout, w: &Wall, height: f64) -> Vec<Span> {
    let levels = w.curtain.as_ref().filter(|_| l.anchor_rows).map_or(&[][..], |e| &e.levels[..]);
    if levels.is_empty() {
        return divide(&l.rows, height);
    }
    let mut out = Vec::new();
    let mut from = 0.0;
    let inside = levels.iter().copied().filter(|z| z.is_finite() && *z > SAME && *z < height - SAME).take(MAX_ANCHORS);
    for (storey, to) in inside.chain([height]).enumerate() {
        if to > from + SAME {
            out.extend(divide(&l.rows, to - from).into_iter().map(|r| Span {
                from: r.from + from,
                to: r.to + from,
                index: r.index + storey as i64 * STOREY_ROWS,
                ..r
            }));
            from = to;
        }
    }
    out
}

/// The heights of the levels wall `w` of pattern `l` should hold, mm from its base.
fn levels_within(m: &Model, l: &CurtainLayout, w: &Wall) -> Vec<f64> {
    if !l.anchor_rows {
        return Vec::new();
    }
    let (base, top) = (m.own_base(w), m.own_top(w));
    let mut at: Vec<f64> = m.levels().iter().map(|(_, x)| x.elevation - base).filter(|z| *z > SAME && *z < top - base - SAME).collect();
    at.sort_by(f64::total_cmp);
    at.truncate(MAX_ANCHORS);
    at
}

/// The curtain walls whose `levels` aren't what the levels say now, and what they should be.
fn levels_plan(m: &Model) -> Vec<(u64, Vec<f64>)> {
    if !m.types.wall.values().any(|t| t.curtain.is_some()) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (id, e) in m.elements.iter() {
        let Element::Wall(w) = e else { continue };
        let held = w.curtain.as_ref().map_or(&[][..], |e| &e.levels[..]);
        let want = pattern(m, w).map_or_else(Vec::new, |l| levels_within(m, l, w));
        if held != want {
            out.push((*id, want));
        }
    }
    out
}

/// Fit Evenly's stretches: the whole modules nearest `length`, at least one, each stretched or shrunk to fill it, its segments in
/// proportion. Nothing is over, so justification has nothing to place. Shrunk, a segment may no longer hold its panel: that's the
/// wall's length and not the pattern's fault, so it isn't `whole`, and a wall says so as it does of a cut segment.
fn evenly(d: &Direction, module: f64, length: f64) -> Vec<Span> {
    let modules = (length / module).round().max(1.0);
    let total = modules * module;
    let whole = length >= total - SAME;
    let count = d.segments.len() as i64;
    let mut out = Vec::new();
    let mut gone = 0.0;
    for k in 0..modules as i64 {
        for (seg, size) in d.segments.iter().enumerate() {
            let from = length * (gone / total);
            gone += size;
            let to = if k + 1 == modules as i64 && seg + 1 == d.segments.len() { length } else { length * (gone / total) };
            out.push(Span { from, to, seg, index: k * count + seg as i64, whole, manual: false, added: false });
        }
    }
    out
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

/// Cells of a wall's grid joined by segments taken off its lines, by their places in its spans.
struct CellGroup {
    cells: BTreeSet<(usize, usize)>,
    columns: (usize, usize),
    rows: (usize, usize),
}

impl CellGroup {
    fn is_rectangle(&self) -> bool {
        self.cells.len() == (self.columns.1 - self.columns.0 + 1) * (self.rows.1 - self.rows.0 + 1)
    }

    /// The segments a panel over all of it has inside it.
    fn inside(&self, columns: &[Span], rows: &[Span]) -> Vec<Address> {
        let mut out = Vec::new();
        for (c, r) in &self.cells {
            let (column, row) = (columns[*c].index, rows[*r].index);
            if *c > self.columns.0 {
                out.push(Address::Vertical { column, row });
            }
            if *r > self.rows.0 {
                out.push(Address::Horizontal { column, row });
            }
        }
        out
    }

    fn holds_a_wall(&self, columns: &[Span], rows: &[Span], walled: &[(i64, i64)]) -> bool {
        self.cells.iter().any(|(c, r)| walled.contains(&(columns[*c].index, rows[*r].index)))
    }
}

/// The cells `removed` joins. A segment naming no line between two cells of these spans joins nothing, and sleeps.
fn cell_groups(columns: &[Span], rows: &[Span], removed: &[Address]) -> Vec<CellGroup> {
    let place = |spans: &[Span]| spans.iter().enumerate().map(|(i, s)| (s.index, i)).collect::<HashMap<i64, usize>>();
    let (cs, rs) = (place(columns), place(rows));
    let mut parent: BTreeMap<(usize, usize), (usize, usize)> = BTreeMap::new();
    fn root(parent: &mut BTreeMap<(usize, usize), (usize, usize)>, cell: (usize, usize)) -> (usize, usize) {
        let mut at = *parent.entry(cell).or_insert(cell);
        while let Some(&up) = parent.get(&at).filter(|up| **up != at) {
            at = up;
        }
        parent.insert(cell, at);
        at
    }
    for a in removed {
        let (one, other) = match *a {
            Address::Vertical { column, row } => match (cs.get(&column), rs.get(&row)) {
                (Some(&c), Some(&r)) if c > 0 => ((c - 1, r), (c, r)),
                _ => continue,
            },
            Address::Horizontal { column, row } => match (cs.get(&column), rs.get(&row)) {
                (Some(&c), Some(&r)) if r > 0 => ((c, r - 1), (c, r)),
                _ => continue,
            },
            _ => continue,
        };
        let (x, y) = (root(&mut parent, one), root(&mut parent, other));
        parent.insert(y, x);
    }
    let cells: Vec<(usize, usize)> = parent.keys().copied().collect();
    let mut groups: BTreeMap<(usize, usize), CellGroup> = BTreeMap::new();
    for cell in cells {
        let g = groups.entry(root(&mut parent, cell)).or_insert(CellGroup {
            cells: BTreeSet::new(),
            columns: (cell.0, cell.0),
            rows: (cell.1, cell.1),
        });
        g.cells.insert(cell);
        g.columns = (g.columns.0.min(cell.0), g.columns.1.max(cell.0));
        g.rows = (g.rows.0.min(cell.1), g.rows.1.max(cell.1));
    }
    groups.into_values().collect()
}

/// The panels a wall's removed segments make: only a rectangle with nothing left inside it and no wall in it is one (the command
/// keeps them so). What a later change leaves otherwise stands as its cells until it's one again.
#[derive(Default)]
struct Merged {
    /// The segments whose mullions aren't there.
    gone: HashSet<Address>,
    /// Each merged panel's first cell and its last, by their places in the spans.
    last: HashMap<(usize, usize), (usize, usize)>,
    /// The cells a merged panel covers, but for its first.
    covered: HashSet<(usize, usize)>,
}

fn merges(columns: &[Span], rows: &[Span], removed: &[Address], walled: &[(i64, i64)]) -> Merged {
    let mut out = Merged::default();
    if removed.is_empty() {
        return out;
    }
    let given: HashSet<Address> = removed.iter().copied().collect();
    for g in cell_groups(columns, rows, removed) {
        let inside = g.inside(columns, rows);
        if g.is_rectangle() && !g.holds_a_wall(columns, rows, walled) && inside.iter().all(|a| given.contains(a)) {
            let first = (g.columns.0, g.rows.0);
            out.last.insert(first, (g.columns.1, g.rows.1));
            out.covered.extend(g.cells.iter().copied().filter(|c| *c != first));
            out.gone.extend(inside);
        }
    }
    out
}

/// The segments wall `w` holds once `given` are taken off or, with `restore`, put back, as `laid` stands. Taken off, they must leave
/// rectangles, and what's left inside one goes too. Put back, a segment brings back the whole line through its merged panel,
/// which leaves two rectangles.
pub(crate) fn segments_after(
    laid: &Layout,
    held: &[Address],
    given: &[Address],
    restore: bool,
    walled: &[(i64, i64)],
) -> Result<Vec<Address>, String> {
    let (columns, rows) = (&laid.columns[..], &laid.rows[..]);
    for a in given {
        let inner = match *a {
            Address::Vertical { column, row } => columns.iter().skip(1).any(|s| s.index == column) && rows.iter().any(|s| s.index == row),
            Address::Horizontal { column, row } => columns.iter().any(|s| s.index == column) && rows.iter().skip(1).any(|s| s.index == row),
            _ => return Err("a segment is a `vertical` or `horizontal` part".into()),
        };
        if !inner {
            return Err(format!("{a:?} isn't a segment of a grid line inside the wall: a border can't go; `curtain_parts` lists them"));
        }
    }
    let mut list = held.to_vec();
    if restore {
        let groups = cell_groups(columns, rows, held);
        for a in given {
            if !laid.gone.contains(a) {
                return Err(format!("{a:?} hasn't been removed; `curtain_parts` lists those `removed`"));
            }
            let cell = |column: i64, row: i64| {
                let c = columns.iter().position(|s| s.index == column)?;
                Some((c, rows.iter().position(|s| s.index == row)?))
            };
            let (Address::Vertical { column, row } | Address::Horizontal { column, row }) = *a else { continue };
            let Some(g) = cell(column, row).and_then(|at| groups.iter().find(|g| g.cells.contains(&at))) else { continue };
            list.retain(|x| match (*a, *x) {
                (Address::Vertical { column: line, .. }, Address::Vertical { column, row }) => {
                    column != line || !rows.get(g.rows.0..=g.rows.1).is_some_and(|r| r.iter().any(|s| s.index == row))
                }
                (Address::Horizontal { row: line, .. }, Address::Horizontal { column, row }) => {
                    row != line || !columns.get(g.columns.0..=g.columns.1).is_some_and(|c| c.iter().any(|s| s.index == column))
                }
                _ => true,
            });
        }
        return Ok(list);
    }
    list.extend_from_slice(given);
    for g in cell_groups(columns, rows, &list) {
        if !g.is_rectangle() {
            return Err("that would make a panel that isn't a rectangle: take off the segments that make it one".into());
        }
        if g.holds_a_wall(columns, rows, walled) {
            return Err("a wall stands in one of those cells: delete it first".into());
        }
        list.extend(g.inside(columns, rows));
    }
    let mut seen = HashSet::new();
    list.retain(|a| seen.insert(*a));
    Ok(list)
}

/// What a wall changes of its pattern: the types its edited parts take, by where they are, and the cells walls stand in.
#[derive(Default)]
struct Edits<'a> {
    parts: HashMap<Address, Option<&'a str>>,
    walled: Vec<(i64, i64)>,
    /// The segments taken off its grid lines.
    removed: Vec<Address>,
    /// What stands at the wall's start and end in place of its borders.
    posts: [Option<Post>; 2],
    /// How its horizontal mullions meet the other wall's, round a corner at its start and at its end.
    mitres: [Option<Mitre>; 2],
}

impl<'a> Edits<'a> {
    fn of(e: Option<&'a CurtainEdits>, walled: &[(i64, i64)]) -> Self {
        let parts = e.map_or_else(HashMap::new, |e| e.parts.iter().map(|p| (p.at, p.type_name.as_deref())).collect());
        let removed = e.map_or_else(Vec::new, |e| e.removed.clone());
        Edits { parts, walled: walled.to_vec(), removed, posts: [None, None], mitres: [None, None] }
    }
}

/// A mullion cut to `ring`, its corners on the wall's face: centred on what bounds them.
fn cut_part(out: &mut Shapes, b: &Bar, ring: Quad, address: Address) -> Part {
    let bound = |axis: usize| ring.iter().fold((f64::MAX, f64::MIN), |(lo, hi), c| (lo.min(c[axis]), hi.max(c[axis])));
    let ((u0, u1), (v0, v1)) = (bound(0), bound(1));
    let centre = [(u0 + u1) / 2.0, (v0 + v1) / 2.0];
    let face = ring.map(|c| [c[0] - centre[0], c[1] - centre[1]]);
    let shape = out.of(PartKind::Mullion, b.pick, [u1 - u0, v1 - v0, b.depth], Some(face));
    Part { shape, centre: [centre[0], centre[1], b.offset], address, turn: 0.0 }
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
    let index = |spans: &[Span], i: usize| spans.get(i).map_or_else(|| past_line(spans), |s| s.index);
    let merged = merges(&columns, &rows, &edits.removed, &edits.walled);
    let bar = |at: Address, p: Option<&'a MullionPick>, place: At| {
        if merged.gone.contains(&at) {
            return None;
        }
        let name = edits.parts.get(&at).copied().unwrap_or(p.map(|x| x.type_name.as_str()));
        Bar::of(m, name, p.map_or(Join::Butt, |x| x.join), place)
    };
    let mut vertical: Lines = Vec::with_capacity(nc + 1);
    for i in 0..=nc {
        let mut runs = Vec::with_capacity(nr);
        for r in &rows {
            let (p, place) = match columns.get(i) {
                _ if i == 0 => (l.borders.left.as_ref(), At::Start),
                Some(c) if c.added => (l.first_pick(GridWay::Vertical), At::Line(c.from)),
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
                Some(r) if r.added => (l.first_pick(GridWay::Horizontal), At::Line(r.from)),
                Some(r) => (l.horizontal.get(r.seg).and_then(|x| x.get(c.seg)).and_then(Option::as_ref), At::Line(r.from)),
                None => (l.borders.top.as_ref(), At::End(height)),
            };
            runs.push(bar(Address::Horizontal { column: c.index, row: index(&rows, j) }, p, place));
        }
        horizontal.push(runs);
    }
    drop_clashes(&mut vertical);
    drop_clashes(&mut horizontal);
    // A corner mullion takes the place of the border at its end, and of a line it would run into.
    let [post_start, post_end] = &edits.posts;
    for (i, line) in vertical.iter_mut().enumerate() {
        for run in line.iter_mut() {
            let clash = run.is_some_and(|b| {
                post_start.as_ref().is_some_and(|p| i == 0 || b.lo < p.clear_of_bars(b.across, false))
                    || post_end.as_ref().is_some_and(|p| i == nc || b.hi > p.clear_of_bars(b.across, true))
            });
            if clash {
                *run = None;
            }
        }
    }
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
                parts.push(Part { shape, centre: [b.middle(), (v0 + v1) / 2.0, b.offset], address, turn: 0.0 });
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
            // In a mitre with the other wall's, round a corner, where it has one to match; else square against a corner mullion,
            // whichever side of the wall's end its face is.
            let mitre_from = edits.mitres[0].as_ref().filter(|_| c == 0).and_then(|x| x.ends(b));
            let mitre_to = edits.mitres[1].as_ref().filter(|_| c + 1 == nc).and_then(|x| x.ends(b));
            if let Some(p) = post_start.as_ref().filter(|_| c == 0) {
                u0 = p.clear_of_bars(b.across, false);
            }
            if let Some(p) = post_end.as_ref().filter(|_| c + 1 == nc) {
                u1 = p.clear_of_bars(b.across, true);
            }
            let (first, last) = (mitres.get(&(c, j)), mitres.get(&(c + 1, j)));
            let at = |up: f64| (first.map_or(u0, |x| x.along_at(up)), last.map_or(u1, |x| x.along_at(up)));
            let (under, over) = (at(b.lo), at(b.hi));
            let mitred = (first.is_some() || last.is_some()) && under.0 < under.1 - SAME && over.0 < over.1 - SAME;
            let address = Address::Horizontal { column: columns[c].index, row: index(&rows, j) };
            let (from, to) = (mitre_from.unwrap_or((u0, u0)), mitre_to.unwrap_or((u1, u1)));
            if !mitred && (mitre_from.is_some() || mitre_to.is_some()) && from.0 < to.0 - SAME && from.1 < to.1 - SAME {
                let (lo, hi) = (from.0.min(from.1), to.0.max(to.1));
                let (middle, half) = ((lo + hi) / 2.0, b.depth / 2.0);
                let plan = [[from.0 - middle, -half], [to.0 - middle, -half], [to.1 - middle, half], [from.1 - middle, half]];
                let shape = out.of_plan(PartKind::Mullion, b.pick, [hi - lo, b.hi - b.lo, b.depth], plan);
                parts.push(Part { shape, centre: [middle, b.middle(), b.offset], address, turn: 0.0 });
            } else if mitred {
                let ring = [[under.0, b.lo], [under.1, b.lo], [over.1, b.hi], [over.0, b.hi]];
                parts.push(cut_part(&mut out, b, ring, address));
            } else if u1 - u0 > SAME {
                let shape = out.of(PartKind::Mullion, b.pick, [u1 - u0, b.hi - b.lo, b.depth], None);
                parts.push(Part { shape, centre: [(u0 + u1) / 2.0, b.middle(), b.offset], address, turn: 0.0 });
            }
        }
    }
    // Panels, and what's too small to hold one.
    let (mut short_columns, mut short_rows, mut too_small) = (Vec::new(), Vec::new(), false);
    for (r, row) in rows.iter().enumerate() {
        for (c, column) in columns.iter().enumerate() {
            if merged.covered.contains(&(c, r)) {
                continue;
            }
            // A merged panel reaches to its last cell, and stops at what bounds any of its cells.
            let (c1, r1) = merged.last.get(&(c, r)).copied().unwrap_or((c, r));
            let (u_span, v_span) = ((column.from, columns[c1].to), (row.from, rows[r1].to));
            let lines =
                |lines_of: &[Vec<Option<Bar<'a>>>], a: usize, b: usize| a.saturating_sub(1)..=(b + 2).min(lines_of.len().saturating_sub(1));
            let by_columns: Vec<&Bar> =
                lines(&vertical, c, c1).flat_map(|i| (r..=r1).filter_map(|k| vertical[i][k].as_ref()).collect::<Vec<_>>()).collect();
            let by_rows: Vec<&Bar> =
                lines(&horizontal, r, r1).flat_map(|j| (c..=c1).filter_map(|k| horizontal[j][k].as_ref()).collect::<Vec<_>>()).collect();
            // A wall stands between the mullions, however deep it is.
            if edits.walled.contains(&(column.index, row.index)) {
                let across = (f64::MIN, f64::MAX);
                let (u0, u1) = narrowed(u_span, across, by_columns.into_iter());
                let (v0, v1) = narrowed(v_span, across, by_rows.into_iter());
                let u0 = post_start.as_ref().filter(|_| c == 0).map_or(u0, |p| p.reach((0.0, 0.0), false));
                let u1 = post_end.as_ref().filter(|_| c + 1 == nc).map_or(u1, |p| p.reach((0.0, 0.0), true));
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
            let (u0, u1) = narrowed(u_span, across, by_columns.into_iter());
            let (v0, v1) = narrowed(v_span, across, by_rows.into_iter());
            let u0 = post_start.as_ref().filter(|_| c == 0).map_or(u0, |p| p.clear_of_glass(across, false));
            let u1 = post_end.as_ref().filter(|_| c1 + 1 == nc).map_or(u1, |p| p.clear_of_glass(across, true));
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
            parts.push(Part { shape, centre: [(u0 + u1) / 2.0, (v0 + v1) / 2.0, t.offset], address, turn: 0.0 });
        }
    }
    let mut named = HashMap::new();
    for (&(c, r), &(c1, r1)) in &merged.last {
        let first = (columns[c].index, rows[r].index);
        for x in &columns[c..=c1] {
            for y in &rows[r..=r1] {
                named.insert((x.index, y.index), first);
            }
        }
    }
    let gone = merged.gone;
    Built { columns, rows, shapes: out.shapes, parts, walls, short_columns, short_rows, too_small, gone, merged: named }
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
    gone: HashSet<Address>,
    merged: HashMap<(i64, i64), (i64, i64)>,
}

/// What makes two shapes one: kind, type, and size, face and outline to within [`SAME`].
type ShapeKey = (PartKind, String, [i64; 3], Option<CutKey>, Option<CutKey>);

/// A cut shape's corners, to within [`SAME`].
type CutKey = [[i64; 2]; 4];

/// The shapes of a wall's parts, each kept once: what makes a thousand identical panels one.
#[derive(Default)]
struct Shapes {
    shapes: Vec<Shape>,
    known: HashMap<ShapeKey, u32>,
}

impl Shapes {
    fn of(&mut self, kind: PartKind, name: &str, size: [f64; 3], face: Option<Quad>) -> u32 {
        self.put(kind, name, size, face, None)
    }

    fn of_plan(&mut self, kind: PartKind, name: &str, size: [f64; 3], plan: Quad) -> u32 {
        self.put(kind, name, size, None, Some(plan))
    }

    fn put(&mut self, kind: PartKind, name: &str, size: [f64; 3], face: Option<Quad>, plan: Option<Quad>) -> u32 {
        let rounded = |x: f64| (x / SAME).round() as i64;
        let cut = |q: Option<Quad>| q.map(|q| q.map(|c| c.map(rounded)));
        let key = (kind, name.to_string(), size.map(rounded), cut(face), cut(plan));
        if let Some(&s) = self.known.get(&key) {
            return s;
        }
        let s = self.shapes.len() as u32;
        self.shapes.push(Shape { kind, type_name: name.to_string(), size, face, plan, corner: false });
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
        } else if i > 0 && i + 1 < n && !spans[i].manual {
            // A storey's short row, anchored to the level over it: the row below takes it in.
            spans[i - 1].to = spans[i].to;
            spans[i - 1].whole = false;
            spans.remove(i);
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
    let (mut columns, mut rows) = (divide(&l.columns, length), rows_of(l, w, height));
    if columns.is_empty() || rows.is_empty() {
        return Layout::default();
    }
    if let Some(e) = w.curtain.as_ref().filter(|e| !e.grids.is_empty()) {
        by_hand(&mut columns, e, GridWay::Vertical, length, l.columns.segments.len());
        by_hand(&mut rows, e, GridWay::Horizontal, height, l.rows.segments.len());
    }
    let mut edits = Edits::of(w.curtain.as_ref(), walled);
    let meets = [meeting(m, w, false), meeting(m, w, true)];
    let raised = [post_at(m, w, meets[0].as_ref()), post_at(m, w, meets[1].as_ref())];
    edits.posts = [raised[0].as_ref().map(|(p, _)| p.clone()), raised[1].as_ref().map(|(p, _)| p.clone())];
    edits.mitres = [mitre_at(m, w, meets[0].as_ref()), mitre_at(m, w, meets[1].as_ref())];
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
    let (mut shapes, mut parts) = (built.shapes, built.parts);
    for (end, standing) in raised.iter().enumerate() {
        if let Some((post, name)) = standing.as_ref().filter(|_| meets[end].as_ref().is_some_and(|x| x.holds)) {
            let shape = shapes.len() as u32;
            shapes.push(Shape {
                kind: PartKind::Mullion,
                type_name: name.clone(),
                size: [post.size[0], post.v.1 - post.v.0, post.size[1]],
                face: None,
                plan: None,
                corner: true,
            });
            let (centre, turn) = (post.centre, post.turn);
            let address = Address::Corner { column: end as i64, row: 0 };
            parts.push(Part { shape, centre: [centre[0], (post.v.0 + post.v.1) / 2.0, centre[1]], address, turn });
        }
    }
    let corners = [meets[0].as_ref().is_some_and(|x| x.holds), meets[1].as_ref().is_some_and(|x| x.holds)];
    Layout {
        walls: built.walls,
        columns: built.columns,
        rows: built.rows,
        shapes,
        parts,
        malformed,
        corners,
        gone: built.gone,
        merged: built.merged,
    }
}

/// The column or row a line past the last of `spans` is named by: one past the last the pattern put there.
fn past_line(spans: &[Span]) -> i64 {
    spans.iter().filter(|s| !s.manual).map(|s| s.index + 1).max().unwrap_or(0)
}

/// `spans` as a wall's manual grids of way `way` leave them: a line of the pattern one was moved from gone, its neighbour taking
/// its stretch, and each within the wall's `length` starting a span. `count`: the pattern's segments.
fn by_hand(spans: &mut Vec<Span>, e: &CurtainEdits, way: GridWay, length: f64, count: usize) {
    let mine = || e.grids.iter().filter(|g| g.way == way);
    for g in mine() {
        let Some(k) = g.moved.and_then(|i| spans.iter().position(|s| s.index == i)).filter(|k| *k > 0) else { continue };
        let to = spans[k].to;
        spans.remove(k);
        spans[k - 1].to = to;
        spans[k - 1].whole = false;
    }
    let mut sorted: Vec<&Grid> = mine().filter(|g| g.at.is_finite() && g.at > SAME && g.at < length - SAME).collect();
    sorted.sort_by(|a, b| a.at.total_cmp(&b.at));
    for g in sorted {
        let Some(k) = spans.iter().position(|s| s.from < g.at - SAME && g.at + SAME < s.to) else { continue };
        let old = spans[k];
        spans[k].to = g.at;
        spans[k].whole = false;
        let seg = g.seg.map_or(old.seg, |x| x % count.max(1));
        let index = MANUAL_LINE.saturating_add(g.number);
        spans.insert(k + 1, Span { from: g.at, to: old.to, seg, index, whole: false, manual: true, added: g.moved.is_none() });
    }
}

/// How one wall's end meets another curtain wall's: there are just the two, at an angle.
struct Meeting<'a> {
    other: &'a Wall,
    /// Whether it's the other's end that's there, not its start.
    other_end: bool,
    at_end: bool,
    /// Whether this wall holds what's there: the wall ending there, or of two alike, the one whose far end sorts first.
    holds: bool,
}

/// The other wall where `w`'s start or end meets one, if a corner mullion belongs there.
fn meeting<'a>(m: &'a Model, w: &Wall, at_end: bool) -> Option<Meeting<'a>> {
    if w.panel_of.is_some() || w.arc != 0.0 {
        return None;
    }
    let node = if at_end { w.end } else { w.start };
    let thickest = m.types.wall.values().map(|t| t.thickness).filter(|t| t.is_finite()).fold(0.0, f64::max);
    let near = m.lookup().walls_near_at(&[([node.0, node.1, node.0, node.1], m.base_z(w), m.top_z(w))], thickest + 1.0);
    let mut found: Option<(&Wall, bool)> = None;
    for o in near.into_iter().filter_map(|id| m.wall(id)) {
        let same = o.start.dist(w.start) <= crate::model::TOL && o.end.dist(w.end) <= crate::model::TOL;
        if same || o.panel_of.is_some() || !crate::walls::stacked(m, w, o) {
            continue;
        }
        let (at_start, at_its_end) = (o.start.dist(node) <= crate::model::TOL, o.end.dist(node) <= crate::model::TOL);
        if at_start || at_its_end {
            if found.is_some() {
                return None;
            }
            found = Some((o, at_its_end));
        } else {
            // A wall running through the node makes it no corner.
            let axis = Axis::of(o);
            let (s, across) = axis.project(node);
            let (ext, int) = crate::walls::faces(m, o);
            if s > crate::model::TOL && s < axis.length() - crate::model::TOL && (int..=ext).contains(&across) {
                return None;
            }
        }
    }
    let (other, other_end) = found?;
    if pattern(m, other).is_none() || other.arc != 0.0 {
        return None;
    }
    let (u, v) = ((w.end - w.start).unit(), (other.end - other.start).unit());
    if u.cross(v).abs() < MIN_TURN.sin() {
        return None;
    }
    let far = |x: &Wall, end: bool| if end { x.start } else { x.end };
    let holds = match (at_end, other_end) {
        (true, false) => true,
        (false, true) => false,
        _ => {
            let (a, b) = (far(w, at_end), far(other, other_end));
            a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).is_lt()
        }
    };
    Some(Meeting { other, other_end, at_end, holds })
}

/// The least angle between two walls for a corner mullion, radians: short of it they run on in a line.
const MIN_TURN: f64 = 0.0175;

/// A corner mullion as a wall sees it: in its own terms, whichever of the two holds it.
#[derive(Clone, Debug)]
struct Post {
    /// Its corners, as (along, toward the exterior from the grid's plane).
    ring: [[f64; 2]; 4],
    centre: [f64; 2],
    /// Width and depth.
    size: [f64; 2],
    /// What the other wall's glass and its mullions take up, in the same terms: sharp corners bring them across this wall's.
    glass: Option<[[f64; 2]; 4]>,
    bars: Option<[[f64; 2]; 4]>,
    /// From and to, up from this wall's base.
    v: (f64, f64),
    turn: f64,
}

impl Post {
    /// How far along the wall what lies `band` across it stops, square, to clear this standing at the wall's end (`at_end`) or
    /// its start. With nothing of it in that band, at its middle.
    fn reach(&self, band: (f64, f64), at_end: bool) -> f64 {
        furthest(&self.ring, band, at_end).unwrap_or(self.centre[0])
    }

    /// As [`Post::reach`], and clear of `other` too, the other wall's glass or mullions, where a sharp corner brings them across.
    fn clear(&self, band: (f64, f64), at_end: bool, other: Option<&[[f64; 2]; 4]>) -> f64 {
        let post = self.reach(band, at_end);
        match other.and_then(|ring| furthest(ring, band, at_end)) {
            Some(x) if at_end => post.min(x),
            Some(x) => post.max(x),
            None => post,
        }
    }

    /// How far glass of `band` across this wall stops.
    fn clear_of_glass(&self, band: (f64, f64), at_end: bool) -> f64 {
        self.clear(band, at_end, self.glass.as_ref())
    }

    /// How far a mullion of `band` across this wall stops.
    fn clear_of_bars(&self, band: (f64, f64), at_end: bool) -> f64 {
        self.clear(band, at_end, self.bars.as_ref())
    }
}

/// How far into a wall from its end (`at_end`) or its start `ring` reaches within `band` across it; none if it isn't in the band.
fn furthest(ring: &[[f64; 2]; 4], band: (f64, f64), at_end: bool) -> Option<f64> {
    let (lo, hi) = (band.0.min(band.1), band.0.max(band.1));
    let mut along: Vec<f64> = Vec::with_capacity(8);
    for i in 0..4 {
        let (a, b) = (ring[i], ring[(i + 1) % 4]);
        if (lo..=hi).contains(&a[1]) {
            along.push(a[0]);
        }
        for limit in [lo, hi] {
            if (a[1] - limit) * (b[1] - limit) < 0.0 {
                along.push(a[0] + (b[0] - a[0]) * (limit - a[1]) / (b[1] - a[1]));
            }
        }
    }
    if along.is_empty() {
        return None;
    }
    Some(if at_end { along.iter().copied().fold(f64::MAX, f64::min) } else { along.iter().copied().fold(f64::MIN, f64::max) })
}

/// What's `band` across wall `w` from its far end to where it stops at the mullion `post` at this end (`at_end`), on the plan.
fn stopped(m: &Model, w: &Wall, at_end: bool, band: (f64, f64), post: &[P2; 4]) -> [P2; 4] {
    let (u, plane, length) = ((w.end - w.start).unit(), grid_plane(m, w), Axis::of(w).length());
    let ring = post.map(|q| [(q - w.start).dot(u), (q - w.start).dot(u.perp()) - plane]);
    let far = if at_end { 0.0 } else { length };
    let stop = furthest(&ring, band, at_end).unwrap_or(if at_end { length } else { 0.0 });
    [(far, band.0), (stop, band.0), (stop, band.1), (far, band.1)].map(|(a, c)| w.start + u * a + u.perp() * (plane + c))
}

/// Whether two convex rings on a plan overlap, by more than a part of a mm, by their separating axes.
fn rings_cross(a: &[P2; 4], b: &[P2; 4]) -> bool {
    for ring in [a, b] {
        for i in 0..4 {
            let n = (ring[(i + 1) % 4] - ring[i]).unit().perp();
            let span = |r: &[P2; 4]| r.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(q.dot(n)), hi.max(q.dot(n))));
            let ((a0, a1), (b0, b1)) = (span(a), span(b));
            if a1 <= b0 + SAME || b1 <= a0 + SAME {
                return false;
            }
        }
    }
    true
}

/// What of a pattern takes up what across its wall: [`panel_band`] or [`bar_band`].
type BandOf = fn(&Model, &CurtainLayout) -> Option<(f64, f64)>;

/// What a pattern's panels take up across its wall, from the grid's plane: from and to.
fn panel_band(m: &Model, l: &CurtainLayout) -> Option<(f64, f64)> {
    let t = l.panels.iter().flatten().flatten().filter_map(|n| m.types.curtain_panel.get(n));
    t.map(|t| (t.offset - t.thickness / 2.0, t.offset + t.thickness / 2.0)).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

/// What a pattern's mullions take up across its wall, from the grid's plane: from and to.
fn bar_band(m: &Model, l: &CurtainLayout) -> Option<(f64, f64)> {
    let t = l.picks().filter_map(|p| m.types.mullion.get(&p.type_name));
    t.map(|t| (t.offset_y - t.depth() / 2.0, t.offset_y + t.depth() / 2.0)).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

/// The least angle between two walls for their horizontal mullions to mitre, radians: a sharper one throws a spike past the corner.
const MIN_MITRE: f64 = 30.0 * std::f64::consts::PI / 180.0;

/// How a wall's horizontal mullions meet the other's round a corner: the two walls' lines, and the mullions the other has there.
#[derive(Clone, Debug)]
struct Mitre {
    start: P2,
    u: P2,
    plane: f64,
    other_start: P2,
    other_u: P2,
    other_plane: f64,
    /// Whether the other wall's exterior is on the other side of the corner from this one's, as when one is drawn the other way.
    flipped: bool,
    /// The other wall's horizontal mullions at its end there: from and to, up from this wall's base; depth and offset across.
    others: Vec<[f64; 4]>,
}

impl Mitre {
    /// Where bar `b` ends at the mitre, along this wall: on its two faces across it. None without one of its size at its height.
    fn ends(&self, b: &Bar) -> Option<(f64, f64)> {
        let same = |o: &[f64; 4]| {
            (o[0] - b.lo).abs() < SAME && (o[1] - b.hi).abs() < SAME && (o[2] - b.depth).abs() < SAME && (o[3] - b.offset).abs() < SAME
        };
        if !self.others.iter().any(same) {
            return None;
        }
        let along = |mine: f64, theirs: f64| {
            let (a, c) =
                (self.start + self.u.perp() * (self.plane + mine), self.other_start + self.other_u.perp() * (self.other_plane + theirs));
            let at = (c - a).cross(self.other_u) / self.u.cross(self.other_u);
            at.is_finite().then_some(at)
        };
        let (near, far) = b.across;
        let (with_near, with_far) = if self.flipped { (b.offset * 2.0 - near, b.offset * 2.0 - far) } else { (near, far) };
        Some((along(near, with_near)?, along(far, with_far)?))
    }
}

/// How `w`'s horizontal mullions meet the other wall's where `meet` says, if that corner isn't too sharp for a mitre.
fn mitre_at(m: &Model, w: &Wall, meet: Option<&Meeting>) -> Option<Mitre> {
    let meet = meet?;
    let o = meet.other;
    let l = pattern(m, o)?;
    let (node, far_w, far_o) = (
        if meet.at_end { w.end } else { w.start },
        if meet.at_end { w.start } else { w.end },
        if meet.other_end { o.start } else { o.end },
    );
    let (u, v) = ((far_w - node).unit(), (far_o - node).unit());
    if u.dot(v).clamp(-1.0, 1.0).acos() < MIN_MITRE {
        return None;
    }
    let (su, ou) = ((w.end - w.start).unit(), (o.end - o.start).unit());
    let (mine_inside, theirs_inside) = (v.dot(su.perp()) > 0.0, u.dot(ou.perp()) > 0.0);
    // The other wall's horizontal mullions at its end there, as `build` would have them.
    let (length, height) = (Axis::of(o).length(), m.own_top(o) - m.own_base(o));
    let (mut columns, mut rows) = (divide(&l.columns, length), rows_of(l, o, height));
    if let Some(e) = o.curtain.as_ref().filter(|e| !e.grids.is_empty()) {
        by_hand(&mut columns, e, GridWay::Vertical, length, l.columns.segments.len());
        by_hand(&mut rows, e, GridWay::Horizontal, height, l.rows.segments.len());
    }
    let column = if meet.other_end { columns.last() } else { columns.first() }?;
    let shift = m.own_base(o) - m.own_base(w);
    let edited = |at: Address| o.curtain.as_ref().and_then(|e| e.parts.iter().find(|p| p.at == at));
    let gone = o.curtain.as_ref().map_or_else(Merged::default, |e| merges(&columns, &rows, &e.removed, &e.walled)).gone;
    let mut others = Vec::with_capacity(rows.len() + 1);
    for j in 0..=rows.len() {
        let (pick, place, row) = match rows.get(j) {
            _ if j == 0 => (l.borders.bottom.as_ref(), At::Start, rows.first().map_or(0, |r| r.index)),
            Some(r) if r.added => (l.first_pick(GridWay::Horizontal), At::Line(r.from), r.index),
            Some(r) => (l.horizontal.get(r.seg).and_then(|x| x.get(column.seg)).and_then(Option::as_ref), At::Line(r.from), r.index),
            None => (l.borders.top.as_ref(), At::End(height), past_line(&rows)),
        };
        let name = match edited(Address::Horizontal { column: column.index, row }) {
            _ if gone.contains(&Address::Horizontal { column: column.index, row }) => None,
            Some(e) => e.type_name.as_deref(),
            None => pick.map(|x| x.type_name.as_str()),
        };
        if let Some(b) = Bar::of(m, name, Join::Butt, place) {
            others.push([b.lo + shift, b.hi + shift, b.depth, b.offset]);
        }
    }
    Some(Mitre {
        start: w.start,
        u: su,
        plane: grid_plane(m, w),
        other_start: o.start,
        other_u: ou,
        other_plane: grid_plane(m, o),
        flipped: mine_inside != theirs_inside,
        others,
    })
}

/// The corner mullion standing where `meet` says, in `w`'s terms, and its type: the holder's edit there, or its type's.
fn post_at(m: &Model, w: &Wall, meet: Option<&Meeting>) -> Option<(Post, String)> {
    let meet = meet?;
    let (holder, holder_end, other) = if meet.holds { (w, meet.at_end, meet.other) } else { (meet.other, meet.other_end, w) };
    let at = Address::Corner { column: i64::from(holder_end), row: 0 };
    let name = match holder.curtain.as_ref().and_then(|e| e.parts.iter().find(|p| p.at == at)) {
        Some(edit) => edit.type_name.as_deref(),
        None => pattern(m, holder)?.corner.as_deref(),
    }?;
    let t = m.types.corner_mullion.get(name)?;
    let (ua, ub) = ((holder.end - holder.start).unit(), (other.end - other.start).unit());
    let (a, b) =
        (holder.start + ua.perp() * (grid_plane(m, holder) + t.offset_y), other.start + ub.perp() * (grid_plane(m, other) + t.offset_x));
    let turn_of = ua.cross(ub);
    if turn_of == 0.0 {
        return None;
    }
    let centre = a + ua * ((b - a).cross(ub) / turn_of);
    // Toward the corner along the one, and turned toward the other as it leaves it.
    let heading = if holder_end { ua } else { ua * -1.0 };
    let node = if holder_end { holder.end } else { holder.start };
    let far = if node.dist(other.start) <= node.dist(other.end) { other.end } else { other.start };
    let side = heading.cross((far - node).unit()).signum();
    // The corner's turn, half of which keeps it square to the diagonal.
    let auto = if t.auto_rotation { heading.dot((far - node).unit()).clamp(-1.0, 1.0).acos() / 2.0 } else { 0.0 };
    let angle = (t.rotation.to_radians() + auto) * side;
    let tangent = P2(heading.0 * angle.cos() - heading.1 * angle.sin(), heading.0 * angle.sin() + heading.1 * angle.cos());
    let across = tangent.perp();
    let (hw, hd) = (t.width() / 2.0, t.depth() / 2.0);
    let corners = [(-hw, -hd), (hw, -hd), (hw, hd), (-hw, hd)].map(|(x, y)| centre + tangent * x + across * y);
    if !(centre.is_finite() && corners.iter().all(|c| c.is_finite())) {
        return None;
    }
    let u = (w.end - w.start).unit();
    let plane = grid_plane(m, w);
    let local = |q: P2| [(q - w.start).dot(u), (q - w.start).dot(u.perp()) - plane];
    let base = m.own_base(w);
    let v = (m.own_base(w).min(m.own_base(other)) - base, m.own_top(w).max(m.own_top(other)) - base);
    let mut turn = tangent.dot(u.perp()).atan2(tangent.dot(u));
    if turn > std::f64::consts::FRAC_PI_2 + 1e-9 {
        turn -= std::f64::consts::PI;
    } else if turn < -std::f64::consts::FRAC_PI_2 - 1e-9 {
        turn += std::f64::consts::PI;
    }
    // The other wall, on from the corner and back past it, as far as its line could cross this one. Only where the two would
    // still cross, each stopped at the mullion: a sharp corner, past what the mullion covers.
    let partner = meet.other;
    let (up, plane_o) = ((partner.end - partner.start).unit(), grid_plane(m, partner));
    let reach_o = Axis::of(partner).length().max(Axis::of(w).length()) * 2.0;
    let strip = |band: (f64, f64)| {
        [(-reach_o, band.0), (reach_o, band.0), (reach_o, band.1), (-reach_o, band.1)]
            .map(|(a, c)| local(partner.start + up * a + up.perp() * (plane_o + c)))
    };
    let (mine, theirs) = (pattern(m, w), pattern(m, partner));
    let collide = |of: BandOf| -> Option<[[f64; 2]; 4]> {
        let (a, b) = (of(m, mine?)?, of(m, theirs?)?);
        let (x, y) = (stopped(m, w, meet.at_end, a, &corners), stopped(m, partner, meet.other_end, b, &corners));
        rings_cross(&x, &y).then(|| strip(b))
    };
    let (glass, bars) = (collide(panel_band), collide(bar_band));
    Some((Post { ring: corners.map(local), centre: local(centre), size: [t.width(), t.depth()], glass, bars, v, turn }, name.to_string()))
}

impl CurtainLayout {
    /// The mullion a grid added by hand takes: the pattern's A1 of its way, or with none there, the first it has.
    fn first_pick(&self, way: GridWay) -> Option<&MullionPick> {
        let edges = if way == GridWay::Vertical { &self.vertical } else { &self.horizontal };
        let a1 = edges.first().and_then(|row| row.first()).and_then(Option::as_ref);
        a1.or_else(|| edges.iter().flatten().flatten().next()).or_else(|| self.picks().next())
    }
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
        let (panel, mullion, corner) =
            (Kind::Type("curtainpanel".into()), Kind::Type("mullion".into()), Kind::Type("cornermullion".into()));
        for e in &mut self.parts {
            let kind = match e.at {
                Address::Cell { .. } => &panel,
                Address::Corner { .. } => &corner,
                _ => &mullion,
            };
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
        "corner" => Some(Address::Corner { column, row }),
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
    if let (Some(p), Some(s)) = (part, shape)
        && (p.turn != 0.0 || s.plan.is_some())
    {
        return Some(p.corners(s).map(|q| axis.at(q[0], plane + q[1])));
    }
    Some([c(u0, -half), c(u1, -half), c(u1, half), c(u0, half)])
}

/// Where a grid line of way `way` at `at` would run on the wall's grid plane: its two ends.
pub fn line_on_face(m: &Model, w: &Wall, way: GridWay, at: f64) -> [[f64; 3]; 2] {
    let (axis, plane, base) = (Axis::of(w), grid_plane(m, w), m.own_base(w));
    let c = |along: f64, up: f64| {
        let q = axis.at(along, plane);
        [q.0, q.1, base + up]
    };
    match way {
        GridWay::Vertical => [c(at, 0.0), c(at, m.own_top(w) - base)],
        GridWay::Horizontal => [c(0.0, at), c(axis.length(), at)],
    }
}

/// A curtain wall's outline on a plan: what shows it selected, its parts left as they are until Tab picks one.
pub fn outline_on_plan(m: &Model, w: &Wall) -> [P2; 4] {
    let axis = Axis::of(w);
    let (ext, int) = crate::walls::faces(m, w);
    [axis.at(0.0, int), axis.at(axis.length(), int), axis.at(axis.length(), ext), axis.at(0.0, ext)]
}

/// A curtain wall's outline on its grid plane, in 3D: as [`outline_on_plan`].
pub fn outline_on_face(m: &Model, w: &Wall) -> [[f64; 3]; 4] {
    let (axis, plane, base, top) = (Axis::of(w), grid_plane(m, w), m.own_base(w), m.own_top(w));
    let c = |along: f64, z: f64| {
        let q = axis.at(along, plane);
        [q.0, q.1, z]
    };
    [c(0.0, base), c(axis.length(), base), c(axis.length(), top), c(0.0, top)]
}

/// A segment of a grid line, its ends on the wall's grid plane.
pub fn segment_on_face(m: &Model, w: &Wall, at: Address) -> Option<[[f64; 3]; 2]> {
    let l = layout(m, w);
    let span = |spans: &[Span], i: i64| spans.iter().find(|s| s.index == i).copied();
    let [(u0, v0), (u1, v1)] = match at {
        Address::Vertical { column, row } => {
            let (x, r) = (span(&l.columns, column)?.from, span(&l.rows, row)?);
            [(x, r.from), (x, r.to)]
        }
        Address::Horizontal { column, row } => {
            let (c, y) = (span(&l.columns, column)?, span(&l.rows, row)?.from);
            [(c.from, y), (c.to, y)]
        }
        _ => return None,
    };
    let (axis, plane, base) = (Axis::of(w), grid_plane(m, w), m.own_base(w));
    let c = |along: f64, up: f64| {
        let q = axis.at(along, plane);
        [q.0, q.1, base + up]
    };
    Some([c(u0, v0), c(u1, v1)])
}

/// Where a vertical grid line at `at` crosses the wall on a plan: its two faces.
pub fn line_on_plan(m: &Model, w: &Wall, at: f64) -> [P2; 2] {
    let axis = Axis::of(w);
    let (ext, int) = crate::walls::faces(m, w);
    [axis.at(at, int), axis.at(at, ext)]
}

/// Which of a point's two places on a wall a grid of way `way` takes, if the point is on the wall.
pub fn grid_place(m: &Model, w: &Wall, way: GridWay, along: f64, up: f64) -> Option<f64> {
    let (length, height) = (Axis::of(w).length(), m.own_top(w) - m.own_base(w));
    ((0.0..=length).contains(&along) && (0.0..=height).contains(&up)).then_some(if way == GridWay::Vertical { along } else { up })
}

/// What marks a place in 3D, as [`face_ring`], but for a grid line: a ring for each stretch of it still there, a segment taken off
/// leaving a gap.
pub fn face_rings(m: &Model, w: &Wall, at: Address) -> Vec<[[f64; 3]; 4]> {
    if !at.is_grid() {
        return face_ring(m, w, at).into_iter().collect();
    }
    let l = layout(m, w);
    let Some([u0, u1, v0, v1]) = l.extent(at) else { return Vec::new() };
    let (axis, plane, base) = (Axis::of(w), grid_plane(m, w), m.own_base(w));
    let c = |along: f64, up: f64| {
        let q = axis.at(along, plane);
        [q.0, q.1, base + up]
    };
    // The stretches of the line, each its span across, those gone left out and the rest run together.
    let vertical = matches!(at, Address::VerticalGrid { .. });
    let spans = if vertical { &l.rows } else { &l.columns };
    let line = match at {
        Address::VerticalGrid { column, .. } => column,
        Address::HorizontalGrid { row, .. } => row,
        _ => return Vec::new(),
    };
    let mut out: Vec<(f64, f64)> = Vec::new();
    for s in spans {
        let segment =
            if vertical { Address::Vertical { column: line, row: s.index } } else { Address::Horizontal { column: s.index, row: line } };
        if l.gone.contains(&segment) {
            continue;
        }
        match out.last_mut().filter(|last| (last.1 - s.from).abs() < SAME) {
            Some(last) => last.1 = s.to,
            None => out.push((s.from, s.to)),
        }
    }
    out.into_iter()
        .map(|(a, b)| if vertical { [c(u0, a), c(u1, a), c(u1, b), c(u0, b)] } else { [c(a, v0), c(b, v0), c(b, v1), c(a, v1)] })
        .collect()
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

impl Address {
    /// The kind of part at this place.
    pub fn kind(self) -> PartKind {
        if matches!(self, Address::Cell { .. }) { PartKind::Panel } else { PartKind::Mullion }
    }

    /// Whether it's a grid line, and no part.
    pub fn is_grid(self) -> bool {
        matches!(self, Address::VerticalGrid { .. } | Address::HorizontalGrid { .. })
    }

    /// The same place with column or row `from` of way `way` called `to`.
    fn renamed(self, way: GridWay, from: i64, to: i64) -> Address {
        let new = |x: i64| if x == from { to } else { x };
        match (self, way) {
            (Address::Cell { column, row }, GridWay::Vertical) => Address::Cell { column: new(column), row },
            (Address::Cell { column, row }, GridWay::Horizontal) => Address::Cell { column, row: new(row) },
            (Address::Vertical { column, row }, GridWay::Vertical) => Address::Vertical { column: new(column), row },
            (Address::Vertical { column, row }, GridWay::Horizontal) => Address::Vertical { column, row: new(row) },
            (Address::Horizontal { column, row }, GridWay::Vertical) => Address::Horizontal { column: new(column), row },
            (Address::Horizontal { column, row }, GridWay::Horizontal) => Address::Horizontal { column, row: new(row) },
            (other, _) => other,
        }
    }
}

impl Part {
    /// A turned or a mitred part's corners from above, as (along, toward the exterior from the grid's plane).
    pub fn corners(&self, s: &Shape) -> [[f64; 2]; 4] {
        if let Some(q) = s.plan {
            return q.map(|c| [self.centre[0] + c[0], self.centre[2] + c[1]]);
        }
        let (cos, sin) = (self.turn.cos(), self.turn.sin());
        let [hw, _, hd] = s.size.map(|x| x / 2.0);
        [(-hw, -hd), (hw, -hd), (hw, hd), (-hw, hd)].map(|(x, y)| [self.centre[0] + x * cos - y * sin, self.centre[2] + x * sin + y * cos])
    }
}

impl Layout {
    /// What's at a point of the wall, along it and up from its base: the mullions' places within `reach` of it, then its cell.
    pub fn at(&self, along: f64, up: f64, reach: f64) -> Vec<Address> {
        let within = |spans: &[Span], x: f64| {
            spans.iter().position(|s| s.from <= x && x < s.to).or_else(|| spans.last().filter(|s| x == s.to).map(|_| spans.len() - 1))
        };
        let (Some(c), Some(r)) = (within(&self.columns, along), within(&self.rows, up)) else { return Vec::new() };
        let (column, row) = (&self.columns[c], &self.rows[r]);
        let next = |spans: &[Span], i: usize| spans.get(i + 1).map_or_else(|| past_line(spans), |n| n.index);
        let mut out = Vec::new();
        for p in self.parts.iter().filter(|p| matches!(p.address, Address::Corner { .. })) {
            let half = self.shapes.get(p.shape as usize).map_or(0.0, |s| s.size[0].max(s.size[2]) / 2.0);
            if (along - p.centre[0]).abs() <= half + reach {
                out.push(p.address);
            }
        }
        // A wall's own edge is no grid to move, the first line nor the one past the last.
        // A segment taken off is no line here, and no mullion to pick.
        let gone = &self.gone;
        let mut column_line = |line: i64, inner: bool| {
            if gone.contains(&Address::Vertical { column: line, row: row.index }) {
                return;
            }
            out.push(Address::Vertical { column: line, row: row.index });
            if inner {
                out.push(Address::VerticalGrid { column: line, row: 0 });
            }
        };
        if (along - column.from).abs() <= reach {
            column_line(column.index, c > 0);
        }
        if (column.to - along).abs() <= reach {
            column_line(next(&self.columns, c), c + 1 < self.columns.len());
        }
        let mut row_line = |line: i64, inner: bool| {
            if gone.contains(&Address::Horizontal { column: column.index, row: line }) {
                return;
            }
            out.push(Address::Horizontal { column: column.index, row: line });
            if inner {
                out.push(Address::HorizontalGrid { column: 0, row: line });
            }
        };
        if (up - row.from).abs() <= reach {
            row_line(row.index, r > 0);
        }
        if (row.to - up).abs() <= reach {
            row_line(next(&self.rows, r), r + 1 < self.rows.len());
        }
        let (first, first_row) = self.merged.get(&(column.index, row.index)).copied().unwrap_or((column.index, row.index));
        out.push(Address::Cell { column: first, row: first_row });
        out
    }

    /// The segment of a grid line a cursor at `along` and `up` is nearest, of the edges of its cell inside the wall.
    pub fn segment_near(&self, along: f64, up: f64) -> Option<Address> {
        let within = |spans: &[Span], x: f64| spans.iter().position(|s| s.from <= x && x <= s.to);
        let (c, r) = (within(&self.columns, along)?, within(&self.rows, up)?);
        let (column, row) = (&self.columns[c], &self.rows[r]);
        let mut edges = Vec::with_capacity(4);
        if c > 0 {
            edges.push((along - column.from, Address::Vertical { column: column.index, row: row.index }));
        }
        if let Some(next) = self.columns.get(c + 1) {
            edges.push((column.to - along, Address::Vertical { column: next.index, row: row.index }));
        }
        if r > 0 {
            edges.push((up - row.from, Address::Horizontal { column: column.index, row: row.index }));
        }
        if let Some(next) = self.rows.get(r + 1) {
            edges.push((row.to - up, Address::Horizontal { column: column.index, row: next.index }));
        }
        edges.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, a)| a)
    }

    /// The places of the mullions along the grid line of the mullion at `at`: all its row's or column's, part or none.
    pub fn along_grid(&self, at: Address) -> Vec<Address> {
        match at {
            Address::Vertical { column, .. } => self.rows.iter().map(|r| Address::Vertical { column, row: r.index }).collect(),
            Address::Horizontal { row, .. } => self.columns.iter().map(|c| Address::Horizontal { column: c.index, row }).collect(),
            _ => Vec::new(),
        }
    }

    /// The grid line a mullion runs along, unless it's on the wall's own edge.
    pub fn grid_of(&self, at: Address) -> Option<Address> {
        let inner = |spans: &[Span], i: i64| spans.iter().skip(1).any(|s| s.index == i);
        match at {
            Address::Vertical { column, .. } if inner(&self.columns, column) => Some(Address::VerticalGrid { column, row: 0 }),
            Address::Horizontal { row, .. } if inner(&self.rows, row) => Some(Address::HorizontalGrid { column: 0, row }),
            _ => None,
        }
    }

    /// Where a grid line is: along the wall for a vertical one, up from its base for a horizontal one.
    pub fn grid_at(&self, at: Address) -> Option<f64> {
        let on = |spans: &[Span], i: i64| spans.iter().skip(1).find(|s| s.index == i).map(|s| s.from);
        match at {
            Address::VerticalGrid { column, .. } => on(&self.columns, column),
            Address::HorizontalGrid { row, .. } => on(&self.rows, row),
            _ => None,
        }
    }

    /// Whether a grid line is a manual one.
    pub fn grid_is_manual(&self, at: Address) -> bool {
        let of = |spans: &[Span], i: i64| spans.iter().any(|s| s.index == i && s.manual);
        match at {
            Address::VerticalGrid { column, .. } => of(&self.columns, column),
            Address::HorizontalGrid { row, .. } => of(&self.rows, row),
            _ => false,
        }
    }

    /// Whether a line would stand within `reach` of another of `way`'s; `except`, the line that's moving, doesn't count.
    pub fn line_near(&self, way: GridWay, at: f64, reach: f64, except: Option<i64>) -> bool {
        let spans = if way == GridWay::Vertical { &self.columns } else { &self.rows };
        spans.iter().skip(1).any(|s| Some(s.index) != except && (s.from - at).abs() <= reach)
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
        let whole = |spans: &'_ [Span]| (spans.first().map_or(0.0, |s| s.from), spans.last().map_or(0.0, |s| s.to));
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
            // It has a part, or `has` said no.
            Address::Corner { .. } => return None,
            Address::VerticalGrid { .. } => {
                let (x, (v0, v1)) = (self.grid_at(at)?, whole(&self.rows));
                [x - half, x + half, v0, v1]
            }
            Address::HorizontalGrid { .. } => {
                let ((u0, u1), y) = (whole(&self.columns), self.grid_at(at)?);
                [u0, u1, y - half, y + half]
            }
        })
    }

    /// Whether the place is on the wall now, a part there or none.
    pub fn has(&self, at: Address) -> bool {
        let (columns, rows) = (&self.columns, &self.rows);
        let on = |spans: &[Span], i: i64| spans.iter().any(|s| s.index == i);
        let past = |spans: &[Span], i: i64| !spans.is_empty() && past_line(spans) == i;
        match at {
            Address::Cell { column, row } => on(columns, column) && on(rows, row),
            Address::Vertical { column, row } => (on(columns, column) || past(columns, column)) && on(rows, row),
            Address::Horizontal { column, row } => on(columns, column) && (on(rows, row) || past(rows, row)),
            Address::Corner { column, .. } => usize::try_from(column).ok().and_then(|c| self.corners.get(c)).copied().unwrap_or(false),
            Address::VerticalGrid { .. } | Address::HorizontalGrid { .. } => self.grid_at(at).is_some(),
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
        if let Some(n) = &mut self.corner {
            f(&Kind::Type("cornermullion".into()), n);
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
            corner: None,
            anchor_rows: false,
        }
    }
}

/// A pattern from a `type` command's params: as a project holds it, with `panel` and `mullion` to fill what it leaves out
/// (the project's first unless they're given; null for none).
pub(crate) fn from_params(m: &Model, v: &Value) -> Result<CurtainLayout, String> {
    let mut o = v.as_object().cloned().ok_or("`curtain` is {columns: {segments, fit?, justify?}, rows: {…}, panel?, mullion?, panels?, vertical?, horizontal?, borders?, continuous?, min_panel?, corner?, anchor_rows?}")?;
    let name = |key: &str, category: &str, o: &mut serde_json::Map<String, Value>| match o.remove(key) {
        None => Ok(m.default_type(category).map(String::from)),
        Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => Err(format!("`{key}` is a type's name or null, not {other}")),
    };
    let (panel, mullion) = (name("panel", "curtainpanel", &mut o)?, name("mullion", "mullion", &mut o)?);
    if !o.contains_key("corner") {
        o.insert("corner".into(), json!(m.default_type("cornermullion")));
    }
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

/// Whether [`out_of_date`] can say yes of a model a command made of `before`, a model that was in date, without looking at
/// every wall: only a change to a type, a setting or a level, or to a wall that holds a pattern's edits or stands in a cell, can
/// put one out of date.
pub(crate) fn may_be_out_of_date(m: &Model, before: &Model) -> bool {
    let crate::ids::Changed::Ids(changed) = m.changed_since(before) else { return true };
    let reaches = |e: &Element| match e {
        Element::Level(_) => true,
        Element::Wall(w) => w.panel_of.is_some() || pattern(m, w).is_some() || pattern(before, w).is_some(),
        _ => false,
    };
    changed.iter().any(|id| [m, before].iter().any(|x| x.elements.get(*id).is_some_and(reaches)))
}

/// Whether a curtain wall type's thickness isn't what its panels and mullions make it.
pub(crate) fn out_of_date(m: &Model) -> bool {
    m.types.wall.values().any(|t| t.curtain.as_ref().is_some_and(|l| (t.thickness - thickness(m, l)).abs() > 1e-9))
        || !panel_plan(m).is_empty()
        || !levels_plan(m).is_empty()
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
    for (id, levels) in levels_plan(m) {
        if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
            let mut e = w.curtain.take().unwrap_or_default();
            e.levels = levels;
            w.curtain = (!e.is_empty()).then_some(e);
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
    if let Some(c) = l.corner.as_ref().filter(|c| !m.types.corner_mullion.contains_key(*c)) {
        return Err(format!("{who}: there is no corner mullion type `{c}`; make it with `type`"));
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
        let (known, family) = match e.at {
            Address::Cell { .. } => (m.types.curtain_panel.contains_key(name), "curtain panel"),
            Address::Corner { .. } => (m.types.corner_mullion.contains_key(name), "corner mullion"),
            _ => (m.types.mullion.contains_key(name), "mullion"),
        };
        if !known {
            return Err(format!("curtain wall {id}: an edited part names `{name}`, which isn't a {family} type; add it with `type`"));
        }
    }
    let grids = w.curtain.as_ref().map_or(&[][..], |c| &c.grids[..]);
    if grids.len() > MAX_GRIDS {
        return Err(format!("curtain wall {id}: it has {} grids by hand; a wall has at most {MAX_GRIDS}", grids.len()));
    }
    let mut numbers = BTreeSet::new();
    for g in grids {
        if !(g.at.is_finite() && g.at.abs() <= MAX_COORD) {
            return Err(format!("curtain wall {id}: a grid at {} mm; it is on the wall, within {MAX_COORD} mm", g.at));
        }
        if !((0..MANUAL_LINE).contains(&g.number) && numbers.insert((g.way == GridWay::Vertical, g.number))) {
            return Err(format!("curtain wall {id}: grid number {} is out of range or used twice", g.number));
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
