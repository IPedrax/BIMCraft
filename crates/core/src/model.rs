//! The building model: levels, grids, walls, floors, ceilings, shafts, masses, doors, windows, rooms,
//! columns, stairs, ramps, railings and roofs, the types they use, the dimensions, tags, text notes and spot elevations annotating
//! views, the sections, elevations, schedules and 3D views looking at it, and the sheets its views
//! are placed on. Lengths are millimetres, x east, y north, z up; each level has an elevation. In
//! a section or an elevation, millimetres along its line from its start, and up; on a sheet,
//! millimetres of paper from its bottom left corner.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::cloud::Cloud;
use crate::family::{Family, Piece};
pub use crate::fills::FilledRegion;
use crate::fills::{self, FillPattern, FilledRegionType};
use crate::geom::{self, P2, Region};
use crate::graphics::{self, Graphics, ViewFilter, ViewTemplate};
pub use crate::groups::{DetailGroup, RepeatingDetail};
use crate::language::Language;
pub use crate::lines::DetailLine;
use crate::lines::{self, LinePattern, LineStyle};
use crate::phasing::{self, OptionSet, PhaseFilter, Status};
use crate::texture::{Kind, MAX_IMAGES, Picture, Texture};
use crate::units::Units;
use crate::walls;

pub use crate::ids::{Elements, FIRST_ID, Guid, Id, Key};

/// A line in plan, through two points.
pub type Line = (P2, P2);

/// The least a section box runs on each axis, mm.
pub const MIN_SECTION_BOX: f64 = 10.0;

/// Height of a plan view's cut plane above its level, unless its view range says otherwise; rooms
/// find the walls that bound them at this height.
pub const CUT_HEIGHT: f64 = 1200.0;
/// A text note's capitals, on paper, unless told otherwise: mm.
pub const NOTE_SIZE: f64 = 2.5;
/// Wall ends closer than this join, in mm.
pub const TOL: f64 = 1.0;
/// Coordinates and lengths beyond 1000 km are refused.
pub const MAX_COORD: f64 = 1e9;
/// Height of a wall or room with no level above it.
pub const DEFAULT_HEIGHT: f64 = 3000.0;
/// A stair's tallest riser, its treads' depth and its width, unless told otherwise, in mm.
pub const RISER: f64 = 180.0;
pub const TREAD: f64 = 280.0;
pub const STAIR_WIDTH: f64 = 1000.0;
/// A ramp's steepest slope, percent (1 in 12), and its width, unless told otherwise.
pub const RAMP_SLOPE: f64 = 100.0 / 12.0;
pub const RAMP_WIDTH: f64 = 1200.0;
/// How far a railing on a stair or a ramp runs in from its edge, mm.
pub const RAILING_INSET: f64 = 50.0;
/// Balusters one railing may have.
pub const MAX_BALUSTERS: f64 = 10_000.0;
/// Names, marks and numbers longer than this are refused.
pub const MAX_TEXT: usize = 256;
/// Points one command or floor outline may carry.
pub const MAX_POINTS: usize = 10_000;
/// Height of a reflected ceiling plan's cut plane above its level: it looks up from there.
pub const CEILING_CUT: f64 = 2300.0;
/// Height of a ceiling's underside above its level, unless told otherwise.
pub const CEILING_HEIGHT: f64 = 2600.0;
/// The steepest slope a floor or a roof may have, degrees.
pub const MAX_SLOPE: f64 = 75.0;
/// The version of the project file this build writes, and the newest it reads. 6: a project holds detailing (library kinds
/// `line-pattern`, `line-style`, `fill-pattern`, `filled-region-type` and `detail-group`; the elements `line`, `region`,
/// `detailgroup` and `repeatingdetail`; sketch parts; the model keys `line_styles`, `fill_patterns` and `region_types`);
/// nothing in a file of 5 changes, but a build that reads up to 5 would answer one of 6 with a serde error ("isn't a kind of
/// library entry", "unknown variant"), not "saved by a newer BIMCraft". 5: a project holds scans (`clouds`, library
/// entries of kind `Cloud`) and point clouds place them; nothing in a file of 4 changes, but a build that reads up to 4 would
/// answer one of 5 with a serde error, not "saved by a newer BIMCraft". 4: a line weight is a pen (0 to 16), not
/// pixels of a sheet's drawing. 3: a type's layers and a family's forms and
/// symbols hold ids and GUIDs; a family's types and parameters are library entries, and what uses them names them by GUID.
/// 2: the library has ids and GUIDs too
/// (`library`), and what names a library entry holds its GUID. 1: elements have GUIDs, ids six digits, and what points at
/// an element holds its GUID. None (0): a file from before.
pub const FORMAT: u32 = 6;
/// How far past the model a level reaches without an extent of its own, mm.
pub const LEVEL_PAST: f64 = 1000.0;

/// A level, and its floor plan: what the plan sees (its view range), and the part of it the plan
/// shows (its crop region, corners south-west then north-east; all of it without one). As a datum
/// it is a horizontal plane over its `extent`, corners south-west then north-east (without one, the
/// model's, [`LEVEL_PAST`] past it): sections and elevations draw it where they see that, and the 3D
/// view its outline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub name: String,
    pub elevation: f64,
    #[serde(default)]
    pub view_range: ViewRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<[P2; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<[P2; 2]>,
    /// What its plan hides and how it draws the rest.
    #[serde(default, skip_serializing_if = "Graphics::is_default")]
    pub graphics: Graphics,
    /// Whether its plan shows what the default 3D view's section box keeps, while it's on: the box's heights as its view
    /// range and its sides as its crop.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub section_box: bool,
}

impl Level {
    pub fn new(name: impl Into<String>, elevation: f64) -> Level {
        Level {
            name: name.into(),
            elevation,
            view_range: ViewRange::default(),
            crop: None,
            extent: None,
            graphics: Graphics::default(),
            section_box: false,
        }
    }
}

/// A plan's view range, as Revit's: mm above its level. Cut by the `cut` plane, seen from it down
/// to the `bottom` and, beyond, to the view `depth`; above the cut, up to the `top` (the level
/// above when none) for what overhangs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewRange {
    pub top: Option<f64>,
    pub cut: f64,
    pub bottom: f64,
    pub depth: f64,
}

impl Default for ViewRange {
    fn default() -> ViewRange {
        ViewRange { top: None, cut: CUT_HEIGHT, bottom: 0.0, depth: 0.0 }
    }
}

/// A plan's view range as heights: its top, cut plane, bottom and view depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanRange {
    pub top: f64,
    pub cut: f64,
    pub bottom: f64,
    pub depth: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grid {
    pub name: String,
    pub start: P2,
    pub end: P2,
}

/// A wall: its location line from `start` to `end`, straight or, turning `arc` degrees, an arc;
/// which line of the wall that is (its `location`); its exterior side to the left of start to end,
/// as Revit's, so walls drawn clockwise face out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wall {
    #[serde(rename = "type")]
    pub type_name: String,
    /// Base level.
    pub level: Guid,
    /// Ends of the location line.
    pub start: P2,
    pub end: P2,
    /// Top constraint: a level, or none for an unconnected wall `height` tall.
    #[serde(default)]
    pub top_level: Option<Guid>,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub base_offset: f64,
    /// How far it turns from its start to its end, degrees counter-clockwise: 0 straight, 180 a
    /// half circle bulging to its right.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub arc: f64,
    #[serde(default, skip_serializing_if = "is_centre")]
    pub location: Location,
    /// The roof or floor its top follows, from below, instead of its top constraint; and the one its
    /// base stands on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_top: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_base: Option<Guid>,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

fn is_centre(l: &Location) -> bool {
    *l == Location::Centre
}

/// The line of a wall its ends lie on, as Revit's location line: its centre, its core's centre, or
/// a face, of its finish or of its core, on its exterior or its interior side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Location {
    #[default]
    #[serde(alias = "center")]
    Centre,
    #[serde(alias = "core-center")]
    CoreCentre,
    ExteriorFinish,
    InteriorFinish,
    ExteriorCore,
    InteriorCore,
}

impl Location {
    pub const ALL: [Location; 6] = [
        Location::Centre,
        Location::CoreCentre,
        Location::ExteriorFinish,
        Location::InteriorFinish,
        Location::ExteriorCore,
        Location::InteriorCore,
    ];

    /// Its name in commands and files.
    pub fn name(self) -> &'static str {
        match self {
            Location::Centre => "centre",
            Location::CoreCentre => "core-centre",
            Location::ExteriorFinish => "exterior-finish",
            Location::InteriorFinish => "interior-finish",
            Location::ExteriorCore => "exterior-core",
            Location::InteriorCore => "interior-core",
        }
    }

    /// As a menu names it.
    pub fn label(self) -> &'static str {
        match self {
            Location::Centre => "Wall Centreline",
            Location::CoreCentre => "Core Centreline",
            Location::ExteriorFinish => "Finish Face: Exterior",
            Location::InteriorFinish => "Finish Face: Interior",
            Location::ExteriorCore => "Core Face: Exterior",
            Location::InteriorCore => "Core Face: Interior",
        }
    }

    pub fn from_name(name: &str) -> Option<Location> {
        let n = name.trim().to_ascii_lowercase().replace("center", "centre");
        Location::ALL.into_iter().find(|l| l.name() == n)
    }
}

/// What a layer of a wall does: carries it (structure, its core), backs another (substrate),
/// insulates (thermal), seals it (a membrane, which may be as thin as nothing), or finishes a face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Function {
    #[default]
    Structure,
    Substrate,
    Thermal,
    Membrane,
    Finish,
}

/// A layer of a compound type: of a wall, from its exterior in; of a floor, a roof or a ceiling,
/// from its top down. Its material is one of the project's library's, or none.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    /// Its id and GUID, as an element's: given when it's made, kept while it stays where it is in its list.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    #[serde(default)]
    pub function: Function,
    #[serde(default)]
    pub material: String,
    pub thickness: f64,
}

/// Layers a type may have.
pub const MAX_LAYERS: usize = 32;

/// A material of the project's library: its colour in shaded views, the pattern its cut shows in
/// plans and sections, and the one its surfaces show where a plan looks down on a floor or a
/// ceiling plan up at a ceiling.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub color: [u8; 3],
    #[serde(default)]
    pub cut: Pattern,
    #[serde(default)]
    pub surface: Pattern,
    /// The project's fill patterns its cut and its surfaces show, by name, as Revit's materials: over `cut` and `surface`
    /// when given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_pattern: Option<String>,
    /// What its surfaces look like in the 3D view, over its colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<Texture>,
}

impl Default for Material {
    fn default() -> Material {
        Material {
            color: [200, 200, 200],
            cut: Pattern::None,
            surface: Pattern::None,
            cut_pattern: None,
            surface_pattern: None,
            texture: None,
        }
    }
}

/// A drafting pattern, drawn in code: its lines a few pixels apart whatever the scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pattern {
    #[default]
    None,
    Solid,
    Diagonal,
    Crosshatch,
    Horizontal,
    Vertical,
    Grid,
    Brick,
    Concrete,
    Insulation,
}

impl Pattern {
    pub const ALL: [Pattern; 10] = [
        Pattern::None,
        Pattern::Solid,
        Pattern::Diagonal,
        Pattern::Crosshatch,
        Pattern::Horizontal,
        Pattern::Vertical,
        Pattern::Grid,
        Pattern::Brick,
        Pattern::Concrete,
        Pattern::Insulation,
    ];
}

/// A slab under its top, along `boundary` (counter-clockwise), open through its `openings` (holes
/// inside it, apart): its top at its level, `offset` mm above it; with a slope arrow, from tail to
/// head in plan, the top there and rising `slope` degrees toward the head (falling, negative). It's
/// its type's thickness across its slope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Floor {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Guid,
    pub boundary: Vec<P2>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Vec<P2>>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub slope: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow: Option<[P2; 2]>,
    /// Carries load: a structural floor, as Revit's Floor: Structural makes one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub structural: bool,
    /// Where its top's model pattern starts, and how far it's turned, degrees: Revit's aligned and rotated pattern.
    #[serde(default, skip_serializing_if = "crate::fills::at_origin")]
    pub pattern_origin: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pattern_angle: f64,
    /// The sketch it was drawn or last edited by: its lines, each a part with an id and a GUID of its own. Empty when it
    /// was made another way (over the walls, from a file); Edit Boundary then starts from its outline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch: Vec<crate::sketch::SketchLine>,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// A ceiling along `boundary` (counter-clockwise), open through its `openings` (holes inside it,
/// apart), its underside `offset` mm above its level, its type's thickness over that.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ceiling {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Guid,
    pub boundary: Vec<P2>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Vec<P2>>,
    pub offset: f64,
    /// The sketch it was drawn or last edited by: its lines, each a part with an id and a GUID of its own. Empty when it
    /// was made another way (over the walls, from a file); Edit Boundary then starts from its outline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch: Vec<crate::sketch::SketchLine>,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// A shaft opening: a hole along `boundary` through the floors, ceilings and roofs it passes
/// through, from `base_offset` above `level` up to `top_level`, or `height` mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shaft {
    pub level: Guid,
    pub boundary: Vec<P2>,
    #[serde(default)]
    pub top_level: Option<Guid>,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub base_offset: f64,
    /// The sketch it was drawn or last edited by: its lines, each a part with an id and a GUID of its own. Empty when it
    /// was made another way (over the walls, from a file); Edit Boundary then starts from its outline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch: Vec<crate::sketch::SketchLine>,
}

/// A mass, Revit's conceptual building volume: `boundary` (counter-clockwise) extruded from `base_offset` above `level`
/// up to `top_level`, or `height` mm. Walls, floors and roofs are made from its faces; its floors, one for each level it
/// crosses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mass {
    #[serde(default)]
    pub name: String,
    pub level: Guid,
    pub boundary: Vec<P2>,
    #[serde(default)]
    pub top_level: Option<Guid>,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub base_offset: f64,
    /// Its outline at the top, point for point with `boundary` at the base: the mass is lofted straight between the two. A
    /// sloped mass is one whose top outline is its footprint moved sideways. None: it stands straight up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Vec<P2>>,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

impl Mass {
    /// Whether its top outline is not its footprint: it is lofted between the two.
    pub fn lofted(&self) -> bool {
        self.top.as_ref().is_some_and(|t| *t != self.boundary)
    }

    /// Its top outline, counter-clockwise like [`Mass::footprint`] and point for point with it: the footprint itself when it
    /// stands straight up.
    pub fn top_ring(&self) -> Vec<P2> {
        let mut ring = self.top.as_ref().filter(|t| t.len() == self.boundary.len()).unwrap_or(&self.boundary).clone();
        if geom::area(&self.boundary) < 0.0 {
            ring.reverse();
        }
        ring
    }

    /// Where its top face is: a level, and how far over it.
    pub fn top_face(&self) -> (Guid, f64) {
        self.top_level.map_or((self.level, self.base_offset + self.height), |t| (t, 0.0))
    }

    /// Its footprint, counter-clockwise whichever way round `boundary` was given.
    pub fn footprint(&self) -> Vec<P2> {
        let mut ring = self.boundary.clone();
        if geom::area(&ring) < 0.0 {
            ring.reverse();
        }
        ring
    }
}

/// A door or window in a host wall, centred `offset` mm along it from the wall's start. Of a `family`
/// (a door's or a window's own, its `type` one of that family's) or, with none, of one of the project's
/// door or window types.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Opening {
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub host: Guid,
    pub offset: f64,
    /// Height of its bottom above the wall's base (0 for doors).
    #[serde(default)]
    pub sill: f64,
    #[serde(default)]
    pub mark: String,
    /// Swing (doors) or face (windows) to the wall's other side.
    #[serde(default)]
    pub flip_facing: bool,
    /// Hinge on the other jamb.
    #[serde(default)]
    pub flip_hand: bool,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub level: Guid,
    /// A point inside the room. The boundary is found from the walls around it, so it follows
    /// them when they move.
    pub point: P2,
    pub name: String,
    pub number: String,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// Which of an element's lines a dimension measures to: a wall's centre line, its face on the
/// left or the right of its start-to-end direction, or its start or end; a door's or a window's
/// centre, or its jamb toward its wall's start or end. A grid has only its line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    #[default]
    #[serde(alias = "center")]
    Centre,
    Left,
    Right,
    Start,
    End,
}

impl Face {
    /// The same line of a mirrored wall, which runs back the other way: its start and end swap
    /// (a door's or a window's jambs too), its exterior and interior faces stay its left and right.
    pub fn mirrored(self) -> Face {
        match self {
            Face::Start => Face::End,
            Face::End => Face::Start,
            other => other,
        }
    }

    /// The same line of a flipped wall, which runs the other way about the same line: its left and
    /// right swap, and its start and end.
    pub fn flipped(self) -> Face {
        match self {
            Face::Left => Face::Right,
            Face::Right => Face::Left,
            other => other.mirrored(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Face::Centre => "centre line",
            Face::Left => "left face",
            Face::Right => "right face",
            Face::Start => "start",
            Face::End => "end",
        }
    }
}

/// A line a dimension measures to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub id: Guid,
    #[serde(default)]
    pub face: Face,
}

/// How far a point stands from a line `toward` it (a unit vector along x or y): see [`Model::clearances`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clearance {
    pub toward: P2,
    pub distance: f64,
    pub to: Reference,
}

/// A dimension in a level's plan. Aligned: the distances between parallel references, along a
/// line through `at` across them. Angular: the angle between two references that meet, on the
/// side of `at`, its arc through `at`. Radial: a round element's radius, its line toward `at`. It
/// measures the model as it is, so it follows what it measures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    pub level: Guid,
    /// The plan view of `level` it is in, when not the level's own plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<Guid>,
    #[serde(default)]
    pub kind: DimKind,
    pub refs: Vec<Reference>,
    pub at: P2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DimKind {
    #[default]
    Aligned,
    Angular,
    Radial,
}

/// A text note in a view: a level's plan, a section, an elevation or a sheet. `at` is the top left
/// of its first line, in the view's own terms; `size` its capitals' height on paper, mm; turned
/// `angle` degrees counter-clockwise; with a leader to `leader`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub view: Guid,
    pub at: P2,
    pub text: String,
    #[serde(default = "note_size")]
    pub size: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<P2>,
}

fn note_size() -> f64 {
    NOTE_SIZE
}

/// A scan placed: the project's `cloud`, its middle at `at` in plan and `offset` mm over `level`, turned `angle` degrees
/// (counter-clockwise) about its middle. It holds no `params`: phases and paint don't reach it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointCloud {
    pub cloud: String,
    pub level: Guid,
    pub at: P2,
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub angle: f64,
}

/// A spot elevation in a view: in a level's plan, the height of the top of `host` at `at`; in a
/// section or an elevation, of its point `at`, along and up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spot {
    pub view: Guid,
    pub at: P2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Guid>,
}

/// Another project shown in this one, a reference that can't be edited: Revit's linked model. The file stays where it is;
/// what it holds is read when the link is made, and again by `reload`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub name: String,
    /// The `.bimcraft` or `.ifc` it reads, as it was given: a relative one is relative to this project's file.
    pub path: String,
    /// Where its origin stands in this project, and how far it is turned about it, degrees. Its levels stand at their own
    /// elevations.
    pub at: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
}

/// A schedule view: the elements of category `of` in rows, as the `schedule` command lists them:
/// its `fields` (its category's unless it chooses), of the elements its `filters` keep, sorted by
/// its `sort` fields (by its first unless it says) and, `grouped`, a row for each set of their values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub name: String,
    pub of: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<crate::schedule::Field>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<crate::schedule::Filter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<crate::schedule::Sort>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub grouped: bool,
}

impl Schedule {
    /// The category it's of, as elements name theirs: `room`.
    pub fn category(&self) -> String {
        crate::commands::category(&self.of)
    }
}

/// A plan view of a level besides the level's own, as Revit's Duplicate View makes one: it looks at the same model through
/// its own view range, crop and graphics, and holds annotation of its own (a dimension, a tag, a note or a spot elevation says
/// which view it is in). A dependent view, `parent` set, follows its parent's view range and graphics (and so its scale and
/// detail) and has a crop of its own; it shows its parent's annotation as well as its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanView {
    pub level: Guid,
    pub name: String,
    #[serde(default)]
    pub view_range: ViewRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<[P2; 2]>,
    /// What it hides and how it draws the rest.
    #[serde(default, skip_serializing_if = "Graphics::is_default")]
    pub graphics: Graphics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Guid>,
    /// Whether its plan shows what the default 3D view's section box keeps, while it's on: the box's heights as its view
    /// range and its sides as its crop.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub section_box: bool,
}

/// A 3D view, seen from `azimuth` degrees round from east (counter-clockwise) and `elevation`
/// degrees up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View3D {
    pub name: String,
    pub azimuth: f64,
    pub elevation: f64,
    /// What it hides and how it draws the rest.
    #[serde(default, skip_serializing_if = "Graphics::is_default")]
    pub graphics: Graphics,
    /// The box it is cut to, as Revit's Section Box: kept, with its extents, when it's turned off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_box: Option<SectionBox>,
}

impl View3D {
    /// The default 3D view, `{3D}`, as it is before anything changes it.
    pub fn default_view() -> View3D {
        View3D {
            name: "{3D}".into(),
            azimuth: crate::view3d::AZIMUTH,
            elevation: crate::view3d::ELEVATION,
            graphics: Graphics::default(),
            section_box: None,
        }
    }
}

/// A 3D view's section box: six planes square to the axes, from `min` to `max` (mm), keeping what lies between them while it
/// is `on`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
    #[serde(default = "yes")]
    pub on: bool,
}

fn yes() -> bool {
    true
}

/// A door's or window's tag, showing its mark in the plan of its wall's level, `offset` mm from
/// its centre: it goes where its door or window goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tag {
    pub host: Guid,
    /// The plan view of its host's level it is in, when not the level's own plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<Guid>,
    pub offset: P2,
    /// A line from the tag to what it names.
    #[serde(default)]
    pub leader: bool,
    /// The field of its host it shows, by the name a schedule of its category gives it (`Type`, `Height`, a project parameter's);
    /// none for the category's usual one (`Tag::usual`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Tag {
    /// The field a tag of category `cat` shows unless it names one: a door's or window's mark, a room's name, else the type.
    pub fn usual(cat: &str) -> &'static str {
        match cat {
            "door" | "window" => "Mark",
            "room" => "Name",
            _ => "Type",
        }
    }
}

/// A column standing on `level` at `at`, its width along `angle` degrees (counter-clockwise from
/// east): up to `top_level`, or `height` mm tall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Guid,
    pub at: P2,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub top_level: Option<Guid>,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub base_offset: f64,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// Which line of a run's section its heights are: the top of it, its middle, or its bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Justify {
    Top,
    #[default]
    #[serde(alias = "center")]
    Centre,
    Bottom,
}

impl Justify {
    /// Its name, as a project file and IFC write it.
    pub fn word(self) -> &'static str {
        match self {
            Justify::Top => "top",
            Justify::Centre => "centre",
            Justify::Bottom => "bottom",
        }
    }

    /// The one `word` names, or `center`.
    pub fn parse(s: &str) -> Option<Justify> {
        match s {
            "top" => Some(Justify::Top),
            "centre" | "center" => Some(Justify::Centre),
            "bottom" => Some(Justify::Bottom),
            _ => None,
        }
    }
}

/// A straight run from `start` to `end` in plan, of a type (a section `width` across and `depth` deep, or round): a beam,
/// and what the other disciplines draw with it, a pipe, a conduit, a cable tray. Its `offset` mm over its `level` is the
/// height of its `justify` line at the start, and `end_offset` at the end (level when none): a sloped run has both, a
/// vertical one the same `start` and `end` and two heights. Its section is square to its axis. `system` names the system it
/// belongs to; schedules group by it. Under several categories, as an opening is a door or a window: `Element::Beam`,
/// `Element::Pipe` and the others `run_arm!` names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Guid,
    pub start: P2,
    pub end: P2,
    #[serde(default)]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_offset: Option<f64>,
    #[serde(default)]
    pub justify: Justify,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub system: String,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// A family's type placed: at `at` on `level`, `offset` mm up from it, turned `angle` degrees
/// (counter-clockwise from east) and, `mirrored`, flipped across its own x axis; `params`, its
/// instance parameters' values where they aren't the family's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub family: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Guid,
    pub at: P2,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// A stair from `level` up to `top_level` (or `height` mm), climbing from the middle of its first
/// riser at `start` toward `angle` degrees (counter-clockwise from east): as many risers as keep
/// each no higher than `riser`, a tread `tread` deep between each two; in one straight run, or in
/// runs with a landing between each two.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stair {
    pub level: Guid,
    #[serde(default)]
    pub top_level: Option<Guid>,
    #[serde(default)]
    pub height: f64,
    pub start: P2,
    pub angle: f64,
    pub width: f64,
    pub riser: f64,
    pub tread: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub landings: Vec<Landing>,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// A landing between two runs of a stair or a ramp: after `at` risers of a stair, or `at` mm up a
/// ramp; then the next run goes on straight (`turn` 0), turned left (90) or right (-90), or back
/// beside the last, to its left (180) or its right (-180). It's as deep as the runs are wide.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Landing {
    pub at: f64,
    #[serde(default)]
    pub turn: f64,
}

/// A ramp from `level` up to `top_level` (or `height` mm), from the middle of its foot at `start`
/// toward `angle` degrees: rising at its slope, `slope` percent (rise over run), in one run or in
/// runs with a landing between each two. Solid down to its level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ramp {
    pub level: Guid,
    #[serde(default)]
    pub top_level: Option<Guid>,
    #[serde(default)]
    pub height: f64,
    pub start: P2,
    pub angle: f64,
    pub width: f64,
    pub slope: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub landings: Vec<Landing>,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// A side of a stair or a ramp, going up it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Left,
    Right,
}

/// A railing: along a side of a stair or a ramp, `host`, climbing with it; or along `path` on its
/// level, `base_offset` mm above it. Its rail is its type's height over what it runs along.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Railing {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Guid,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<P2>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub base_offset: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// A roof on `level`, its base `offset` mm above it. By its footprint: its eaves along `boundary`
/// and round its `courtyards` (holes in it), reaching its base `overhang` mm in from them; flat at
/// `slope` 0, else each edge sloping up at `slope` degrees, hips and valleys where slopes meet, but
/// the edges in `gables` (edge i from point i to the next, the courtyards' numbered on after the
/// boundary's), left upright; `gable` leaves a rectangle's two short ends upright. Or by its
/// `extrusion`. Holed through by `openings` in plan, and where it runs into the roof it `join`s,
/// it ends at that one's top.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roof {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Guid,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boundary: Vec<P2>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub courtyards: Vec<Vec<P2>>,
    #[serde(default)]
    pub slope: f64,
    #[serde(default)]
    pub overhang: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gables: Vec<usize>,
    #[serde(default)]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Vec<P2>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrusion: Option<Extrusion>,
    /// The sketch it was drawn or last edited by: its lines, each a part with an id and a GUID of its own. Empty when it
    /// was made another way (over the walls, from a file); Edit Boundary then starts from its outline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch: Vec<crate::sketch::SketchLine>,
    /// Its values of the project's parameters, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

impl Element {
    /// The sketch it keeps, when it's of a kind drawn by one: a floor's, a ceiling's, a roof's, a shaft's.
    pub fn sketch(&self) -> Option<&Vec<crate::sketch::SketchLine>> {
        match self {
            Element::Floor(f) => Some(&f.sketch),
            Element::Ceiling(c) => Some(&c.sketch),
            Element::Roof(r) => Some(&r.sketch),
            Element::Shaft(sh) => Some(&sh.sketch),
            Element::FilledRegion(r) => Some(&r.sketch),
            _ => None,
        }
    }

    pub fn sketch_mut(&mut self) -> Option<&mut Vec<crate::sketch::SketchLine>> {
        match self {
            Element::Floor(f) => Some(&mut f.sketch),
            Element::Ceiling(c) => Some(&mut c.sketch),
            Element::Roof(r) => Some(&mut r.sketch),
            Element::Shaft(sh) => Some(&mut sh.sketch),
            Element::FilledRegion(r) => Some(&mut r.sketch),
            _ => None,
        }
    }
}

impl Roof {
    /// Its footprint's edges as `gables` numbers them: its boundary's, then each courtyard's.
    pub fn edges(&self) -> impl Iterator<Item = (P2, P2)> + '_ {
        geom::ring_edges(&self.boundary).chain(self.courtyards.iter().flat_map(|c| geom::ring_edges(c)))
    }
}

/// An extrusion roof's shape: its underside's profile in the upright plane through `line`, points
/// `[along, up]` mm along it from its start and up from the roof's base, swept square to it from
/// `from` to `to` mm to its left.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extrusion {
    pub line: [P2; 2],
    pub profile: Vec<P2>,
    #[serde(default)]
    pub from: f64,
    pub to: f64,
}

/// The most points a pitched roof's footprint has: where its slopes meet takes time to find.
pub const MAX_ROOF_POINTS: usize = 256;

/// One of a roof's slopes: the loops round it in plan (its outline counter-clockwise, then any
/// holes in it, clockwise), and the plane its underside lies in.
pub type Slope = (Vec<Vec<P2>>, walls::Plane);

/// The way `angle` degrees (counter-clockwise from east) points.
pub fn heading(angle: f64) -> P2 {
    let a = angle.to_radians();
    P2(a.cos(), a.sin())
}

/// A section or an elevation: a vertical view along the line from `start` to `end`, looking to
/// its left. A section cuts the model at its line; an elevation's line lies outside the building,
/// looking at it. It sees `depth` mm past its line, or everything without one; cropped, only from
/// the height `crop[0]` up to `crop[1]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub name: String,
    pub start: P2,
    pub end: P2,
    #[serde(default)]
    pub depth: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<[f64; 2]>,
    /// What it hides and how it draws the rest.
    #[serde(default, skip_serializing_if = "Graphics::is_default")]
    pub graphics: Graphics,
}

impl Section {
    /// The way it looks: square to its line, to the left.
    pub fn looking(&self) -> P2 {
        (self.end - self.start).unit().perp()
    }
}

/// The side of the building an elevation shows: North shows the north side, looking south.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Facing {
    North,
    South,
    East,
    West,
}

impl Facing {
    pub const ALL: [Facing; 4] = [Facing::North, Facing::South, Facing::East, Facing::West];

    pub fn name(self) -> &'static str {
        match self {
            Facing::North => "North",
            Facing::South => "South",
            Facing::East => "East",
            Facing::West => "West",
        }
    }

    /// A side by its name, in any language there is.
    pub fn parse(s: &str) -> Option<Facing> {
        Facing::ALL.into_iter().find(|f| Language::ALL.iter().any(|l| l.find(s, &[f.name()]).is_some()))
    }

    /// Out of the building, toward its side.
    fn out(self) -> P2 {
        match self {
            Facing::North => P2(0.0, 1.0),
            Facing::South => P2(0.0, -1.0),
            Facing::East => P2(1.0, 0.0),
            Facing::West => P2(-1.0, 0.0),
        }
    }
}

/// A sheet: paper of an ISO size (A0 to A4, landscape), with a title block, and the views placed
/// on it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sheet {
    pub number: String,
    pub name: String,
    pub size: String,
}

/// The paper an ISO size names, landscape: width and height in mm.
pub fn paper(size: &str) -> Option<P2> {
    match size.trim().to_ascii_uppercase().as_str() {
        "A0" => Some(P2(1189.0, 841.0)),
        "A1" => Some(P2(841.0, 594.0)),
        "A2" => Some(P2(594.0, 420.0)),
        "A3" => Some(P2(420.0, 297.0)),
        "A4" => Some(P2(297.0, 210.0)),
        _ => None,
    }
}

/// A view on a sheet: a level's plan, a section, an elevation, a schedule or a 3D view, its middle
/// `at` mm from the paper's bottom left corner, at 1 : `scale` (a schedule keeps its own size).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub sheet: Guid,
    pub view: Guid,
    pub at: P2,
    pub scale: f64,
}

/// A pattern for the element of any run category, its run bound as `$r`: a tab adds its categories here, and every place
/// that treats runs alike takes them.
macro_rules! run_arm {
    ($r:pat) => {
        Element::Beam($r)
            | Element::StripFooting($r)
            | Element::Pipe($r)
            | Element::Conduit($r)
            | Element::CableTray($r)
            | Element::Wire($r)
            | Element::DownConductor($r)
            | Element::GroundConductor($r)
            | Element::Cable($r)
    };
}
pub(crate) use run_arm;

/// The references of `$e` (an element, by `&` or `&mut` as `$($m)?` says), each given to `$f`.
macro_rules! visit_refs {
    ($e:expr, $f:expr, $($m:tt)?) => {{
        let f: &mut dyn FnMut(& $($m)? Guid) = $f;
        match $e {
            // A view's graphics, as `views_graphics` gives them.
            Element::Level(_) | Element::Section(_) | Element::Elevation(_) | Element::View3D(_) => {}
            Element::Grid(_) | Element::Sheet(_) | Element::Schedule(_) | Element::Link(_) => {}
            Element::PlanView(v) => {
                f(& $($m)? v.level);
                if let Some(g) = & $($m)? v.parent { f(g) }
            }
            Element::Wall(w) => {
                f(& $($m)? w.level);
                if let Some(g) = & $($m)? w.top_level { f(g) }
                if let Some(g) = & $($m)? w.attach_top { f(g) }
                if let Some(g) = & $($m)? w.attach_base { f(g) }
            }
            Element::Floor(Floor { level, .. }) | Element::Ceiling(Ceiling { level, .. }) | Element::Room(Room { level, .. }) => f(level),
            Element::Shaft(Shaft { level, top_level, .. })
            | Element::Mass(Mass { level, top_level, .. })
            | Element::Column(Column { level, top_level, .. })
            | Element::StructuralColumn(Column { level, top_level, .. })
            | Element::Footing(Column { level, top_level, .. })
            | Element::Stair(Stair { level, top_level, .. })
            | Element::Ramp(Ramp { level, top_level, .. }) => {
                f(level);
                if let Some(g) = top_level { f(g) }
            }
            Element::Door(o) | Element::Window(o) => f(& $($m)? o.host),
            Element::Dimension(d) => {
                f(& $($m)? d.level);
                if let Some(g) = & $($m)? d.view { f(g) }
                for r in & $($m)? d.refs {
                    f(& $($m)? r.id);
                }
            }
            Element::Tag(t) => {
                f(& $($m)? t.host);
                if let Some(g) = & $($m)? t.view { f(g) }
            }
            Element::Viewport(v) => {
                f(& $($m)? v.sheet);
                f(& $($m)? v.view);
            }
            run_arm!(r) => f(& $($m)? r.level),
            Element::Component(c) => f(& $($m)? c.level),
            Element::Railing(r) => {
                f(& $($m)? r.level);
                if let Some(g) = & $($m)? r.host { f(g) }
            }
            Element::Roof(r) => {
                f(& $($m)? r.level);
                if let Some(g) = & $($m)? r.join { f(g) }
            }
            Element::Text(n) => f(& $($m)? n.view),
            Element::DetailLine(l) => f(& $($m)? l.view),
            Element::FilledRegion(r) => f(& $($m)? r.view),
            Element::DetailGroup(g) => f(& $($m)? g.view),
            Element::RepeatingDetail(r) => f(& $($m)? r.view),
            Element::Spot(s) => {
                f(& $($m)? s.view);
                if let Some(g) = & $($m)? s.host { f(g) }
            }
            Element::PointCloud(pc) => f(& $($m)? pc.level),
        }
    }};
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "category", rename_all = "lowercase")]
pub enum Element {
    Level(Level),
    Grid(Grid),
    Wall(Wall),
    Floor(Floor),
    Ceiling(Ceiling),
    Shaft(Shaft),
    Mass(Mass),
    Door(Opening),
    Window(Opening),
    Room(Room),
    Dimension(Dimension),
    Tag(Tag),
    Section(Section),
    Elevation(Section),
    Sheet(Sheet),
    Viewport(Viewport),
    Column(Column),
    /// A column of the structure: the same shape as an architectural one, its own category and types.
    StructuralColumn(Column),
    /// An isolated (pad) footing: a column laid flat, its `height` its thickness.
    Footing(Column),
    Beam(Run),
    StripFooting(Run),
    Pipe(Run),
    Conduit(Run),
    CableTray(Run),
    Wire(Run),
    DownConductor(Run),
    GroundConductor(Run),
    Cable(Run),
    Component(Component),
    Stair(Stair),
    Ramp(Ramp),
    Railing(Railing),
    Roof(Roof),
    Text(Note),
    /// A detail line: drawn in one view, in a line style; of the Lines category, as Revit's.
    #[serde(rename = "line")]
    DetailLine(DetailLine),
    /// A filled region: a sketch's outlines in one view, filled as its type says; of Revit's Filled Region.
    #[serde(rename = "region")]
    FilledRegion(FilledRegion),
    /// A detail group placed in one view: its type's lines, regions and notes, moved and turned; of Revit's Detail Groups.
    #[serde(rename = "detailgroup")]
    DetailGroup(DetailGroup),
    /// A detail group repeated along a line in one view; of Revit's Repeating Detail.
    #[serde(rename = "repeatingdetail")]
    RepeatingDetail(RepeatingDetail),
    Spot(Spot),
    Schedule(Schedule),
    View3D(View3D),
    PlanView(PlanView),
    Link(Link),
    /// A scan placed in the model.
    PointCloud(PointCloud),
}

impl Element {
    /// Its own values of parameters: the project's, and a component's family's.
    pub fn params(&self) -> Option<&BTreeMap<String, serde_json::Value>> {
        match self {
            Element::Wall(Wall { params, .. })
            | Element::Floor(Floor { params, .. })
            | Element::Ceiling(Ceiling { params, .. })
            | Element::Roof(Roof { params, .. })
            | Element::Mass(Mass { params, .. })
            | Element::Door(Opening { params, .. })
            | Element::Window(Opening { params, .. })
            | Element::Room(Room { params, .. })
            | Element::Column(Column { params, .. })
            | Element::StructuralColumn(Column { params, .. })
            | Element::Footing(Column { params, .. })
            | run_arm!(Run { params, .. })
            | Element::Component(Component { params, .. })
            | Element::Stair(Stair { params, .. })
            | Element::Ramp(Ramp { params, .. })
            | Element::Railing(Railing { params, .. }) => Some(params),
            _ => None,
        }
    }

    /// Its own values of parameters, to change.
    pub fn params_mut(&mut self) -> Option<&mut BTreeMap<String, serde_json::Value>> {
        match self {
            Element::Wall(Wall { params, .. })
            | Element::Floor(Floor { params, .. })
            | Element::Ceiling(Ceiling { params, .. })
            | Element::Roof(Roof { params, .. })
            | Element::Mass(Mass { params, .. })
            | Element::Door(Opening { params, .. })
            | Element::Window(Opening { params, .. })
            | Element::Room(Room { params, .. })
            | Element::Column(Column { params, .. })
            | Element::StructuralColumn(Column { params, .. })
            | Element::Footing(Column { params, .. })
            | run_arm!(Run { params, .. })
            | Element::Component(Component { params, .. })
            | Element::Stair(Stair { params, .. })
            | Element::Ramp(Ramp { params, .. })
            | Element::Railing(Railing { params, .. }) => Some(params),
            _ => None,
        }
    }

    /// Every element it points at, by GUID, each given to `f`, which may put another in its place: its levels, its
    /// host, what it measures or is attached to, its view, its sheet, the elements its graphics override, and the mass a
    /// mark in its `params` names.
    pub fn refs_mut(&mut self, f: &mut dyn FnMut(&mut Guid)) {
        visit_refs!(self, f, mut);
        if let Some(params) = self.params_mut() {
            crate::massing::refs_mut(params, f);
        }
        if let Some(g) = graphics::graphics_mut(self) {
            // Its keys: taken out, each given to `f`, and put back.
            g.elements = std::mem::take(&mut g.elements)
                .into_iter()
                .map(|(mut k, v)| {
                    f(&mut k);
                    (k, v)
                })
                .collect();
        }
    }

    /// Every element it points at, by GUID, as [`Element::refs_mut`] gives them.
    pub fn refs(&self) -> Vec<Guid> {
        let mut out = Vec::new();
        let mut f = |g: &Guid| out.push(*g);
        visit_refs!(self, &mut f,);
        if let Some(params) = self.params() {
            out.extend(crate::massing::refs(params));
        }
        if let Some(g) = graphics::graphics_of_element(self) {
            out.extend(g.elements.keys());
        }
        out
    }

    /// The family a component or a family's door or window is of, and its type.
    pub fn family(&self) -> Option<(&str, &str)> {
        match self {
            Element::Component(c) => Some((&c.family, &c.type_name)),
            Element::Door(o) | Element::Window(o) => o.family.as_deref().map(|f| (f, o.type_name.as_str())),
            _ => None,
        }
    }

    /// The run a beam or a strip footing is, whatever its category.
    pub fn run(&self) -> Option<&Run> {
        match self {
            run_arm!(r) => Some(r),
            _ => None,
        }
    }

    /// A run of `category` as an element: a beam, a strip footing. None for a category that has no runs.
    pub fn of_run(category: &str, r: Run) -> Option<Element> {
        match category {
            "beam" => Some(Element::Beam(r)),
            "stripfooting" => Some(Element::StripFooting(r)),
            "pipe" => Some(Element::Pipe(r)),
            "conduit" => Some(Element::Conduit(r)),
            "cabletray" => Some(Element::CableTray(r)),
            "wire" => Some(Element::Wire(r)),
            "downconductor" => Some(Element::DownConductor(r)),
            "groundconductor" => Some(Element::GroundConductor(r)),
            "cable" => Some(Element::Cable(r)),
            _ => None,
        }
    }

    /// The run to change.
    pub fn run_mut(&mut self) -> Option<&mut Run> {
        match self {
            run_arm!(r) => Some(r),
            _ => None,
        }
    }

    /// The column an architectural column, a structural one or an isolated footing is, whatever its category.
    pub fn column(&self) -> Option<&Column> {
        match self {
            Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => Some(c),
            _ => None,
        }
    }

    /// The column to change.
    pub fn column_mut(&mut self) -> Option<&mut Column> {
        match self {
            Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => Some(c),
            _ => None,
        }
    }

    /// The kind of family a door or a window is of.
    pub fn hosted_category(&self) -> Option<crate::family::Category> {
        match self {
            Element::Door(_) => Some(crate::family::Category::Door),
            Element::Window(_) => Some(crate::family::Category::Window),
            _ => None,
        }
    }

    /// The name of its family, to change.
    pub fn family_mut(&mut self) -> Option<&mut String> {
        match self {
            Element::Component(c) => Some(&mut c.family),
            Element::Door(o) | Element::Window(o) => o.family.as_mut(),
            _ => None,
        }
    }

    pub fn category(&self) -> &'static str {
        match self {
            Element::Text(_) => "text",
            Element::DetailLine(_) => "line",
            Element::FilledRegion(_) => "region",
            Element::DetailGroup(_) => "detailgroup",
            Element::RepeatingDetail(_) => "repeatingdetail",
            Element::Spot(_) => "spot",
            Element::Link(_) => "link",
            Element::PointCloud(_) => "pointcloud",
            Element::Schedule(_) => "schedule",
            Element::View3D(_) => "view3d",
            Element::PlanView(_) => "planview",
            Element::Level(_) => "level",
            Element::Grid(_) => "grid",
            Element::Wall(_) => "wall",
            Element::Floor(_) => "floor",
            Element::Ceiling(_) => "ceiling",
            Element::Shaft(_) => "shaft",
            Element::Mass(_) => "mass",
            Element::Door(_) => "door",
            Element::Window(_) => "window",
            Element::Room(_) => "room",
            Element::Dimension(_) => "dimension",
            Element::Tag(_) => "tag",
            Element::Section(_) => "section",
            Element::Elevation(_) => "elevation",
            Element::Sheet(_) => "sheet",
            Element::Viewport(_) => "viewport",
            Element::Column(_) => "column",
            Element::StructuralColumn(_) => "structuralcolumn",
            Element::Footing(_) => "footing",
            Element::Beam(_) => "beam",
            Element::StripFooting(_) => "stripfooting",
            Element::Pipe(_) => "pipe",
            Element::Conduit(_) => "conduit",
            Element::CableTray(_) => "cabletray",
            Element::Wire(_) => "wire",
            Element::DownConductor(_) => "downconductor",
            Element::GroundConductor(_) => "groundconductor",
            Element::Cable(_) => "cable",
            Element::Component(_) => "component",
            Element::Stair(_) => "stair",
            Element::Ramp(_) => "ramp",
            Element::Railing(_) => "railing",
            Element::Roof(_) => "roof",
        }
    }
}

/// A wall type: how thick its walls are and, a compound one, its layers from the exterior side in,
/// which make up that thickness. Its core runs from its first structure layer to its last (all of
/// it, without one).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallType {
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
}

/// A floor type: how thick its floors are and, a compound one, its layers from the top down.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloorType {
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
}

/// A railing type: how high its rail stands, and how far apart its balusters are (0: none).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RailingType {
    pub height: f64,
    #[serde(default)]
    pub spacing: f64,
}

/// A ceiling type: how thick, and with `grid`, square tiles that wide, which its reflected ceiling
/// plan draws; a compound one, its layers from the top down.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CeilingType {
    pub thickness: f64,
    #[serde(default)]
    pub grid: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
}

/// A column type: `width` by `depth`, or round, `width` across.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnType {
    pub width: f64,
    pub depth: f64,
    #[serde(default)]
    pub round: bool,
}

/// A run's type: a section `width` across by `depth` deep, or round, `width` across; made of `material` (one of the
/// library's, or none).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunType {
    pub width: f64,
    pub depth: f64,
    #[serde(default)]
    pub round: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
}

/// The categories whose elements are runs, as `Element::run` returns them.
pub const RUN_CATEGORIES: [&str; 9] =
    ["beam", "stripfooting", "pipe", "conduit", "cabletray", "wire", "downconductor", "groundconductor", "cable"];

/// A roof type: how thick its roofs are across their slope and, a compound one, its layers from
/// the top down.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoofType {
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
}

/// A door or window type. `sill` is a window's default sill height.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningType {
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub sill: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Types {
    pub wall: BTreeMap<String, WallType>,
    pub floor: BTreeMap<String, FloorType>,
    pub door: BTreeMap<String, OpeningType>,
    pub window: BTreeMap<String, OpeningType>,
    #[serde(default)]
    pub column: BTreeMap<String, ColumnType>,
    #[serde(default)]
    pub roof: BTreeMap<String, RoofType>,
    #[serde(default)]
    pub ceiling: BTreeMap<String, CeilingType>,
    #[serde(default)]
    pub railing: BTreeMap<String, RailingType>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub structural_column: BTreeMap<String, ColumnType>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub footing: BTreeMap<String, ColumnType>,
    /// The types of the runs, by category, then name: `runs["beam"]["Beam 200 x 500"]`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub runs: BTreeMap<String, BTreeMap<String, RunType>>,
}

/// What a project parameter holds: text, a number, a length (mm), or yes or no.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterKind {
    #[default]
    Text,
    Number,
    Length,
    YesNo,
}

/// A project parameter, as Revit's Manage > Project Parameters makes one: a value of its kind each
/// element of its categories may have of its own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectParameter {
    #[serde(default)]
    pub kind: ParameterKind,
    pub categories: Vec<String>,
}

/// The most project parameters a project holds.
pub const MAX_PROJECT_PARAMETERS: usize = 256;

/// The categories project parameters are for.
pub const PARAMETER_CATEGORIES: [&str; 24] = [
    "wall",
    "floor",
    "ceiling",
    "roof",
    "door",
    "window",
    "room",
    "column",
    "component",
    "stair",
    "ramp",
    "railing",
    "mass",
    "structuralcolumn",
    "footing",
    "beam",
    "stripfooting",
    "pipe",
    "conduit",
    "cabletray",
    "wire",
    "downconductor",
    "groundconductor",
    "cable",
];

impl ProjectParameter {
    /// Whether `v` is a value it holds.
    pub fn holds(&self, v: &serde_json::Value) -> bool {
        match self.kind {
            ParameterKind::Text => v.as_str().is_some_and(|t| t.chars().count() <= MAX_TEXT),
            ParameterKind::Number | ParameterKind::Length => v.as_f64().is_some_and(|x| x.is_finite() && x.abs() <= MAX_COORD),
            ParameterKind::YesNo => v.is_boolean(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    /// Which version of the file this is: [`FORMAT`] when written; none, a file from before GUIDs.
    #[serde(default)]
    pub format: u32,
    pub name: String,
    /// Seeds the IFC GlobalIds, so every export of this model keeps them.
    pub uid: u64,
    /// How lengths and areas are shown and typed; the model itself is in millimetres.
    #[serde(default)]
    pub units: Units,
    /// What its drawings say, and the names commands give.
    #[serde(default)]
    pub language: Language,
    pub next_id: Id,
    pub types: Types,
    /// What layers are made of, by name.
    #[serde(default)]
    pub materials: BTreeMap<String, Material>,
    /// The pictures its materials' textures are made of, by id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub images: BTreeMap<String, Picture>,
    /// The scans its point clouds place, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "crate::cloud::deserialize_scans")]
    pub clouds: BTreeMap<String, Cloud>,
    /// Its families, by name: what components place.
    #[serde(default)]
    pub families: BTreeMap<String, Family>,
    /// Its project parameters, by name: values each element of their categories may have.
    #[serde(default)]
    pub parameters: BTreeMap<String, ProjectParameter>,
    /// Its view filters, by name: categories and rules that pick elements out for views to hide
    /// or draw another way.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub view_filters: BTreeMap<String, ViewFilter>,
    /// Its view templates, by name: settings views follow.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub view_templates: BTreeMap<String, ViewTemplate>,
    /// Its phases, in order: what an element is made in, and may be taken down in, a later one.
    #[serde(default = "phasing::default_phases", skip_serializing_if = "phasing::is_default_phases")]
    pub phases: Vec<String>,
    /// How a view's phase filter shows each status of what's in its phase, by name.
    #[serde(default = "phasing::default_filters", skip_serializing_if = "phasing::is_default_filters")]
    pub phase_filters: BTreeMap<String, PhaseFilter>,
    /// What each status is drawn as where a phase filter says overridden.
    #[serde(default = "phasing::default_overrides", skip_serializing_if = "phasing::is_default_overrides")]
    pub phase_overrides: BTreeMap<Status, graphics::Override>,
    /// Its sets of design options, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub option_sets: BTreeMap<String, OptionSet>,
    /// What its pens draw on paper (Manage > Line Weights), once set; until then, what a project in its units starts with
    /// ([`Model::line_weights`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_weights: Option<crate::pen::LineWeights>,
    /// The pens each model category draws in, cut and in projection (Manage > Object Styles), by category: only those it
    /// has set apart from what a project starts with ([`Model::object_style`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub object_styles: BTreeMap<String, crate::styles::ObjectStyle>,
    /// Its line patterns, by name: dashes, dots and spaces, mm on paper.
    #[serde(default = "lines::default_patterns")]
    pub line_patterns: BTreeMap<String, LinePattern>,
    /// Its line styles, by name: a pen, a colour and a pattern, what detail lines draw in.
    #[serde(default = "lines::default_styles")]
    pub line_styles: BTreeMap<String, LineStyle>,
    /// Its fill patterns, by name: families of lines, drafting (mm on paper) or model (mm in the building); none, solid.
    #[serde(default = "fills::default_patterns")]
    pub fill_patterns: BTreeMap<String, FillPattern>,
    /// Its filled region types, by name: a foreground and a background pattern and their colours, masking or not.
    #[serde(default = "fills::default_region_types")]
    pub region_types: BTreeMap<String, FilledRegionType>,
    /// Its detail group types, by name: the members each one placed draws.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub detail_groups: BTreeMap<String, crate::groups::GroupType>,
    pub elements: Elements,
    /// The ids and GUIDs of the library: its types, materials, pictures, families, parameters, view filters and
    /// templates, phases, phase filters and design options.
    #[serde(default)]
    pub library: crate::library::Library,
    /// What the links read, by their `path`: not in a file, which holds the path alone. A link with no entry here is
    /// not loaded, and draws nothing.
    #[serde(skip)]
    pub loaded: BTreeMap<String, Arc<Model>>,
}

impl Model {
    /// What its pens draw on paper: its own line weights, or those a project in its units starts with.
    pub fn line_weights(&self) -> std::borrow::Cow<'_, crate::pen::LineWeights> {
        match &self.line_weights {
            Some(lw) => std::borrow::Cow::Borrowed(lw),
            None => std::borrow::Cow::Owned(crate::pen::LineWeights::default_for(self.units)),
        }
    }

    /// An empty project with two levels and a few types to start from.
    pub fn new(name: &str) -> Model {
        Model::new_in(name, Language::English)
    }

    /// An empty project in `language`: its two levels named in it.
    pub fn new_in(name: &str, language: Language) -> Model {
        let mut m = Model {
            format: FORMAT,
            name: name.into(),
            uid: fresh_uid(),
            units: Units::default(),
            language,
            next_id: FIRST_ID,
            types: Types::default(),
            materials: BTreeMap::new(),
            images: BTreeMap::new(),
            clouds: BTreeMap::new(),
            families: BTreeMap::new(),
            parameters: BTreeMap::new(),
            view_filters: BTreeMap::new(),
            view_templates: BTreeMap::new(),
            phases: phasing::phases(language),
            phase_filters: phasing::filters(language),
            phase_overrides: phasing::overrides(),
            option_sets: BTreeMap::new(),
            line_weights: None,
            object_styles: BTreeMap::new(),
            line_patterns: lines::patterns(language),
            line_styles: lines::styles(language),
            fill_patterns: fills::patterns(language),
            region_types: fills::region_types(language),
            detail_groups: BTreeMap::new(),
            elements: Elements::new(),
            library: crate::library::Library::default(),
            loaded: BTreeMap::new(),
        };
        // A library to start from: each material its colour, its cut's pattern and its surface's, and the texture it has.
        for (name, color, cut, surface, texture) in [
            ("Brick", [178, 92, 70], Pattern::Brick, Pattern::Brick, Some(Kind::Brick)),
            ("Insulation", [232, 214, 140], Pattern::Insulation, Pattern::None, Some(Kind::Insulation)),
            ("Concrete block", [176, 176, 170], Pattern::Crosshatch, Pattern::None, Some(Kind::Block)),
            ("Plaster", [238, 234, 224], Pattern::None, Pattern::None, Some(Kind::Plaster)),
            ("Concrete", [188, 188, 182], Pattern::Concrete, Pattern::None, Some(Kind::Concrete)),
            ("Screed", [206, 200, 188], Pattern::Concrete, Pattern::None, Some(Kind::Concrete)),
            ("Timber", [196, 154, 102], Pattern::Diagonal, Pattern::Horizontal, Some(Kind::Boards)),
            ("Gypsum board", [242, 242, 238], Pattern::None, Pattern::None, None),
            ("Ceramic tile", [222, 220, 210], Pattern::Solid, Pattern::Grid, Some(Kind::Tile)),
            ("Roof tiles", [170, 86, 64], Pattern::Solid, Pattern::Horizontal, Some(Kind::Roof)),
            ("Membrane", [70, 70, 74], Pattern::Solid, Pattern::None, None),
            ("Fabric", [176, 160, 136], Pattern::None, Pattern::None, Some(Kind::Fabric)),
            ("Porcelain", [244, 244, 240], Pattern::None, Pattern::None, None),
            ("Glass", [166, 200, 228], Pattern::None, Pattern::None, None),
            ("Metal", [150, 152, 158], Pattern::Solid, Pattern::None, Some(Kind::Metal)),
            ("PVC", [226, 226, 214], Pattern::Solid, Pattern::None, None),
            ("Copper", [184, 115, 51], Pattern::Solid, Pattern::None, Some(Kind::Metal)),
        ] {
            let texture = texture.map(Texture::new);
            m.materials.insert(language.word(name).into(), Material { color, cut, surface, texture, ..Material::default() });
        }
        m.families = crate::family::library(language);
        for (n, t) in [("Generic - 200mm", 200.0), ("Interior - 100mm", 100.0)] {
            m.types.wall.insert(n.into(), WallType { thickness: t, layers: Vec::new() });
        }
        // An exterior wall's layers, outside in: brick, insulation, a block core, plaster.
        let layer = |function, material: &str, thickness| Layer {
            function,
            material: language.word(material).into(),
            thickness,
            ..Layer::default()
        };
        let layers = vec![
            layer(Function::Finish, "Brick", 100.0),
            layer(Function::Thermal, "Insulation", 50.0),
            layer(Function::Structure, "Concrete block", 140.0),
            layer(Function::Finish, "Plaster", 10.0),
        ];
        m.types.wall.insert("Exterior - 300mm".into(), WallType { thickness: 300.0, layers });
        m.types.floor.insert("Generic - 200mm".into(), FloorType { thickness: 200.0, layers: Vec::new() });
        for (n, w) in [("Single 900 x 2100", 900.0), ("Single 800 x 2100", 800.0)] {
            m.types.door.insert(n.into(), OpeningType { width: w, height: 2100.0, sill: 0.0 });
        }
        for (n, w, h, s) in [("Fixed 1200 x 1500", 1200.0, 1500.0, 900.0), ("Fixed 600 x 900", 600.0, 900.0, 1100.0)] {
            m.types.window.insert(n.into(), OpeningType { width: w, height: h, sill: s });
        }
        m.types.column.insert("Rectangular 400 x 400".into(), ColumnType { width: 400.0, depth: 400.0, round: false });
        m.types.column.insert("Round 300".into(), ColumnType { width: 300.0, depth: 300.0, round: true });
        // The structure's: concrete columns, footings, beams and strip footings.
        for (n, w, d, round) in [
            ("Concrete 300 x 300", 300.0, 300.0, false),
            ("Concrete 200 x 500", 200.0, 500.0, false),
            ("Concrete Round 400", 400.0, 400.0, true),
        ] {
            m.types.structural_column.insert(n.into(), ColumnType { width: w, depth: d, round });
        }
        for (n, w) in [("Footing 1200 x 1200", 1200.0), ("Footing 1500 x 1500", 1500.0)] {
            m.types.footing.insert(n.into(), ColumnType { width: w, depth: w, round: false });
        }
        let run = |width, depth, round, material: &str| RunType { width, depth, round, material: language.word(material).into() };
        let beams = [
            ("Beam 200 x 500", run(200.0, 500.0, false, "Concrete")),
            ("Beam 150 x 400", run(150.0, 400.0, false, "Concrete")),
            ("Steel Tube 150", run(150.0, 150.0, true, "Metal")),
        ];
        m.types.runs.insert("beam".into(), beams.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        m.types.runs.insert("stripfooting".into(), BTreeMap::from([("Strip 600 x 300".to_string(), run(600.0, 300.0, false, "Concrete"))]));
        // The plumbing's: PVC for cold water and drains (outside diameters, mm) and copper for hot water.
        let pipes = [
            ("PVC 25", run(25.0, 25.0, true, "PVC")),
            ("PVC 50", run(50.0, 50.0, true, "PVC")),
            ("PVC 110", run(110.0, 110.0, true, "PVC")),
            ("Copper 22", run(22.0, 22.0, true, "Copper")),
        ];
        m.types.runs.insert("pipe".into(), pipes.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        // The electrical's: PVC and steel conduits (eletrodutos), perforated trays (eletrocalhas) and wires, by their outside size.
        let conduits = [
            ("PVC 20", run(20.0, 20.0, true, "PVC")),
            ("PVC 25", run(25.0, 25.0, true, "PVC")),
            ("PVC 32", run(32.0, 32.0, true, "PVC")),
            ("Steel 25", run(25.0, 25.0, true, "Metal")),
        ];
        m.types.runs.insert("conduit".into(), conduits.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        let trays = [
            ("Perforated 100 x 50", run(100.0, 50.0, false, "Metal")),
            ("Perforated 200 x 50", run(200.0, 50.0, false, "Metal")),
            ("Perforated 300 x 100", run(300.0, 100.0, false, "Metal")),
        ];
        m.types.runs.insert("cabletray".into(), trays.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        let wires = [
            ("Wire 2.5", run(4.0, 4.0, true, "Copper")),
            ("Wire 4", run(5.0, 5.0, true, "Copper")),
            ("Wire 6", run(6.5, 6.5, true, "Copper")),
        ];
        // Lightning protection (NBR 5419): down conductors in steel, copper or flat steel strip, earthing conductors in bare copper.
        let down = [
            ("Steel 8", run(8.0, 8.0, true, "Metal")),
            ("Copper 8", run(8.0, 8.0, true, "Copper")),
            ("Flat 25 x 3", run(25.0, 3.0, false, "Metal")),
        ];
        m.types.runs.insert("downconductor".into(), down.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        let ground = [("Copper 9", run(9.0, 9.0, true, "Copper")), ("Steel 8", run(8.0, 8.0, true, "Metal"))];
        m.types.runs.insert("groundconductor".into(), ground.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        // Data and voice cabling (NBR 14565): twisted pair by category, and an optical cable.
        let cables =
            [("Cat6", run(6.0, 6.0, true, "PVC")), ("Cat6A", run(7.5, 7.5, true, "PVC")), ("Optical 3", run(3.0, 3.0, true, "PVC"))];
        m.types.runs.insert("cable".into(), cables.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        m.types.runs.insert("wire".into(), wires.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        // The electrical's: PVC and steel conduits (eletrodutos), perforated trays (eletrocalhas) and wires, by their outside size.
        let conduits = [
            ("PVC 20", run(20.0, 20.0, true, "PVC")),
            ("PVC 25", run(25.0, 25.0, true, "PVC")),
            ("PVC 32", run(32.0, 32.0, true, "PVC")),
            ("Steel 25", run(25.0, 25.0, true, "Metal")),
        ];
        m.types.runs.insert("conduit".into(), conduits.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        let trays = [
            ("Perforated 100 x 50", run(100.0, 50.0, false, "Metal")),
            ("Perforated 200 x 50", run(200.0, 50.0, false, "Metal")),
            ("Perforated 300 x 100", run(300.0, 100.0, false, "Metal")),
        ];
        m.types.runs.insert("cabletray".into(), trays.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        let wires = [
            ("Wire 2.5", run(4.0, 4.0, true, "Copper")),
            ("Wire 4", run(5.0, 5.0, true, "Copper")),
            ("Wire 6", run(6.5, 6.5, true, "Copper")),
        ];
        m.types.runs.insert("wire".into(), wires.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        m.types.roof.insert("Generic - 300mm".into(), RoofType { thickness: 300.0, layers: Vec::new() });
        m.types.ceiling.insert("Generic - 20mm".into(), CeilingType { thickness: 20.0, grid: 0.0, layers: Vec::new() });
        m.types.ceiling.insert("Tile 600 x 600".into(), CeilingType { thickness: 20.0, grid: 600.0, layers: Vec::new() });
        m.types.railing.insert("Railing - 1000mm".into(), RailingType { height: 1000.0, spacing: 120.0 });
        let level = language.word("Level");
        m.insert(Element::Level(Level::new(format!("{level} 1"), 0.0)));
        m.insert(Element::Level(Level::new(format!("{level} 2"), 3000.0)));
        m.sync_library();
        m
    }

    /// Every type that may have layers: its category, name, thickness and layers.
    pub fn layered_types(&self) -> impl Iterator<Item = (&'static str, &str, f64, &[Layer])> + '_ {
        let t = &self.types;
        let walls = t.wall.iter().map(|(n, x)| ("wall", n.as_str(), x.thickness, x.layers.as_slice()));
        let floors = t.floor.iter().map(|(n, x)| ("floor", n.as_str(), x.thickness, x.layers.as_slice()));
        let roofs = t.roof.iter().map(|(n, x)| ("roof", n.as_str(), x.thickness, x.layers.as_slice()));
        let ceilings = t.ceiling.iter().map(|(n, x)| ("ceiling", n.as_str(), x.thickness, x.layers.as_slice()));
        walls.chain(floors).chain(roofs).chain(ceilings)
    }

    /// The layers of type `name` of `category`, a wall's from its exterior in, the others' from
    /// the top down; none for a type without layers.
    pub fn type_layers(&self, category: &str, name: &str) -> &[Layer] {
        self.layered_types().find(|(c, n, ..)| *c == category && *n == name).map_or(&[], |(.., l)| l)
    }

    /// Every type's layers, to change.
    pub fn layers_mut(&mut self) -> impl Iterator<Item = &mut Layer> {
        let Types { wall, floor, roof, ceiling, .. } = &mut self.types;
        (wall.values_mut().flat_map(|t| t.layers.iter_mut()))
            .chain(floor.values_mut().flat_map(|t| t.layers.iter_mut()))
            .chain(roof.values_mut().flat_map(|t| t.layers.iter_mut()))
            .chain(ceiling.values_mut().flat_map(|t| t.layers.iter_mut()))
    }

    /// Every material the types' layers name that the library hasn't, put in it: what a project
    /// from before the library, or a file from elsewhere, comes in with.
    pub fn adopt_materials(&mut self) {
        let mut named: Vec<String> = self.layered_types().flat_map(|(.., l)| l.iter().map(|l| l.material.clone())).collect();
        named.extend(self.families.values().flat_map(|f| f.materials()).map(String::from));
        named.extend(self.types.runs.values().flat_map(|t| t.values().map(|t| t.material.clone())));
        named.extend(self.elements.values().flat_map(|e| self.component_materials(e)).map(String::from));
        for n in named.into_iter().filter(|n| !n.is_empty()) {
            self.materials.entry(n).or_default();
        }
    }

    /// The materials component `e` names itself, in its material parameters.
    fn component_materials<'a>(&self, e: &'a Element) -> Vec<&'a str> {
        let (Some((family, _)), Some(params)) = (e.family(), e.params()) else { return Vec::new() };
        let Some(f) = self.families.get(family) else { return Vec::new() };
        params.iter().filter(|(k, _)| f.material_parameter(k)).filter_map(|(_, v)| v.as_str()).collect()
    }

    /// What names material `name` besides the types' layers: a family, or a component of one.
    pub fn material_user(&self, name: &str) -> Option<String> {
        if let Some((n, _)) = self.families.iter().find(|(_, f)| f.materials().contains(&name)) {
            return Some(format!("family `{n}`"));
        }
        let painted = |e: &Element| {
            e.params().and_then(|p| p.get(crate::paint::KEY)?.as_object()).is_some_and(|o| o.values().any(|v| v.as_str() == Some(name)))
        };
        if let Some((id, e)) = self.elements.iter().find(|(_, e)| painted(e)) {
            return Some(format!("the paint of {} {id}", e.category()));
        }
        self.elements.iter().find(|(_, e)| self.component_materials(e).contains(&name)).map(|(id, _)| format!("component {id}"))
    }

    /// Whether the values `params` give an element of family `name` are its instance parameters' own: what the
    /// element sets of the family are instance parameters, none worked out by a formula, materials in the library.
    fn family_values_ok(&self, who: &str, name: &str, f: &Family, params: &BTreeMap<String, serde_json::Value>) -> Result<(), String> {
        for (k, v) in params {
            let Some(p) = f.parameters.iter().find(|p| p.name == *k) else { continue };
            if !p.instance {
                return Err(format!("{who}: `{k}` is a type parameter of `{name}`; change it in its type, with `family`"));
            }
            if p.formula.is_some() {
                return Err(format!("{who}: `{k}` is worked out by its formula"));
            }
            if let crate::family::Val::Text(m) = crate::family::read(p, v).map_err(|e| format!("{who}: {e}"))?
                && !m.is_empty()
                && !self.materials.contains_key(&m)
            {
                return Err(format!("{who}: material `{m}` isn't in the library; add it with `material`"));
            }
        }
        Ok(())
    }

    /// Material `from`, renamed `to` in the families and their components.
    pub fn rename_material_in_families(&mut self, from: &str, to: &str) {
        for f in self.families.values_mut() {
            f.rename_material(from, to);
        }
        let Model { families, elements, .. } = self;
        for e in elements.values_mut() {
            // Paint of it, on any face.
            if let Some(serde_json::Value::Object(paint)) = e.params_mut().and_then(|p| p.get_mut(crate::paint::KEY)) {
                for v in paint.values_mut().filter(|v| v.as_str() == Some(from)) {
                    *v = serde_json::Value::String(to.into());
                }
            }
            if let Some(f) = e.family().and_then(|(f, _)| families.get(f))
                && let Some(params) = e.params_mut()
            {
                for (k, v) in params.iter_mut() {
                    if f.material_parameter(k) && v.as_str() == Some(from) {
                        *v = serde_json::Value::String(to.into());
                    }
                }
            }
        }
    }

    /// Component `c`'s solids in its own axes, mirrored if it is: each visible form's triangles, its
    /// outline seen from above, from its bottom to its top over its base, what it's made of; none
    /// for one that doesn't build.
    pub fn component_local_pieces(&self, c: &Component) -> Vec<Piece> {
        let Some(f) = self.families.get(&c.family) else { return Vec::new() };
        let Ok(pieces) = f.values(&c.type_name, &c.params).and_then(|v| f.pieces(&v)) else { return Vec::new() };
        if !c.mirrored {
            return pieces;
        }
        // Mirrored, its y turns about its x, and so do its triangles' and outlines' windings.
        let flip = |p: [f64; 3]| [p[0], -p[1], p[2]];
        pieces
            .into_iter()
            .map(|pc| Piece {
                tris: pc.tris.iter().map(|[a, b, t]| [flip(*a), flip(*t), flip(*b)]).collect(),
                outline: pc.outline.iter().rev().map(|q| P2(q.0, -q.1)).collect(),
                ..pc
            })
            .collect()
    }

    /// Component `c`'s solids in the model: its own, turned and moved to where it stands.
    pub fn component_pieces(&self, c: &Component) -> Vec<Piece> {
        let base = self.elevation(c.level) + c.offset;
        let (sin, cos) = c.angle.to_radians().sin_cos();
        let place = |p: [f64; 3]| [c.at.0 + p[0] * cos - p[1] * sin, c.at.1 + p[0] * sin + p[1] * cos, base + p[2]];
        self.component_local_pieces(c)
            .into_iter()
            .map(|pc| Piece {
                tris: pc.tris.iter().map(|t| t.map(place)).collect(),
                outline: pc.outline.iter().map(|q| place([q.0, q.1, 0.0])).map(|p| P2(p[0], p[1])).collect(),
                bottom: base + pc.bottom,
                top: base + pc.top,
                ..pc
            })
            .collect()
    }

    /// Adds an element unchecked; the engine runs [`Model::check`] after every command.
    /// It's given the next id and a new GUID.
    pub fn insert(&mut self, e: Element) -> Id {
        let id = self.fresh_id();
        self.elements.insert(id, e);
        id
    }

    /// Every element's id has six digits or more, and what an element points at is an element of the project, by its GUID.
    /// The library's ids and GUIDs are those of what it has, no name is a GUID, and no id or GUID is two things'.
    fn check_ids(&self) -> Result<(), String> {
        if !self.library_in_step() {
            return Err("the library's ids and GUIDs are not those of what it has".into());
        }
        for (id, e) in self.library.entries() {
            if id < FIRST_ID {
                return Err(format!("{} `{}`: ids start at {FIRST_ID}", e.kind.label(), e.name));
            }
            if self.elements.contains_key(id) {
                return Err(format!("{} `{}` and element {id} have the same id", e.kind.label(), e.name));
            }
            if self.elements.contains_key(e.guid) {
                return Err(format!("{} `{}` has the GUID of element {}", e.kind.label(), e.name, self.shown(e.guid)));
            }
            if Guid::parse(&e.name).is_some_and(|g| g.pending().is_none()) {
                return Err(format!("{} `{}`: a name isn't a GUID; name it otherwise", e.kind.label(), e.name));
            }
        }
        for (id, e) in self.elements.iter() {
            if *id < FIRST_ID {
                return Err(format!("element {id}: ids start at {FIRST_ID}"));
            }
            for g in e.refs() {
                match g.pending() {
                    Some(n) if self.elements.contains_key(n) => {
                        return Err(format!("{} {id} points at element {n} by its id, not its GUID", e.category()));
                    }
                    Some(n) => return Err(format!("{} {id} points at element {n}, which the project hasn't", e.category())),
                    None if !self.elements.contains_key(g) => {
                        return Err(format!("{} {id} points at {g}, which no element of the project is", e.category()));
                    }
                    None => {}
                }
            }
        }
        Ok(())
    }

    /// A project read from a file: from an older format, brought up to this one; from a newer, refused, saying so.
    pub fn from_json(text: &str) -> Result<Model, String> {
        Model::read(text).map(|(m, _)| m)
    }

    /// Read from a project file, with what this BIMCraft doesn't know of it left out: a field a newer one added, or a
    /// misspelled one. The fields left out come back as JSON pointers into the file (`/elements/100004/finish`). A file
    /// of a newer format is refused whole, since what changed may be more than a field.
    pub fn read(text: &str) -> Result<(Model, Vec<String>), String> {
        use crate::lenient;
        use serde_json::Value;
        let mut v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let format = v.get("format").map_or(Some(0), Value::as_u64).ok_or("`format` must be a whole number")?;
        if format > u64::from(FORMAT) {
            return Err(format!(
                "it was saved by a newer BIMCraft (file format {format}; this one reads up to {FORMAT}): update BIMCraft to open it"
            ));
        }
        if format < 4 {
            pens_from_pixels(&mut v);
        }
        let mut left_out = Vec::new();
        // An element is an internally tagged enum, which serde reads whole: each is looked into as its category's struct.
        let mut elements = v.get_mut("elements").map_or(Value::Null, Value::take);
        if let Value::Object(es) = &mut elements {
            for (id, e) in es.iter_mut() {
                let mut bare = e.clone();
                let category = bare.as_object_mut().and_then(|o| {
                    o.remove("guid");
                    o.remove("category")
                });
                for p in lenient::element(category.as_ref().and_then(Value::as_str).unwrap_or(""), &bare) {
                    lenient::remove(e, &p);
                    left_out.push(format!("/elements/{id}{p}"));
                }
            }
        }
        // A detail group's members are elements too, held by their type, and are looked into the same way.
        if let Some(Value::Object(groups)) = v.get_mut("detail_groups") {
            for (name, group) in groups.iter_mut() {
                let Some(Value::Array(members)) = group.get_mut("members") else { continue };
                for (i, member) in members.iter_mut().enumerate() {
                    let Some(e) = member.get_mut("element") else { continue };
                    let mut bare = e.clone();
                    let category = bare.as_object_mut().and_then(|o| o.remove("category"));
                    for p in lenient::element(category.as_ref().and_then(Value::as_str).unwrap_or(""), &bare) {
                        lenient::remove(e, &p);
                        left_out.push(format!("/detail_groups/{}/members/{i}/element{p}", lenient::escaped(name)));
                    }
                }
            }
        }
        if v.is_object() {
            v["elements"] = Value::Object(serde_json::Map::new());
            let found = lenient::unknown::<Model>(&v);
            for p in found {
                lenient::remove(&mut v, &p);
                left_out.push(p);
            }
            v["elements"] = elements;
        }
        let mut m: Model = serde_json::from_value(v).map_err(|e| e.to_string())?;
        m.migrate();
        m.names_from_file();
        Ok((m, left_out))
    }

    /// Brought up to [`FORMAT`]. From before pens, see [`pens_from_pixels`]. From before GUIDs: each element was given one as it was read, what points at an element
    /// (its id, then) points at its GUID, and the ids move up to six digits, by as much as takes the first to [`FIRST_ID`]
    /// (`1` is `100000`, `7` is `100006`).
    pub fn migrate(&mut self) {
        if self.format < 1 {
            self.resolve();
            if self.elements.keys().next().is_some_and(|&first| first < FIRST_ID) {
                let shift = FIRST_ID - 1;
                let low: Vec<Id> = self.elements.keys().copied().filter(|&id| id < FIRST_ID).collect();
                match self.elements.renumbered(|id| if id < FIRST_ID { id + shift } else { id }) {
                    Some(es) => {
                        self.elements = es;
                        self.next_id = self.next_id.saturating_add(shift);
                    }
                    // Some of the ids it would take are taken: those get new ones.
                    None => {
                        for old in low {
                            if let (Some(e), Some(g)) = (self.elements.get(old).cloned(), self.elements.guid(old)) {
                                self.elements.remove(old);
                                let id = self.fresh_id();
                                let _ = self.elements.insert_as(id, g, e);
                            }
                        }
                    }
                }
            }
        }
        self.next_id = self.next_id.max(FIRST_ID);
        // A file from before line styles leaves the patterns and styles out, so it has the English ones, whatever its
        // language: named in its language (a file that holds them has them by GUID, never equal to these).
        if self.language != Language::English
            && self.line_styles == lines::default_styles()
            && self.line_patterns == lines::default_patterns()
        {
            (self.line_styles, self.line_patterns) = (lines::styles(self.language), lines::patterns(self.language));
        }
        // 4 to 5 (scans, and the point clouds that place them) has no step: a file of 4 holds none of it, or holds it as 5 does.
        // 5 to 6 (detailing: lines, regions, groups, repeating details, line and fill styles) has none either: a file of 5 holds
        // none of it, so reading it as 6 changes nothing.
        self.format = FORMAT;
    }

    /// What points at an element by its id (as a command gives a reference, or a file from before GUIDs held it) points at
    /// its GUID. An id no element has is left as it is, for [`Model::check`] to name.
    pub fn resolve(&mut self) {
        let pending = |e: &Element| e.refs().iter().any(|g| g.pending().is_some());
        let ids: Vec<Id> = self.elements.iter().filter(|(_, e)| pending(e)).map(|(id, _)| *id).collect();
        for id in ids {
            let Some(mut e) = self.elements.get(id).cloned() else { continue };
            e.refs_mut(&mut |g| {
                if let Some(guid) = g.pending().and_then(|i| self.elements.guid(i)) {
                    *g = guid;
                }
            });
            self.elements.insert(id, e);
        }
    }

    /// The next id, taken: past every element's, since a hand-edited file can carry a stale `next_id`.
    pub fn fresh_id(&mut self) -> Id {
        let mut id = self.next_id.max(FIRST_ID).max(self.elements.keys().next_back().map_or(FIRST_ID, |k| k + 1));
        // The library counts down from the top: past an id of its, on.
        while self.library.taken(id) {
            id += 1;
        }
        self.next_id = id + 1;
        id
    }

    /// Where element `id`'s model pattern starts (its `pattern_origin`) once its pattern line `line` lies on `reference`:
    /// the origin moved across by the gap between the two, which must run the same way (plan mm, a line by its two ends).
    /// What Align on a pattern asks of the model; set the answer with `modify`. A filled region or a floor has a pattern to
    /// align; the error says what else is wrong.
    pub fn align_pattern(&self, id: Id, reference: (P2, P2), line: (P2, P2)) -> Result<P2, String> {
        let origin = match self.elements.get(id) {
            Some(Element::FilledRegion(r)) => r.pattern_origin,
            Some(Element::Floor(f)) => f.pattern_origin,
            Some(e) => return Err(format!("{} {id} has no model pattern to align; a filled region or a floor has", e.category())),
            None => return Err(format!("no element {id}")),
        };
        if [reference, line].iter().any(|(a, b)| !(a.is_finite() && b.is_finite()) || a.dist(*b) < TOL) {
            return Err("align: a line of no length, or no number, can't be aligned to".into());
        }
        if !crate::geom::parallel(reference.0, reference.1, line.0, line.1) {
            return Err("align: the lines aren't parallel".into());
        }
        let across = (reference.1 - reference.0).unit().perp();
        Ok(crate::commands::micron(origin + across * (reference.0 - line.0).dot(across)))
    }

    /// Element `k` as messages name it: its id, or, when the project hasn't it, what `k` is.
    pub fn shown(&self, k: impl Key) -> String {
        self.elements.id(k).map_or_else(|| k.to_string(), |id| id.to_string())
    }

    /// Whether `a` and `b`, ids or GUIDs, are the same element of the project.
    pub fn same(&self, a: impl Key, b: impl Key) -> bool {
        self.elements.id(a).is_some_and(|a| self.elements.id(b) == Some(a))
    }

    /// The GUID of the element `k` finds; the nil GUID, which is no element's, when there's none.
    pub fn guid(&self, k: impl Key) -> Guid {
        self.elements.guid(k).unwrap_or_default()
    }

    pub fn level(&self, id: impl Key) -> Option<&Level> {
        match self.elements.get(id) {
            Some(Element::Level(l)) => Some(l),
            _ => None,
        }
    }

    pub fn wall(&self, id: impl Key) -> Option<&Wall> {
        match self.elements.get(id) {
            Some(Element::Wall(w)) => Some(w),
            _ => None,
        }
    }

    /// The rectangle level `id` stretches over in plan, south-west corner then north-east: its
    /// extent, else [`Model::model_extent`].
    pub fn level_extent(&self, id: impl Key) -> Option<[P2; 2]> {
        Some(self.level(id)?.extent.unwrap_or_else(|| self.model_extent()))
    }

    /// The box round every solid and placed scan, its min and max corners; none in an empty project.
    pub fn model_box(&self) -> Option<([f64; 3], [f64; 3])> {
        let clouds = self.elements.iter().filter_map(|(id, _)| self.cloud_box(*id)).flat_map(|(lo, hi)| [lo, hi]);
        let pts = crate::view3d::solids(self).iter().flat_map(|s| s.corners()).chain(clouds).collect::<Vec<_>>();
        let first = *pts.first()?;
        Some(
            pts.iter()
                .fold((first, first), |(lo, hi), p| (std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i])))),
        )
    }

    /// What a level stretches over without an extent of its own: every solid and grid in plan,
    /// [`LEVEL_PAST`] past them (10 m each way of the origin in an empty project).
    pub fn model_extent(&self) -> [P2; 2] {
        let pts = crate::view3d::solids(self)
            .iter()
            .flat_map(|s| s.corners())
            .map(|p| P2(p[0], p[1]))
            .chain(self.elements.iter().flat_map(|(id, e)| match e {
                Element::Grid(g) => vec![g.start, g.end],
                Element::PointCloud(_) => self.cloud_box(*id).map_or(Vec::new(), |(lo, hi)| vec![P2(lo[0], lo[1]), P2(hi[0], hi[1])]),
                _ => Vec::new(),
            }))
            .collect::<Vec<_>>();
        match pts.first() {
            Some(&first) => {
                let (lo, hi) =
                    pts.iter().fold((first, first), |(lo, hi), q| (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1))));
                [crate::commands::micron(lo - P2(LEVEL_PAST, LEVEL_PAST)), crate::commands::micron(hi + P2(LEVEL_PAST, LEVEL_PAST))]
            }
            None => [P2(-10_000.0, -10_000.0), P2(10_000.0, 10_000.0)],
        }
    }

    /// Level `level`'s extent with the end it shows at in section or elevation `view` dragged to
    /// `to` along its line (its far end with `far`, else its near one): the edge of the extent that
    /// end lies on (across the axis nearest the line's) moves as far as takes it there, leaving it
    /// at least 100 mm across.
    pub fn level_extent_along(&self, level: impl Key, view: impl Key, far: bool, to: f64) -> Option<[P2; 2]> {
        let v = self.section(view)?;
        let extent @ [lo, hi] = self.level_extent(level)?;
        let u = (v.end - v.start).unit();
        let along: Vec<f64> = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)].iter().map(|q| (*q - v.start).dot(u)).collect();
        let now = if far { along.iter().copied().fold(f64::MIN, f64::max) } else { along.iter().copied().fold(f64::MAX, f64::min) };
        let toward = if far { u } else { u * -1.0 };
        // The edge: west, east, south or north, as `geom::edge_grips` orders them; and how far along
        // the line a mm of it goes.
        let (k, rate) = match toward.0.abs() >= toward.1.abs() {
            true => (usize::from(toward.0 > 0.0), u.0),
            false => (2 + usize::from(toward.1 > 0.0), u.1),
        };
        let by = (to - now) / rate;
        if !by.is_finite() {
            return None;
        }
        let edge = match k {
            0 => P2(lo.0 + by, 0.0),
            1 => P2(hi.0 + by, 0.0),
            2 => P2(0.0, lo.1 + by),
            _ => P2(0.0, hi.1 + by),
        };
        let [a, b] = geom::move_edge(extent, k, edge);
        Some([crate::commands::micron(a), crate::commands::micron(b)])
    }

    /// Levels from lowest to highest.
    pub fn levels(&self) -> Vec<(Id, &Level)> {
        let mut v: Vec<(Id, &Level)> = self
            .elements
            .iter()
            .filter_map(|(&id, e)| match e {
                Element::Level(l) => Some((id, l)),
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| a.1.elevation.total_cmp(&b.1.elevation));
        v
    }

    pub fn elevation(&self, level: impl Key) -> f64 {
        self.level(level).map_or(0.0, |l| l.elevation)
    }

    /// Where a placed scan's points are in the model, mm: each point about the scan's middle, turned by `angle` and moved to
    /// `at`, `offset` over its level.
    pub fn cloud_place(&self, pc: &PointCloud) -> impl Fn([f32; 3]) -> [f64; 3] + use<> {
        let (sin, cos) = pc.angle.to_radians().sin_cos();
        let (at, z) = (pc.at, self.elevation(pc.level) + pc.offset);
        move |p: [f32; 3]| {
            let (x, y) = (f64::from(p[0]), f64::from(p[1]));
            [at.0 + x * cos - y * sin, at.1 + x * sin + y * cos, z + f64::from(p[2])]
        }
    }

    /// The scans no point cloud places.
    pub fn unplaced_clouds(&self) -> Vec<String> {
        if self.clouds.is_empty() {
            return Vec::new();
        }
        let placed: BTreeSet<&str> =
            self.elements.values().filter_map(|e| if let Element::PointCloud(pc) = e { Some(pc.cloud.as_str()) } else { None }).collect();
        self.clouds.keys().filter(|n| !placed.contains(n.as_str())).cloned().collect()
    }

    /// The box round a placed scan's points in the model, mm, lowest corner first; none if `id` places no scan.
    pub fn cloud_box(&self, id: Id) -> Option<([f64; 3], [f64; 3])> {
        let Some(Element::PointCloud(pc)) = self.elements.get(id) else { return None };
        let (lo, hi) = self.clouds.get(&pc.cloud)?.bounds();
        let place = self.cloud_place(pc);
        let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for corner in 0..8 {
            let pick = |k: usize| (if corner >> k & 1 == 0 { lo[k] } else { hi[k] }) as f32;
            let p = place([pick(0), pick(1), pick(2)]);
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        Some((min, max))
    }

    /// The next level up from `level`.
    pub fn level_above(&self, level: impl Key) -> Option<Guid> {
        let z = self.elevation(level);
        self.levels().into_iter().find(|(_, l)| l.elevation > z + TOL).map(|(id, _)| self.guid(id))
    }

    pub fn thickness(&self, w: &Wall) -> f64 {
        self.types.wall.get(&w.type_name).map_or(0.0, |t| t.thickness)
    }

    /// Its base by its level and offset, whatever it's attached to: where its doors' and windows'
    /// sills are measured from.
    pub fn own_base(&self, w: &Wall) -> f64 {
        self.elevation(w.level) + w.base_offset
    }

    /// Its top by its top constraint, whatever it's attached to.
    pub fn own_top(&self, w: &Wall) -> f64 {
        match w.top_level {
            Some(t) => self.elevation(t),
            None => self.own_base(w) + w.height,
        }
    }

    /// Its lowest base: on what its base is attached to, or by its level and offset.
    pub fn base_z(&self, w: &Wall) -> f64 {
        crate::walls::heights(self, w).0
    }

    /// Its highest top: under what its top is attached to, or by its top constraint.
    pub fn top_z(&self, w: &Wall) -> f64 {
        crate::walls::heights(self, w).1
    }

    /// Whether a plan of `level` cuts the wall: the wall spans the plan's cut plane.
    pub fn cut_by(&self, w: &Wall, level: impl Key) -> bool {
        self.spans(w, self.plan_range(level).cut)
    }

    /// Whether the wall bounds the rooms of `level`: it spans the height rooms are found at,
    /// [`CUT_HEIGHT`] above the level, whatever the level's plan cuts at.
    pub fn bounding(&self, w: &Wall, level: impl Key) -> bool {
        self.spans(w, self.elevation(level) + CUT_HEIGHT)
    }

    /// Whether the wall spans height `z`.
    pub(crate) fn spans(&self, w: &Wall, z: f64) -> bool {
        self.base_z(w) <= z && z < self.top_z(w)
    }

    /// The view range plan `key` has (a level's own plan, or a plan view): its own, unless a view template it follows controls
    /// it, or it is a dependent view, which has its parent's.
    pub fn view_range_of(&self, key: impl Key) -> ViewRange {
        let key = self.plan_source(key);
        let (range, graphics) = match self.elements.get(key) {
            Some(Element::Level(l)) => (l.view_range, &l.graphics),
            Some(Element::PlanView(v)) => (v.view_range, &v.graphics),
            _ => return ViewRange::default(),
        };
        graphics
            .template
            .as_ref()
            .and_then(|n| self.view_templates.get(n))
            .filter(|t| t.controls.contains(&graphics::Group::Range))
            .and_then(|t| t.view_range)
            .unwrap_or(range)
    }

    /// The heights of the view range of plan `key`. A top that isn't set is the level above
    /// (none above: no limit), and never below the cut plane.
    pub fn plan_range(&self, key: impl Key) -> PlanRange {
        let level = self.plan_level(key).unwrap_or_default();
        let z = self.elevation(level);
        let r = self.view_range_of(key);
        let above = || self.level_above(level).map_or(f64::INFINITY, |a| self.elevation(a));
        PlanRange { top: r.top.map_or_else(above, |t| z + t).max(z + r.cut), cut: z + r.cut, bottom: z + r.bottom, depth: z + r.depth }
    }

    /// The default 3D view's section box, its min and max corners, if plan `key` follows it (a dependent view as its parent
    /// does) and it's on.
    pub fn plan_box(&self, key: impl Key) -> Option<([f64; 3], [f64; 3])> {
        let follows = match self.elements.get(self.plan_source(key)) {
            Some(Element::Level(l)) => l.section_box,
            Some(Element::PlanView(v)) => v.section_box,
            _ => false,
        };
        follows.then(|| self.default_3d().and_then(|v| self.section_box(v))).flatten()
    }

    /// The heights plan `key` shows: its view range's, or, following the section box, from the box's bottom up to its top,
    /// where it cuts. What a plan draws and snaps to; commands that ask what a plan cuts keep its own range.
    pub fn shown_range(&self, key: impl Key) -> PlanRange {
        match self.plan_box(key) {
            Some((lo, hi)) => PlanRange { top: hi[2], cut: hi[2], bottom: lo[2], depth: lo[2] },
            None => self.plan_range(key),
        }
    }

    /// The crop plan `key` is drawn with: its own crop region, cut to the section box's sides when it follows the box.
    pub fn shown_crop(&self, key: impl Key) -> Option<[P2; 2]> {
        let own = self.crop_of(key);
        let Some((lo, hi)) = self.plan_box(key) else { return own };
        let (blo, bhi) = (P2(lo[0], lo[1]), P2(hi[0], hi[1]));
        Some(match own {
            Some([a, b]) => [P2(a.0.max(blo.0), a.1.max(blo.1)), P2(b.0.min(bhi.0), b.1.min(bhi.1))],
            None => [blo, bhi],
        })
    }

    /// The level plan `key` is of: a level itself, or the level of a plan view.
    pub fn plan_level(&self, key: impl Key) -> Option<Id> {
        match self.elements.get(key)? {
            Element::Level(_) => self.elements.id(key),
            Element::PlanView(v) => self.elements.id(v.level),
            _ => None,
        }
    }

    /// Element `id` if it is a plan view.
    pub fn plan_view(&self, id: impl Key) -> Option<&PlanView> {
        match self.elements.get(id) {
            Some(Element::PlanView(v)) => Some(v),
            _ => None,
        }
    }

    /// Plan view `view` if it is a plan view of `level`.
    pub fn plan_view_of(&self, view: impl Key, level: impl Key) -> Option<&PlanView> {
        match self.elements.get(view) {
            Some(Element::PlanView(v)) if self.same(v.level, level) => Some(v),
            _ => None,
        }
    }

    /// The plan views of `level`, in element order.
    pub fn plan_views(&self, level: impl Key) -> Vec<(Id, &PlanView)> {
        self.elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::PlanView(v) if self.same(v.level, level) => Some((*id, v)),
                _ => None,
            })
            .collect()
    }

    /// The plan whose view range, graphics (and so scale and detail) plan `key` follows: itself, or a dependent view's parent.
    pub fn plan_source(&self, key: impl Key) -> Guid {
        match self.elements.get(key) {
            Some(Element::PlanView(PlanView { parent: Some(p), .. })) => *p,
            _ => self.guid(key),
        }
    }

    /// The crop region of plan `key`: its own, whichever kind of plan it is.
    pub fn crop_of(&self, key: impl Key) -> Option<[P2; 2]> {
        match self.elements.get(key)? {
            Element::Level(l) => l.crop,
            Element::PlanView(v) => v.crop,
            _ => None,
        }
    }

    /// The plan an annotation is in: a level's own, or a plan view; none for what isn't annotation of a plan.
    pub fn annotation_plan(&self, e: &Element) -> Option<Guid> {
        match e {
            Element::Dimension(d) => Some(d.view.unwrap_or(d.level)),
            Element::Tag(t) => t.view.or_else(|| self.level_of(e)),
            Element::Text(Note { view, .. })
            | Element::Spot(Spot { view, .. })
            | Element::DetailLine(DetailLine { view, .. })
            | Element::FilledRegion(FilledRegion { view, .. })
            | Element::DetailGroup(DetailGroup { view, .. })
            | Element::RepeatingDetail(RepeatingDetail { view, .. }) => Some(*view),
            _ => None,
        }
    }

    /// Whether plan `key` draws annotation `e`: when it is in the plan, or in the parent of a dependent view.
    pub fn annotation_in(&self, e: &Element, key: impl Key) -> bool {
        let Some(at) = self.annotation_plan(e) else { return false };
        let key = self.guid(key);
        at == key || matches!(self.elements.get(key), Some(Element::PlanView(PlanView { parent: Some(p), .. })) if *p == at)
    }

    /// The names of a category's types (wall, floor, door or window, in any case, singular or
    /// plural); none for the others.
    pub fn type_names(&self, category: &str) -> Vec<&str> {
        fn names<T>(m: &BTreeMap<String, T>) -> Vec<&str> {
            m.keys().map(String::as_str).collect()
        }
        match crate::commands::category(category).as_str() {
            "region" => names(&self.region_types),
            "wall" => names(&self.types.wall),
            "floor" => names(&self.types.floor),
            "door" => names(&self.types.door),
            "window" => names(&self.types.window),
            "column" => names(&self.types.column),
            "structuralcolumn" => names(&self.types.structural_column),
            "footing" => names(&self.types.footing),
            "roof" => names(&self.types.roof),
            "ceiling" => names(&self.types.ceiling),
            "railing" => names(&self.types.railing),
            run if RUN_CATEGORIES.contains(&run) => self.types.runs.get(run).map(names).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The type a new element of `category` gets when none is named: the one a new model starts
    /// with, else the first by name.
    pub fn default_type(&self, category: &str) -> Option<&str> {
        let preferred = match crate::commands::category(category).as_str() {
            "region" => return fills::default_region_type(&self.region_types, self.language).map(String::as_str),
            "wall" | "floor" => "Generic - 200mm",
            "door" => "Single 900 x 2100",
            "column" => "Rectangular 400 x 400",
            "roof" => "Generic - 300mm",
            "ceiling" => "Generic - 20mm",
            "railing" => "Railing - 1000mm",
            "structuralcolumn" => "Concrete 300 x 300",
            "footing" => "Footing 1200 x 1200",
            "beam" => "Beam 200 x 500",
            "stripfooting" => "Strip 600 x 300",
            "pipe" => "PVC 50",
            "conduit" => "PVC 25",
            "cabletray" => "Perforated 100 x 50",
            "wire" => "Wire 2.5",
            "downconductor" => "Steel 8",
            "groundconductor" => "Copper 9",
            "cable" => "Cat6",
            _ => "Fixed 1200 x 1500",
        };
        let names = self.type_names(category);
        names.iter().find(|n| **n == preferred).or(names.first()).copied()
    }

    /// The family and type a new door or window gets in an interface that places families: the project's single door or fixed window
    /// at their usual size, else its first family of the category and that family's first type.
    pub fn default_family(&self, category: &str) -> Option<(&str, &str)> {
        let (kind, usual, size) = match crate::commands::category(category).as_str() {
            "door" => (crate::family::Category::Door, "Single Door", "900 x 2100"),
            "window" => (crate::family::Category::Window, "Fixed Window", "1200 x 1500"),
            _ => return None,
        };
        let usual = self.language.word(usual);
        let (name, family) = self
            .families
            .get_key_value(usual)
            .filter(|(_, f)| f.category == kind)
            .or_else(|| self.families.iter().find(|(_, f)| f.category == kind))?;
        let ty = family.types.get_key_value(size).or_else(|| family.types.iter().next())?.0;
        Some((name.as_str(), ty.as_str()))
    }

    /// The size of a door or window and its default sill: its type's, or its family's `Width`, `Height` and
    /// `Sill Height` for the type it is, in its wall, with the values it holds of its own; or why it has none.
    pub fn opening_dims(&self, e: &Element) -> Result<OpeningType, String> {
        let (Element::Door(o) | Element::Window(o)) = e else { return Err(format!("{} isn't a door or a window", e.category())) };
        let is_door = matches!(e, Element::Door(_));
        let Some(name) = &o.family else {
            let types = if is_door { &self.types.door } else { &self.types.window };
            return types.get(&o.type_name).copied().ok_or_else(|| {
                let names: Vec<&str> = types.keys().map(String::as_str).collect();
                format!("type `{}` doesn't exist (there are: {})", o.type_name, names.join(", "))
            });
        };
        let f = self.families.get(name).ok_or_else(|| {
            let names: Vec<&str> =
                self.families.iter().filter(|(_, f)| Some(f.category) == e.hosted_category()).map(|(n, _)| n.as_str()).collect();
            format!("family `{name}` doesn't exist (there are: {})", names.join(", "))
        })?;
        if Some(f.category) != e.hosted_category() {
            return Err(format!("family `{name}` isn't a {} family", e.category()));
        }
        let wall = self.wall(o.host).map_or(crate::family::DEFAULT_WALL, |w| self.thickness(w));
        let values = f.values_in(&o.type_name, &o.params, wall)?;
        let number = |key: &str| match values.get(key) {
            Some(crate::family::Val::Number(x)) => Some(*x),
            _ => None,
        };
        let need = |key: &str| number(key).ok_or_else(|| format!("family `{name}` has no length `{key}`"));
        Ok(OpeningType {
            width: need(crate::family::WIDTH)?,
            height: need(crate::family::HEIGHT)?,
            sill: number(crate::family::SILL).unwrap_or(if is_door { 0.0 } else { 900.0 }),
        })
    }

    /// The type of a door or window.
    pub fn opening_type(&self, e: &Element) -> Option<OpeningType> {
        self.opening_dims(e).ok()
    }

    /// Where a door's or window's family's axes lie: the point in plan at the middle of the opening in the middle
    /// of its wall, the way the wall runs there, and the height of its sill.
    pub fn opening_frame(&self, o: &Opening) -> Option<(P2, P2, f64)> {
        let w = self.wall(o.host)?;
        let axis = walls::Axis::of(w);
        Some((axis.at(o.offset, walls::centre(self, w)), axis.tangent(o.offset), self.own_base(w) + o.sill))
    }

    /// A door's or window's family's solids and the lines its plan draws, in the family's own axes, turned out with
    /// the opening's flips as mirror images (its hinge on the other jamb, its swing to the wall's other side);
    /// none for one that isn't of a family, or doesn't build.
    pub fn opening_local(&self, e: &Element) -> Option<(Vec<Piece>, Vec<Vec<P2>>)> {
        let (Element::Door(o) | Element::Window(o)) = e else { return None };
        let f = self.families.get(o.family.as_ref()?)?;
        let values = f.values_in(&o.type_name, &o.params, self.thickness(self.wall(o.host)?)).ok()?;
        let (pieces, strokes) = (f.pieces(&values).ok()?, f.strokes(&values).ok()?);
        let (fx, fy) = (if o.flip_hand { -1.0 } else { 1.0 }, if o.flip_facing { -1.0 } else { 1.0 });
        let mirrored = fx * fy < 0.0;
        let flip = |p: [f64; 3]| [fx * p[0] + 0.0, fy * p[1] + 0.0, p[2]];
        let pieces = pieces
            .into_iter()
            .map(|pc| {
                let tris =
                    pc.tris.iter().map(|[a, b, c]| if mirrored { [flip(*a), flip(*c), flip(*b)] } else { [flip(*a), flip(*b), flip(*c)] });
                let outline: Vec<P2> = pc.outline.iter().map(|q| P2(fx * q.0 + 0.0, fy * q.1 + 0.0)).collect();
                Piece { tris: tris.collect(), outline: if mirrored { outline.into_iter().rev().collect() } else { outline }, ..pc }
            })
            .collect();
        let strokes = strokes.into_iter().map(|l| l.into_iter().map(|q| P2(fx * q.0 + 0.0, fy * q.1 + 0.0)).collect()).collect();
        Some((pieces, strokes))
    }

    /// A door's or window's family's solids where it stands, in the model's own axes: each form's triangles, its
    /// outline seen from above, and what it reaches from and to, over the level.
    pub fn opening_pieces(&self, e: &Element) -> Vec<Piece> {
        let (Element::Door(o) | Element::Window(o)) = e else { return Vec::new() };
        let (Some((pieces, _)), Some((at, u, z))) = (self.opening_local(e), self.opening_frame(o)) else { return Vec::new() };
        let place = |p: [f64; 3]| {
            let q = at + u * p[0] + u.perp() * p[1];
            [q.0, q.1, z + p[2]]
        };
        pieces
            .into_iter()
            .map(|pc| Piece {
                tris: pc.tris.iter().map(|t| t.map(place)).collect(),
                outline: pc.outline.iter().map(|q| place([q.0, q.1, 0.0])).map(|p| P2(p[0], p[1])).collect(),
                bottom: z + pc.bottom,
                top: z + pc.top,
                ..pc
            })
            .collect()
    }

    /// The lines a door's or window's family draws in plan, where it stands.
    pub fn opening_strokes(&self, e: &Element) -> Vec<Vec<P2>> {
        let (Element::Door(o) | Element::Window(o)) = e else { return Vec::new() };
        let (Some((_, strokes)), Some((at, u, _))) = (self.opening_local(e), self.opening_frame(o)) else { return Vec::new() };
        strokes.into_iter().map(|l| l.into_iter().map(|q| at + u * q.0 + u.perp() * q.1).collect()).collect()
    }

    /// The level an element stands on: a door's or window's is its wall's, a tag's its door's or
    /// window's. Levels and grids stand on none.
    pub fn level_of(&self, e: &Element) -> Option<Guid> {
        let wall_level = |o: &Opening| self.wall(o.host).map(|w| w.level);
        match e {
            Element::PointCloud(pc) => Some(pc.level),
            Element::Wall(w) => Some(w.level),
            Element::Floor(f) => Some(f.level),
            Element::Ceiling(c) => Some(c.level),
            Element::Shaft(sh) => Some(sh.level),
            Element::Mass(ms) => Some(ms.level),
            Element::Room(r) => Some(r.level),
            Element::Dimension(d) => Some(d.level),
            Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => Some(c.level),
            run_arm!(r) => Some(r.level),
            Element::Component(c) => Some(c.level),
            Element::Stair(st) => Some(st.level),
            Element::Ramp(r) => Some(r.level),
            // On a stair or a ramp, its host's.
            Element::Railing(r) => r.host.and_then(|h| self.elements.get(h)).and_then(|e| self.level_of(e)).or(Some(r.level)),
            Element::Roof(r) => Some(r.level),
            Element::Door(o) | Element::Window(o) => wall_level(o),
            Element::Tag(t) => self.elements.get(t.host).filter(|h| !matches!(h, Element::Tag(_))).and_then(|h| self.level_of(h)),
            // Notes and spot elevations in a level's plan.
            Element::Text(Note { view, .. })
            | Element::Spot(Spot { view, .. })
            | Element::DetailLine(DetailLine { view, .. })
            | Element::FilledRegion(FilledRegion { view, .. })
            | Element::DetailGroup(DetailGroup { view, .. })
            | Element::RepeatingDetail(RepeatingDetail { view, .. }) => self.plan_level(*view).map(|l| self.guid(l)),
            // A plan view stands on its level: it goes with it.
            Element::PlanView(v) => Some(v.level),
            Element::Level(_)
            | Element::Grid(_)
            | Element::Section(_)
            | Element::Elevation(_)
            | Element::Sheet(_)
            | Element::Viewport(_)
            | Element::Schedule(_)
            | Element::Link(_)
            | Element::View3D(_) => None,
        }
    }

    /// From a base level and offset up to a top level, or `height` above the base: bottom and top.
    pub fn span(&self, level: impl Key, base_offset: f64, top_level: Option<Guid>, height: f64) -> (f64, f64) {
        let base = self.elevation(level) + base_offset;
        (base, top_level.map_or(base + height, |t| self.elevation(t)))
    }

    /// Whether the horizontal plane at height `z` passes through mass `ms`.
    pub fn mass_cut_by(&self, ms: &Mass, z: f64) -> bool {
        let (bottom, top) = self.span(ms.level, ms.base_offset, ms.top_level, ms.height);
        bottom <= z && z < top
    }

    /// The outline of mass `ms` at height `z`: its footprint, or when it's lofted each corner that far along its straight way
    /// to its twin in the top outline.
    pub fn mass_outline(&self, ms: &Mass, z: f64) -> Vec<P2> {
        if !ms.lofted() {
            return ms.footprint();
        }
        let (bottom, top) = self.span(ms.level, ms.base_offset, ms.top_level, ms.height);
        let t = if top > bottom { ((z - bottom) / (top - bottom)).clamp(0.0, 1.0) } else { 0.0 };
        crate::massing::ring_at(&ms.footprint(), &ms.top_ring(), t)
    }

    /// The volume of mass `ms` between heights `from` and `to`, mm³. Its cross section's area is a quadratic of the height, so
    /// Simpson's rule gives it exactly.
    pub fn mass_volume(&self, ms: &Mass, from: f64, to: f64) -> f64 {
        let area = |z: f64| geom::area(&self.mass_outline(ms, z)).abs();
        (to - from) / 6.0 * (area(from) + 4.0 * area((from + to) / 2.0) + area(to))
    }

    /// The levels a mass crosses, lowest first: from its base up to, not including, its top. Each with its elevation and how
    /// far the mass goes up from there, to the next of them or its top: what its floor there holds.
    pub fn mass_slices(&self, ms: &Mass) -> Vec<(Id, f64, f64)> {
        let (bottom, top) = self.span(ms.level, ms.base_offset, ms.top_level, ms.height);
        let crossed: Vec<(Id, f64)> =
            self.levels().into_iter().map(|(id, l)| (id, l.elevation)).filter(|(_, z)| *z >= bottom - TOL && *z < top - TOL).collect();
        crossed.iter().enumerate().map(|(i, (id, z))| (*id, *z, crossed.get(i + 1).map_or(top, |next| next.1) - z)).collect()
    }

    /// A stair's flight: how many risers, how high each is, and how long its runs are together
    /// (a tread less each, the last riser of each climbing onto a landing or the floor above).
    pub fn flight(&self, st: &Stair) -> (u32, f64, f64) {
        let (base, top) = self.span(st.level, 0.0, st.top_level, st.height);
        let rise = top - base;
        // Not a number, or less than one, is none.
        let n = (rise / st.riser - 1e-9).ceil().clamp(0.0, 1e6) as u32;
        let each = if n > 0 { rise / f64::from(n) } else { 0.0 };
        let runs = u32::try_from(st.landings.len()).unwrap_or(u32::MAX).saturating_add(1);
        (n, each, f64::from(n.saturating_sub(runs)) * st.tread)
    }

    /// A roof's outline in plan: its footprint, or the rectangle its extrusion sweeps, counter-clockwise.
    pub fn roof_outline(&self, r: &Roof) -> Vec<P2> {
        match &r.extrusion {
            Some(x) => {
                let (o, u) = (x.line[0], (x.line[1] - x.line[0]).unit());
                let (a0, a1) = (x.profile.first().map_or(0.0, |p| p.0), x.profile.last().map_or(0.0, |p| p.0));
                ccw(&[(a0, x.from), (a1, x.from), (a1, x.to), (a0, x.to)].map(|(a, c)| o + u * a + u.perp() * c))
            }
            None => ccw(&r.boundary),
        }
    }

    /// How far a roof's top lies over its underside where that's in `plane`: its type's thickness, plumb.
    pub fn roof_rise(&self, r: &Roof, plane: &walls::Plane) -> f64 {
        self.types.roof.get(&r.type_name).map_or(0.0, |t| t.thickness) * plane.g.dot(plane.g).sqrt().hypot(1.0)
    }

    /// A roof's slopes. Over its footprint, flat at its base, or each edge but its gables sloping
    /// up from its eaves until the slopes meet (the footprint's straight skeleton), round its
    /// courtyards too; or its extrusion's profile, each stretch swept across. Where it runs into
    /// the roof it joins, it ends at that one's top.
    pub fn roof_faces(&self, r: &Roof) -> Result<Vec<Slope>, String> {
        let base = self.elevation(r.level) + r.offset;
        let faces = match &r.extrusion {
            Some(x) => {
                let (o, u) = (x.line[0], (x.line[1] - x.line[0]).unit());
                x.profile
                    .windows(2)
                    .filter_map(|w| match *w {
                        [a, b] if b.0 > a.0 => {
                            let k = (b.1 - a.1) / (b.0 - a.0);
                            let plane = walls::Plane { z: base + a.1 - k * (a.0 + o.dot(u)), g: u * k };
                            let ring = [(a.0, x.from), (b.0, x.from), (b.0, x.to), (a.0, x.to)].map(|(s, c)| o + u * s + u.perp() * c);
                            Some((vec![ring.to_vec()], plane))
                        }
                        _ => None,
                    })
                    .collect()
            }
            None if r.slope == 0.0 => {
                let courtyards = r.courtyards.iter().map(|c| ccw(c).into_iter().rev().collect());
                vec![(std::iter::once(ccw(&r.boundary)).chain(courtyards).collect(), walls::Plane::flat(base))]
            }
            None => {
                // Its outline counter-clockwise and its courtyards clockwise, the roof to the left of
                // every edge; its edges left upright, counted the other way round on a ring turned round.
                let up = self.roof_uprights(r)?;
                let (mut rings, mut speeds) = (Vec::new(), Vec::new());
                for (k, given) in std::iter::once(&r.boundary).chain(&r.courtyards).enumerate() {
                    let (n, first) = (given.len(), speeds.len());
                    let turn = (geom::area(given) < 0.0) == (k == 0);
                    rings.push(if turn { given.iter().rev().copied().collect() } else { given.clone() });
                    speeds.extend(
                        (0..n).map(|i| if up.get(first + if turn { (2 * n - 2 - i) % n } else { i }) == Some(&true) { 0.0 } else { 1.0 }),
                    );
                }
                if !(3..=MAX_ROOF_POINTS).contains(&speeds.len()) {
                    let n = speeds.len();
                    return Err(format!("a pitched roof's footprint has 3 to {MAX_ROOF_POINTS} points, courtyards and all, not {n}"));
                }
                let tan = r.slope.to_radians().tan();
                let eave = base - r.overhang * tan;
                let skeleton = crate::skeleton::faces(&rings, &speeds)?;
                rings
                    .iter()
                    .flat_map(|ring| geom::ring_edges(ring))
                    .zip(skeleton)
                    .filter_map(|((a, b), face)| {
                        let inward = (b - a).unit().perp();
                        let plane = walls::Plane { z: eave - tan * a.dot(inward), g: inward * tan };
                        let loops: Vec<Vec<P2>> = face.into_iter().map(|l| l.into_iter().map(|(p, _)| p).collect()).collect();
                        loops.first().is_some_and(|l| l.len() >= 3).then_some((loops, plane))
                    })
                    .collect()
            }
        };
        let Some(h) = r.join else { return Ok(faces) };
        let host = match self.elements.get(h) {
            Some(Element::Roof(host)) if host.join.is_none() => host,
            _ => return Err(format!("it joins element {h}, which isn't a roof that joins none")),
        };
        // Over the roof it joins, only where its top is over that one's.
        let tops: Vec<(Vec<P2>, walls::Plane)> = self
            .roof_pieces(host)?
            .into_iter()
            .flat_map(|(ring, p)| {
                let top = walls::Plane { z: p.z + self.roof_rise(host, &p), ..p };
                walls::convex_pieces(&ring).into_iter().map(move |c| (c, top))
            })
            .collect();
        Ok(faces
            .into_iter()
            .flat_map(|(loops, plane)| {
                let mine = walls::Plane { z: plane.z + self.roof_rise(r, &plane), ..plane };
                let mut kept = Vec::new();
                let mut rest = solid(&loops, &[]);
                for (piece, top) in &tops {
                    let mut out = Vec::new();
                    for part in rest {
                        let (inside, outside) = walls::split(part, piece);
                        kept.extend(
                            inside.map(|p| geom::clip(&p, |q| mine.at(q) - top.at(q))).filter(|p| p.len() >= 3 && geom::area(p) > 1.0),
                        );
                        out.extend(outside);
                    }
                    rest = out;
                }
                kept.extend(rest);
                kept.into_iter().map(move |k| (vec![k], plane))
            })
            .collect())
    }

    /// Which edges of a roof's footprint stand upright, gable ends, numbered as `gables` numbers
    /// them: those in `gables`, and with `gable`, a rectangle's two short ends.
    pub fn roof_uprights(&self, r: &Roof) -> Result<Vec<bool>, String> {
        let n = r.edges().count();
        let mut up = vec![false; n];
        for &g in &r.gables {
            *up.get_mut(g).ok_or_else(|| format!("gable edge {g} isn't one of its {n} edges (0 to {})", n.saturating_sub(1)))? = true;
        }
        if r.gable {
            let square = |a: P2, b: P2, c: P2| (b - a).unit().dot((c - b).unit()).abs() < 1e-6;
            let ends = match geom::corners(&ccw(&r.boundary)).as_slice() {
                &[a, b, c, d] if square(a, b, c) && square(b, c, d) && square(c, d, a) => {
                    if a.dist(b) >= b.dist(c) {
                        [(b, c), (d, a)]
                    } else {
                        [(a, b), (c, d)]
                    }
                }
                _ => return Err("`gable` leaves a rectangle's ends upright; over any other footprint, name the edges in `gables`".into()),
            };
            for (u, (p, q)) in up.iter_mut().zip(geom::ring_edges(&r.boundary)) {
                *u |= ends.iter().any(|(a, b)| geom::dist_to_segment(p, *a, *b) < TOL && geom::dist_to_segment(q, *a, *b) < TOL);
            }
        }
        Ok(up)
    }

    /// The roof a plan of `level` shows over `p`, on it or on the level above: of two, the higher.
    pub fn roof_at(&self, level: impl Key, p: P2) -> Option<Id> {
        let above = self.level_above(level);
        let shown = |id: &Id| matches!(self.elements.get(id), Some(Element::Roof(r)) if self.same(r.level, level) || above.is_some_and(|a| self.same(r.level, a)));
        crate::view3d::tops_at(self, p).into_iter().filter(|(id, _)| shown(id)).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
    }

    /// The footprint edge nearest `p`, within `within` mm, of the roofs a plan of `level` shows:
    /// the roof's and the edge's numbers.
    pub fn roof_edge(&self, level: impl Key, p: P2, within: f64) -> Option<(Id, usize)> {
        self.elements
            .iter()
            .filter_map(|(&id, e)| match e {
                Element::Roof(r)
                    if r.extrusion.is_none()
                        && (self.same(r.level, level) || self.level_above(level).is_some_and(|a| self.same(r.level, a))) =>
                {
                    Some((id, r))
                }
                _ => None,
            })
            .flat_map(|(id, r)| r.edges().enumerate().map(move |(i, (a, b))| (geom::dist_to_segment(p, a, b), id, i)))
            .filter(|(d, ..)| *d <= within)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id, i)| (id, i))
    }

    /// A roof's slabs: each piece's underside in space, and how far its top lies over it.
    pub fn roof_slabs(&self, r: &Roof) -> Vec<(Vec<[f64; 3]>, f64)> {
        self.roof_pieces(r).unwrap_or_default().into_iter().map(|(ring, p)| (p.lift(&ring), self.roof_rise(r, &p))).collect()
    }

    /// A roof's slopes less the holes in them and its openings, in convex pieces where a hole cuts one.
    pub fn roof_pieces(&self, r: &Roof) -> Result<Vec<(Vec<P2>, walls::Plane)>, String> {
        Ok(self
            .roof_faces(r)?
            .into_iter()
            .flat_map(|(loops, plane)| solid(&loops, &r.openings).into_iter().map(move |p| (p, plane)))
            .collect())
    }

    /// A floor's top: level, at its level and offset; or through there at its slope arrow's tail,
    /// rising `slope` degrees toward its head.
    pub fn floor_top(&self, f: &Floor) -> walls::Plane {
        let z = self.elevation(f.level) + f.offset;
        match f.arrow {
            Some([tail, head]) if f.slope != 0.0 && tail.dist(head) > 0.0 => {
                let g = (head - tail).unit() * f.slope.to_radians().tan();
                walls::Plane { z: z - g.dot(tail), g }
            }
            _ => walls::Plane::flat(z),
        }
    }

    /// How far a floor's underside lies below its top, plumb: its type's thickness across its slope.
    pub fn floor_depth(&self, f: &Floor) -> f64 {
        self.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness) / f.slope.to_radians().cos()
    }

    /// A floor's slabs: its underside less its openings (in convex pieces once one cuts it), each a
    /// flat polygon in space, counter-clockwise; and how far its top lies above that.
    pub fn floor_slabs(&self, f: &Floor) -> Vec<(Vec<[f64; 3]>, f64)> {
        let (top, depth) = (self.floor_top(f), self.floor_depth(f));
        let under = walls::Plane { z: top.z - depth, ..top };
        solid(&[ccw(&f.boundary)], &f.openings).into_iter().map(|ring| (under.lift(&ring), depth)).collect()
    }

    /// A ceiling's slabs: its underside less its openings (in convex pieces once one cuts it), each
    /// counter-clockwise; and its thickness.
    pub fn ceiling_slabs(&self, c: &Ceiling) -> Vec<(Vec<[f64; 3]>, f64)> {
        let under = walls::Plane::flat(self.elevation(c.level) + c.offset);
        let t = self.types.ceiling.get(&c.type_name).map_or(0.0, |t| t.thickness);
        solid(&[ccw(&c.boundary)], &c.openings).into_iter().map(|ring| (under.lift(&ring), t)).collect()
    }

    /// A floor's, a ceiling's or a roof's slab: `face`, a flat polygon in space, counter-clockwise,
    /// and what lies up to `rise` over it. In plan, what's left of it where shafts pass through it,
    /// and those shafts: a shaft cuts what its span reaches into over its outline, all the way
    /// through.
    pub fn less_shafts(&self, face: &[[f64; 3]], rise: f64) -> (Vec<Vec<P2>>, Vec<Id>) {
        let ring: Vec<P2> = face.iter().map(|p| P2(p[0], p[1])).collect();
        let Some(plane) = walls::Plane::of(face) else { return (vec![ring], Vec::new()) };
        let area = |rings: &[Vec<P2>]| rings.iter().map(|r| geom::area(r).abs()).sum::<f64>();
        let (mut pieces, mut cut) = (vec![ring], Vec::new());
        for (&id, e) in &self.elements {
            let Element::Shaft(sh) = e else { continue };
            let (bottom, top) = self.span(sh.level, sh.base_offset, sh.top_level, sh.height);
            let zs = sh.boundary.iter().map(|q| plane.at(*q));
            let (lo, hi) = zs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)));
            if !(lo < top - TOL && hi + rise > bottom + TOL) {
                continue;
            }
            let left = walls::minus(&pieces, &sh.boundary);
            if area(&left) < area(&pieces) - 1.0 {
                pieces = left;
                cut.push(id);
            }
        }
        (pieces, cut)
    }

    /// The type of a column, a structural column or a footing.
    pub fn column_type(&self, e: &Element) -> Option<&ColumnType> {
        let (col, types) = match e {
            Element::Column(c) => (c, &self.types.column),
            Element::StructuralColumn(c) => (c, &self.types.structural_column),
            Element::Footing(c) => (c, &self.types.footing),
            _ => return None,
        };
        types.get(&col.type_name)
    }

    /// The type of a beam or another run.
    pub fn run_type(&self, e: &Element) -> Option<&RunType> {
        self.types.runs.get(e.category())?.get(&e.run()?.type_name)
    }

    /// A run worked out: its ends and its section. None for another element, or a type it doesn't have.
    pub fn run_geom(&self, e: &Element) -> Option<crate::runs::Geom> {
        Some(crate::runs::Geom::of(self, e.run()?, self.run_type(e)?))
    }

    /// A column's outline in plan (a structural column's, a footing's): its rectangle turned to its angle, or a round one's
    /// 24-sided polygon. Nothing for another element, or an unknown type.
    pub fn column_outline(&self, e: &Element) -> Vec<P2> {
        let (Some(col), Some(t)) = (e.column(), self.column_type(e)) else { return Vec::new() };
        if t.round {
            return (0..24).map(|i| col.at + heading(f64::from(i) * 15.0) * (t.width / 2.0)).collect();
        }
        let (u, w, d) = (heading(col.angle), t.width / 2.0, t.depth / 2.0);
        [(-w, -d), (w, -d), (w, d), (-w, d)].into_iter().map(|(a, c)| col.at + u * a + u.perp() * c).collect()
    }

    /// Sheet `id`.
    pub fn sheet(&self, id: impl Key) -> Option<&Sheet> {
        match self.elements.get(id) {
            Some(Element::Sheet(s)) => Some(s),
            _ => None,
        }
    }

    /// The box 3D view `view` is cut to, its min and max corners: none when it has none, or it's turned off.
    pub fn section_box(&self, view: impl Key) -> Option<([f64; 3], [f64; 3])> {
        match self.elements.get(view) {
            Some(Element::View3D(View3D { section_box: Some(b), .. })) if b.on => Some((b.min, b.max)),
            _ => None,
        }
    }

    /// The default 3D view, `{3D}`: the 3D view of that name, once something has made it.
    pub fn default_3d(&self) -> Option<Id> {
        self.elements.iter().find_map(|(id, e)| matches!(e, Element::View3D(v) if v.name == "{3D}").then_some(*id))
    }

    /// The name of view `id`: a level's plan, a section, an elevation, a schedule or a 3D view;
    /// none for another element.
    pub fn view_name(&self, id: impl Key) -> Option<&str> {
        match self.elements.get(id) {
            Some(Element::Level(l)) => Some(&l.name),
            Some(Element::Section(v) | Element::Elevation(v)) => Some(&v.name),
            Some(Element::Schedule(sc)) => Some(&sc.name),
            Some(Element::View3D(v)) => Some(&v.name),
            Some(Element::PlanView(v)) => Some(&v.name),
            _ => None,
        }
    }

    /// Section or elevation `id`.
    pub fn section(&self, id: impl Key) -> Option<&Section> {
        match self.elements.get(id) {
            Some(Element::Section(v) | Element::Elevation(v)) => Some(v),
            _ => None,
        }
    }

    /// The first of `base`, `base 2`, `base 3`… (or with `numbered`, `base 1`, `base 2`…) that no
    /// view has.
    pub fn free_view_name(&self, base: &str, numbered: bool) -> String {
        let taken: BTreeSet<&str> = self.elements.keys().filter_map(|id| self.view_name(*id)).collect();
        (1..)
            .map(|n| if n == 1 && !numbered { base.to_string() } else { format!("{base} {n}") })
            .find(|n| !taken.contains(n.as_str()))
            .unwrap_or_default()
    }

    /// The box around the walls (faces included) and floors in plan.
    pub fn footprint(&self) -> Option<(P2, P2)> {
        let mut pts = self.elements.iter().flat_map(|(&id, e)| match e {
            Element::Wall(_) => walls::outline(self, id),
            Element::Floor(f) => f.boundary.clone(),
            _ => Vec::new(),
        });
        let first = pts.next()?;
        Some(pts.fold((first, first), |(lo, hi), p| (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))))
    }

    /// The side of the building `at` is on: the way it lies farther from the building's middle.
    pub fn facing_from(&self, at: P2) -> Option<Facing> {
        let (lo, hi) = self.footprint()?;
        let d = at - (lo + hi) * 0.5;
        Some(match (d.0.abs() > d.1.abs(), d.0 > 0.0, d.1 > 0.0) {
            (true, true, _) => Facing::East,
            (true, false, _) => Facing::West,
            (false, _, true) => Facing::North,
            (false, _, false) => Facing::South,
        })
    }

    /// The line of an elevation of the building's `facing` side: 3 m out from it, or through
    /// `through`, and 1 m past it each way; it looks back at the building.
    pub fn elevation_line(&self, facing: Facing, through: Option<P2>) -> Option<(P2, P2)> {
        let (lo, hi) = self.footprint()?;
        let (middle, out) = ((lo + hi) * 0.5, facing.out());
        let u = out.perp();
        let reach = through.map_or((hi - lo).dot(out).abs() / 2.0 + 3000.0, |p| (p - middle).dot(out));
        let half = (hi - lo).dot(u).abs() / 2.0 + 1000.0;
        let base = middle + out * reach;
        Some((base - u * half, base + u * half))
    }

    /// Where a tag points at element `id`: a door's or window's centre, on its wall's centre line; the middle of a wall's or a run's
    /// centre line; a column's, a component's or a room's point; a floor's, ceiling's or roof's outline's centre; a stair's or
    /// ramp's start. None for what has no place in a plan to point at.
    pub fn anchor(&self, id: impl Key) -> Option<P2> {
        let middle = |a: P2, b: P2| Some((a + b) * 0.5);
        match self.elements.get(id)? {
            Element::Door(o) | Element::Window(o) => self.wall(o.host).map(|w| walls::Axis::of(w).at(o.offset, 0.0)),
            Element::Wall(w) => {
                let axis = walls::Axis::of(w);
                Some(axis.at(axis.length() / 2.0, 0.0))
            }
            Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => Some(c.at),
            Element::Component(c) => Some(c.at),
            Element::Room(r) => Some(r.point),
            run_arm!(r) => middle(r.start, r.end),
            Element::Stair(s) => Some(s.start),
            Element::Ramp(r) => Some(r.start),
            Element::Floor(Floor { boundary, .. }) | Element::Ceiling(Ceiling { boundary, .. }) | Element::Roof(Roof { boundary, .. }) => {
                (!boundary.is_empty()).then(|| boundary.iter().fold(P2(0.0, 0.0), |sum, p| sum + *p) * (1.0 / boundary.len() as f64))
            }
            _ => None,
        }
    }

    /// Where a new tag of door or window `host` goes from its centre: beside the wall, 400 mm
    /// past its face, on the side the door doesn't swing to, or the window faces (its wall's
    /// exterior, unless flipped).
    pub fn tag_offset(&self, host: impl Key) -> P2 {
        let (Some(e @ (Element::Door(o) | Element::Window(o))), Some(w)) = (self.elements.get(host), self.wall_of(host)) else {
            // Anything else: a little up and to the right of the point the tag names.
            return if self.anchor(host).is_some() { P2(400.0, 400.0) } else { P2(0.0, 0.0) };
        };
        let (ext, int) = walls::faces(self, w);
        let outside = o.flip_facing == matches!(e, Element::Door(_));
        let p = walls::Axis::of(w).tangent(o.offset).perp() * if outside { ext + 400.0 } else { int - 400.0 };
        // `+ 0.0`: no `-0` in files.
        P2(p.0 + 0.0, p.1 + 0.0)
    }

    fn wall_of(&self, opening: impl Key) -> Option<&Wall> {
        match self.elements.get(opening) {
            Some(Element::Door(o) | Element::Window(o)) => self.wall(o.host),
            _ => None,
        }
    }

    /// What a tag shows: the field of its host it names (as a schedule shows it, in the project's units), `?` while that has no
    /// value (a door's mark before it has one).
    pub fn tag_text(&self, t: &Tag) -> String {
        let field = t.label.clone().or_else(|| self.elements.get(t.host).map(|e| Tag::usual(e.category()).to_string()));
        field.and_then(|f| crate::schedule::field_text(self, t.host, &f)).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "?".into())
    }

    /// The line `r` measures to, or why there is none. A straight wall's lines reach as far as its
    /// plan outline does, past the ends it shares; its ends, a curved wall's too, and a door's or a
    /// window's centre and jambs, run across it from face to face. Its centre is its centre line,
    /// wherever its location line is.
    pub fn reference_line(&self, r: &Reference) -> Result<Line, String> {
        match (self.elements.get(r.id), r.face) {
            (Some(Element::Grid(g)), Face::Centre) => Ok((g.start, g.end)),
            (Some(Element::Grid(_)), _) => Err(format!("grid {} has no faces; measure to its line", r.id)),
            (Some(Element::Wall(w)), face) => {
                let axis = walls::Axis::of(w);
                let (ext, int) = walls::faces(self, w);
                let (a, b) = match axis.radius() {
                    Some(_) if !matches!(face, Face::Start | Face::End) => {
                        return Err(format!("wall {}'s faces are arcs; measure to its start or its end, or its radius", r.id));
                    }
                    Some(_) => (0.0, axis.length()),
                    None => {
                        let along: Vec<f64> = walls::outline(self, self.elements.id(r.id).unwrap_or_default())
                            .iter()
                            .map(|p| (*p - w.start).dot(axis.tangent(0.0)))
                            .collect();
                        (along.iter().copied().fold(0.0, f64::min), along.iter().copied().fold(axis.length(), f64::max))
                    }
                };
                Ok(match face {
                    Face::Centre => (axis.at(a, (ext + int) / 2.0), axis.at(b, (ext + int) / 2.0)),
                    Face::Left => (axis.at(a, ext), axis.at(b, ext)),
                    Face::Right => (axis.at(a, int), axis.at(b, int)),
                    Face::Start => (axis.at(a, int), axis.at(a, ext)),
                    Face::End => (axis.at(b, int), axis.at(b, ext)),
                })
            }
            (Some(e @ (Element::Door(o) | Element::Window(o))), face) => {
                let what = format!("{} {}", e.category(), r.id);
                let (Some(w), Some(t)) = (self.wall(o.host), self.opening_type(e)) else {
                    return Err(format!("{what} has no wall or no type to measure"));
                };
                let along = match face {
                    Face::Centre => o.offset,
                    Face::Start => o.offset - t.width / 2.0,
                    Face::End => o.offset + t.width / 2.0,
                    Face::Left | Face::Right => {
                        return Err(format!("{what} has a centre and two jambs, start and end; measure to one of those"));
                    }
                };
                let (axis, (ext, int)) = (walls::Axis::of(w), walls::faces(self, w));
                Ok((axis.at(along, int), axis.at(along, ext)))
            }
            // A straight detail line: its line, or a short line square to it at either end.
            (Some(Element::DetailLine(l)), face) => {
                if l.curve.kind() != "line" {
                    return Err(format!(
                        "detail line {} is a {}; dimensions measure to straight ones (and radial ones to circles and arcs)",
                        r.id,
                        l.curve.kind()
                    ));
                }
                let (a, b) = (l.curve.start(), l.curve.end());
                let n = (b - a).unit().perp() * 100.0;
                match face {
                    Face::Centre => Ok((a, b)),
                    Face::Start => Ok((a - n, a + n)),
                    Face::End => Ok((b - n, b + n)),
                    Face::Left | Face::Right => Err(format!("detail line {} has a line and two ends; measure to one of those", r.id)),
                }
            }
            (Some(e), _) => Err(format!("{} {} isn't a wall, a door, a window, a grid or a detail line", e.category(), r.id)),
            (None, _) => Err(format!("element {} doesn't exist", r.id)),
        }
    }

    /// The lines dimensions measure to on element `id`: a wall's centre line, faces and ends, a
    /// door's or a window's centre and jambs, a grid's line.
    pub fn reference_lines(&self, id: impl Key) -> Vec<(Reference, Line)> {
        let id = self.guid(id);
        let faces: &[Face] = match self.elements.get(id) {
            Some(Element::Wall(_)) => &[Face::Centre, Face::Left, Face::Right, Face::Start, Face::End],
            Some(Element::Door(_) | Element::Window(_)) => &[Face::Centre, Face::Start, Face::End],
            Some(Element::Grid(_)) => &[Face::Centre],
            Some(Element::DetailLine(_)) => &[Face::Centre, Face::Start, Face::End],
            _ => &[],
        };
        faces
            .iter()
            .filter_map(|&face| {
                let r = Reference { id, face };
                Some((r, self.reference_line(&r).ok()?))
            })
            .collect()
    }

    /// Of element `id`'s reference lines that `keep` takes, the one nearest `p`, and how far it is.
    pub fn reference_at(&self, id: impl Key, p: P2, keep: impl Fn(Line) -> bool) -> Option<(Reference, f64)> {
        self.reference_lines(id)
            .into_iter()
            .filter(|(_, l)| keep(*l))
            .map(|(r, (a, b))| (r, geom::dist_to_segment(p, a, b)))
            .min_by(|x, y| x.1.total_cmp(&y.1))
    }

    /// How far a point standing on `level` at `at` is from the nearest wall centre line or grid east, north, west and south
    /// of it (a straight wall the level's plan cuts, as far as it runs; a grid as far as it is drawn): the temporary
    /// dimensions of a column or a component. A line it stands on is no distance away, so the next one counts.
    pub fn clearances(&self, level: impl Key, at: P2) -> [Option<Clearance>; 4] {
        let lines: Vec<(Reference, Line)> = self
            .elements
            .with_guids()
            .filter_map(|(_, id, e)| match e {
                Element::Grid(g) => Some((Reference { id, face: Face::Centre }, (g.start, g.end))),
                Element::Wall(w) if w.arc == 0.0 && self.cut_by(w, level) => {
                    let (axis, (ext, int)) = (walls::Axis::of(w), walls::faces(self, w));
                    let mid = (ext + int) / 2.0;
                    Some((Reference { id, face: Face::Centre }, (axis.at(0.0, mid), axis.at(axis.length(), mid))))
                }
                _ => None,
            })
            .collect();
        [P2(1.0, 0.0), P2(0.0, 1.0), P2(-1.0, 0.0), P2(0.0, -1.0)].map(|toward| {
            lines
                .iter()
                .filter_map(|&(to, (a, b))| {
                    let hit = geom::line_intersection(at, at + toward, a, b)?;
                    let distance = (hit - at).dot(toward);
                    (distance > TOL && geom::dist_to_segment(hit, a, b) <= 1e-6).then_some(Clearance { toward, distance, to })
                })
                .min_by(|x, y| x.distance.total_cmp(&y.distance))
        })
    }

    /// The stretch of its wall's location line a door or window stands in, as distances along it: from the nearest face,
    /// on either side of its middle, of a wall that meets its wall (crossing it, or joined to it from either side) on the
    /// plan of its wall's level, else from the wall's start and to its end. What its temporary dimensions measure to, as
    /// Revit's do to wall faces. None on a curved wall.
    pub fn opening_span(&self, o: &Opening) -> Option<(f64, f64)> {
        let host = self.wall(o.host).filter(|w| w.arc == 0.0)?;
        let axis = walls::Axis::of(host);
        let length = axis.length();
        if length <= TOL {
            return None;
        }
        let (a, b) = (axis.at(0.0, 0.0), axis.at(length, 0.0));
        let along = |p: P2| (p - a).dot(b - a) / length;
        // A wall joined to it ends at its location line or at one of its faces.
        let (ext, int) = walls::faces(self, host);
        let reach = ext.abs().max(int.abs()) + TOL;
        let hits: Vec<f64> = self
            .elements
            .with_guids()
            .filter_map(|(_, id, e)| match e {
                Element::Wall(w) if id != o.host && w.arc == 0.0 && self.cut_by(w, host.level) => Some(w),
                _ => None,
            })
            .flat_map(|w| {
                let (axis, (ext, int)) = (walls::Axis::of(w), walls::faces(self, w));
                [ext, int].map(|off| (axis.at(0.0, off), axis.at(axis.length(), off)))
            })
            .filter_map(|(p, q)| {
                let hit = geom::line_intersection(a, b, p, q)?;
                (geom::dist_to_segment(hit, p, q) <= reach).then(|| along(hit))
            })
            .collect();
        let start = hits.iter().copied().filter(|&t| t < o.offset).fold(0.0, f64::max);
        let end = hits.iter().copied().filter(|&t| t > o.offset).fold(length, f64::min);
        Some((start, end))
    }

    /// The offset halfway along opening `o`'s stretch of wall (`opening_span`), when `o` stands within `near` mm of it:
    /// where a door or window snaps as it's placed or dragged.
    pub fn centred(&self, o: &Opening, near: f64) -> Option<f64> {
        let (start, end) = self.opening_span(o)?;
        let middle = (start + end) / 2.0;
        ((o.offset - middle).abs() <= near).then_some(middle)
    }

    /// The circle a radial dimension measures on `r`: a round column's centre and radius, or a
    /// curved wall's, of its centre line or one of its faces.
    pub fn reference_arc(&self, r: &Reference) -> Result<(P2, f64), String> {
        let what = "a radial dimension measures round columns and curved walls";
        match self.elements.get(r.id) {
            Some(e) if e.column().is_some() => match (e.column(), self.column_type(e)) {
                (Some(c), Some(t)) if t.round => Ok((c.at, t.width / 2.0)),
                _ => Err(format!("{} {} isn't round; {what}", e.category(), r.id)),
            },
            Some(Element::Wall(w)) => {
                let (axis, (ext, int)) = (walls::Axis::of(w), walls::faces(self, w));
                let off = match r.face {
                    Face::Left => ext,
                    Face::Right => int,
                    _ => (ext + int) / 2.0,
                };
                let c = axis.centre().ok_or_else(|| format!("wall {} is straight; {what}", r.id))?;
                Ok((c, axis.at(0.0, off).dist(c)))
            }
            Some(Element::DetailLine(l)) => match &l.curve {
                crate::curves::Curve::Circle { centre, radius } => Ok((*centre, *radius)),
                c => c
                    .segments()
                    .first()
                    .filter(|_| c.kind() == "arc")
                    .and_then(|s| Some((s.centre()?, s.radius()?)))
                    .ok_or_else(|| format!("detail line {} is a {}; {what}, circles and arcs", r.id, c.kind())),
            },
            Some(e) => Err(format!("{} {} isn't round; {what}", e.category(), r.id)),
            None => Err(format!("element {} doesn't exist", r.id)),
        }
    }

    /// An angular dimension's arc: where its two lines meet, the angle (radians, counter-clockwise
    /// from east) of the ray it starts on, and how far it sweeps counter-clockwise to the other:
    /// the one of the four angles the lines make that holds `at`.
    pub fn angle_sector(&self, d: &Dimension) -> Option<(P2, f64, f64)> {
        let [r1, r2] = d.refs.as_slice() else { return None };
        let ((a, b), (c, e)) = (self.reference_line(r1).ok()?, self.reference_line(r2).ok()?);
        let o = geom::line_intersection(a, b, c, e)?;
        let at = d.at - o;
        let theta = at.1.atan2(at.0);
        let rays = |p: P2, q: P2| {
            let u = q - p;
            [u.1.atan2(u.0), (-u.1).atan2(-u.0)]
        };
        for s in rays(a, b) {
            for t in rays(c, e) {
                for (from, to) in [(s, t), (t, s)] {
                    let sweep = (to - from).rem_euclid(TAU);
                    if sweep < PI && (theta - from).rem_euclid(TAU) <= sweep {
                        return Some((o, from, sweep));
                    }
                }
            }
        }
        None
    }

    /// Which way a dimension's line runs, square to its first reference: eastward, or northward
    /// when it runs north and south. Then where its references cross it, in order that way: how
    /// far from `at`, and the reference's line.
    pub fn crossings(&self, d: &Dimension) -> (P2, Vec<(f64, Line)>) {
        let lines: Vec<Line> = d.refs.iter().filter_map(|r| self.reference_line(r).ok()).collect();
        let Some(&(a, b)) = lines.first() else { return (P2(0.0, 0.0), Vec::new()) };
        let n = (b - a).unit().perp();
        let n = if n.0 < -1e-9 || (n.0.abs() <= 1e-9 && n.1 < 0.0) { n * -1.0 } else { n };
        let mut v: Vec<(f64, Line)> = lines
            .into_iter()
            .map(|(a, b)| {
                let c = geom::line_intersection(d.at, d.at + n, a, b).unwrap_or(a);
                ((c - d.at).dot(n), (a, b))
            })
            .collect();
        v.sort_by(|x, y| x.0.total_cmp(&y.0));
        (n, v)
    }

    /// What a dimension shows: an aligned one's lengths, segment by segment (west to east, or
    /// south to north); an angular one's angle, in degrees; a radial one's radius.
    pub fn dimension_values(&self, d: &Dimension) -> Vec<f64> {
        match d.kind {
            DimKind::Aligned => self
                .crossings(d)
                .1
                .windows(2)
                .filter_map(|w| match w {
                    [x, y] => Some(y.0 - x.0),
                    _ => None,
                })
                .collect(),
            DimKind::Angular => self.angle_sector(d).map(|(_, _, sweep)| sweep.to_degrees()).into_iter().collect(),
            DimKind::Radial => d.refs.first().and_then(|r| self.reference_arc(r).ok()).map(|(_, r)| r).into_iter().collect(),
        }
    }

    /// What a spot elevation placed in the plan of `level` at `p` reads: the highest top there of
    /// what stands on the level.
    pub fn spot_host(&self, level: impl Key, p: P2) -> Option<Id> {
        // A floor plan looks down, under the ceilings.
        let on = |id: &Id| {
            self.elements
                .get(id)
                .is_some_and(|e| self.level_of(e).is_some_and(|l| self.same(l, level)) && !matches!(e, Element::Ceiling(_)))
        };
        crate::view3d::tops_at(self, p).into_iter().filter(|(id, _)| on(id)).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
    }

    /// What a spot elevation reads: in a plan, the top of its host at its point (none while its
    /// host isn't there); in a section or an elevation, its point's height.
    pub fn spot_value(&self, sp: &Spot) -> Option<f64> {
        // ponytail: builds every solid per spot, per drawing; share one set of solids if plans carry many spots.
        match self.elements.get(sp.view)? {
            Element::Level(_) | Element::PlanView(_) => crate::view3d::tops_at(self, sp.at).get(&self.elements.id(sp.host?)?).copied(),
            Element::Section(_) | Element::Elevation(_) => Some(sp.at.1),
            _ => None,
        }
    }

    /// Centre lines of the walls that bound the rooms of `level`, arcs as chords.
    pub fn bounding_lines(&self, level: impl Key) -> Vec<(P2, P2)> {
        self.elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::Wall(w) if self.bounding(w, level)))
            .flat_map(|(&id, _)| {
                let line = walls::centre_line(self, id);
                line.windows(2).filter_map(|s| s.first().zip(s.get(1)).map(|(a, b)| (*a, *b))).collect::<Vec<_>>()
            })
            .collect()
    }

    /// The faces of the walls that bound the rooms of `level`: the edges of their outlines.
    pub fn face_lines(&self, level: impl Key) -> Vec<(P2, P2)> {
        self.elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::Wall(w) if self.bounding(w, level)))
            .flat_map(|(&id, _)| geom::ring_edges(&walls::outline(self, id)).collect::<Vec<_>>())
            .collect()
    }

    /// A room's boundary, from the walls around its point. `None` when it isn't enclosed.
    /// Areas are measured at the wall centre lines (Revit's "At wall centerline" setting).
    pub fn room_region(&self, r: &Room) -> Option<Region> {
        geom::region_at(&self.bounding_lines(r.level), r.point, TOL)
    }

    /// What a detail line, a filled region or a text note must be, wherever it is: in a view or in a detail group.
    fn annotation_ok(&self, who: &str, e: &Element) -> Result<(), String> {
        match e {
            Element::Text(n) => {
                point(who, n.at)?;
                text(who, "text", &n.text)?;
                if n.text.trim().is_empty() {
                    return Err(format!("{who}: a text note needs text"));
                }
                if !(n.size > 0.0 && n.size <= 100.0) {
                    return Err(format!("{who}: size {} mm isn't above 0 and at most 100", n.size));
                }
                coord(&format!("{who} angle"), n.angle)?;
                if let Some(l) = n.leader {
                    point(&format!("{who} leader"), l)?;
                }
            }
            Element::DetailLine(l) => {
                l.check(who)?;
                if !self.line_styles.contains_key(&l.style) {
                    let there: Vec<&str> = self.line_styles.keys().map(String::as_str).collect();
                    return Err(format!("{who}: there's no line style `{}` (there are: {})", l.style, there.join(", ")));
                }
            }
            Element::FilledRegion(r) => {
                point(&format!("{who} pattern origin"), r.pattern_origin)?;
                coord(&format!("{who} pattern angle"), r.pattern_angle)?;
                crate::sketch::check(who, &r.sketch)?;
                if r.sketch.is_empty() {
                    return Err(format!("{who}: a region's sketch has no lines; it needs a loop of them (a circle is one)"));
                }
                if let Err(problems) = crate::sketch::solve(&r.sketch) {
                    let said = crate::sketch::say(&problems, &crate::sketch::namer(&r.sketch));
                    return Err(format!("{who}: its sketch doesn't close into loops: {said}"));
                }
                if !self.region_types.contains_key(&r.type_name) {
                    let there: Vec<&str> = self.region_types.keys().map(String::as_str).collect();
                    return Err(format!("{who}: there's no filled region type `{}` (there are: {})", r.type_name, there.join(", ")));
                }
                if let Some(st) = &r.line_style
                    && !self.line_styles.contains_key(st)
                {
                    return Err(format!("{who}: there's no line style `{st}`"));
                }
                if let Some((i, st)) = r
                    .sketch
                    .iter()
                    .enumerate()
                    .find_map(|(i, l)| l.style.as_ref().filter(|st| !self.line_styles.contains_key(*st)).map(|st| (i, st)))
                {
                    return Err(format!("{who}: boundary line {i} is drawn in `{st}`, and there's no such line style"));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Everything that must hold between commands: references resolve, geometry is finite and
    /// not degenerate, doors and windows fit their walls and don't overlap.
    pub fn check(&self) -> Result<(), String> {
        if let Some(lw) = &self.line_weights {
            lw.check()?;
        }
        for (c, style) in &self.object_styles {
            crate::styles::known(c)?;
            style.check(c)?;
        }
        for (n, p) in &self.line_patterns {
            if n.eq_ignore_ascii_case(lines::SOLID) {
                return Err(format!("line pattern `{n}`: {} is a line with no pattern; name it another way", lines::SOLID));
            }
            p.check(n)?;
        }
        for (n, s) in &self.line_styles {
            s.check(n, &self.line_patterns)?;
        }
        for (n, p) in &self.fill_patterns {
            p.check(n)?;
        }
        for (n, t) in &self.region_types {
            t.check(n, &self.fill_patterns)?;
        }
        for (n, x) in &self.materials {
            for p in [&x.cut_pattern, &x.surface_pattern].into_iter().flatten() {
                if !self.fill_patterns.contains_key(p) {
                    return Err(format!("material `{n}`: there's no fill pattern `{p}`"));
                }
            }
        }
        for (n, g) in &self.detail_groups {
            text(&format!("detail group `{n}`"), "name", n)?;
            if g.members.is_empty() || g.members.len() > crate::groups::MAX_MEMBERS {
                return Err(format!(
                    "detail group `{n}`: it holds {} members; it holds 1 to {}",
                    g.members.len(),
                    crate::groups::MAX_MEMBERS
                ));
            }
            for (i, m) in g.members.iter().enumerate() {
                let who = format!("detail group `{n}` member {i}");
                if !crate::groups::holds(&m.element) {
                    return Err(format!(
                        "{who}: a {} can't be in a detail group; it holds detail lines, filled regions and text",
                        m.element.category()
                    ));
                }
                self.annotation_ok(&who, &m.element)?;
            }
        }
        for (cat, map) in [("door", &self.types.door), ("window", &self.types.window)] {
            for (n, t) in map {
                size(&format!("{cat} type `{n}` width"), t.width)?;
                size(&format!("{cat} type `{n}` height"), t.height)?;
                coord(&format!("{cat} type `{n}` sill"), t.sill)?;
            }
        }
        if self.images.len() > MAX_IMAGES {
            return Err(format!("the project has {} pictures; it holds up to {MAX_IMAGES}", self.images.len()));
        }
        for id in self.images.keys() {
            text(&format!("picture `{id}`"), "id", id)?;
        }
        if self.clouds.len() > crate::cloud::MAX_CLOUDS {
            return Err(format!("the project has {} scans; it holds up to {}", self.clouds.len(), crate::cloud::MAX_CLOUDS));
        }
        let held: usize = self.clouds.values().map(crate::cloud::Cloud::len).sum();
        if held > crate::cloud::MAX_CLOUD_POINTS {
            return Err(format!("the project's scans hold {held} points; it holds up to {}", crate::cloud::MAX_CLOUD_POINTS));
        }
        for name in self.clouds.keys() {
            text(&format!("scan `{name}`"), "name", name)?;
        }
        for (n, x) in &self.materials {
            text(&format!("material `{n}`"), "name", n)?;
            if n.trim().is_empty() {
                return Err("a material needs a name".into());
            }
            if let Some(t) = &x.texture {
                t.check(&self.images).map_err(|e| format!("material `{n}`: texture: {e}"))?;
            }
        }
        for (n, f) in &self.families {
            text(&format!("family `{n}`"), "name", n)?;
            if n.trim().is_empty() {
                return Err("a family needs a name".into());
            }
            f.check(&|m| self.materials.contains_key(m)).map_err(|e| format!("family `{n}`: {e}"))?;
        }
        if self.parameters.len() > MAX_PROJECT_PARAMETERS {
            return Err(format!("the project has {} parameters; it holds up to {MAX_PROJECT_PARAMETERS}", self.parameters.len()));
        }
        for (n, p) in &self.parameters {
            let who = format!("parameter `{n}`");
            if !crate::family::nameable(n) {
                return Err(format!("{who}: a name starts with a letter and holds letters, digits, `_` and spaces, up to {MAX_TEXT}"));
            }
            if crate::paint::reserved(n) || phasing::reserved(n) || crate::massing::reserved(n) {
                return Err(format!(
                    "{who}: elements hold their phase, their design option, their paint and their mass under that name; name it otherwise"
                ));
            }
            if p.categories.is_empty() {
                return Err(format!("{who}: it needs a category: one of {}", PARAMETER_CATEGORIES.join(", ")));
            }
            if let Some(c) = p.categories.iter().find(|c| !PARAMETER_CATEGORIES.contains(&c.as_str())) {
                return Err(format!("{who}: `{c}` isn't a category it can be for; there are {}", PARAMETER_CATEGORIES.join(", ")));
            }
            if let Some(c) = p.categories.iter().find(|c| crate::schedule::own_field(c, n)) {
                return Err(format!("{who}: {c}s have a `{n}` of their own; name it otherwise"));
            }
            if p.categories.iter().any(|c| c == "component")
                && let Some((f, _)) = self.families.iter().find(|(_, f)| f.parameters.iter().any(|q| q.name == *n))
            {
                return Err(format!("{who}: family `{f}` has a parameter of that name; name it otherwise"));
            }
        }
        graphics::check_project(self)?;
        phasing::check_project(self)?;
        for (cat, n, thickness, layers) in self.layered_types() {
            let who = format!("{cat} type `{n}`");
            size(&format!("{who} thickness"), thickness)?;
            if layers.len() > MAX_LAYERS {
                return Err(format!("{who}: more than {MAX_LAYERS} layers"));
            }
            for (i, l) in layers.iter().enumerate() {
                let what = format!("{who} layer {}", i + 1);
                text(&what, "material", &l.material)?;
                if !l.material.is_empty() && !self.materials.contains_key(&l.material) {
                    return Err(format!("{what}: material `{}` isn't in the library; add it with `material`", l.material));
                }
                match l.function {
                    Function::Membrane if l.thickness == 0.0 => {}
                    _ => size(&format!("{what} thickness"), l.thickness)?,
                }
            }
            let sum: f64 = layers.iter().map(|l| l.thickness).sum();
            if !layers.is_empty() && (sum - thickness).abs() > 1e-6 {
                return Err(format!("{who}: its layers add up to {sum} mm, not its thickness, {thickness} mm"));
            }
        }
        for (cat, types) in
            [("column", &self.types.column), ("structural column", &self.types.structural_column), ("footing", &self.types.footing)]
        {
            for (n, t) in types {
                size(&format!("{cat} type `{n}` width"), t.width)?;
                size(&format!("{cat} type `{n}` depth"), t.depth)?;
            }
        }
        for (cat, types) in &self.types.runs {
            if !RUN_CATEGORIES.contains(&cat.as_str()) {
                return Err(format!("`{cat}` has no run types; there are {}", RUN_CATEGORIES.join(", ")));
            }
            for (n, t) in types {
                let who = format!("{cat} type `{n}`");
                size(&format!("{who} width"), t.width)?;
                size(&format!("{who} depth"), t.depth)?;
                if t.round && t.depth != t.width {
                    return Err(format!("{who}: a round section is as deep as it is wide ({} mm), not {} mm", t.width, t.depth));
                }
                text(&who, "material", &t.material)?;
                if !t.material.is_empty() && !self.materials.contains_key(&t.material) {
                    return Err(format!("{who}: material `{}` isn't in the library; add it with `material`", t.material));
                }
            }
        }
        for (n, t) in &self.types.railing {
            size(&format!("railing type `{n}` height"), t.height)?;
            if !(t.spacing == 0.0 || (50.0..=MAX_COORD).contains(&t.spacing)) {
                return Err(format!("railing type `{n}`: spacing {} mm; balusters stand 50 mm apart or more, or 0 for none", t.spacing));
            }
        }
        for (n, t) in &self.types.ceiling {
            // A finer grid would draw a line every few pixels, and millions over a large ceiling.
            if !(t.grid == 0.0 || (100.0..=MAX_COORD).contains(&t.grid)) {
                return Err(format!("ceiling type `{n}`: grid {} mm; its tiles are 100 mm or wider, or 0 for none", t.grid));
            }
        }
        let mut level_names = BTreeSet::new();
        let mut grid_names = BTreeSet::new();
        let mut link_names = BTreeSet::new();
        let mut view_names = BTreeSet::new();
        // The schedules every project has that the project has made its own: one each.
        let mut twins = BTreeSet::new();
        let mut sheet_numbers = BTreeSet::new();
        // Each view on a sheet: the sheet.
        let mut placed: BTreeMap<Guid, Guid> = BTreeMap::new();
        // Openings by host wall, in the order they start along it.
        let mut spans: BTreeMap<Guid, Vec<OpeningSpan>> = BTreeMap::new();
        // The members each view's detail groups and repeating details place.
        let mut members_placed: BTreeMap<Guid, usize> = BTreeMap::new();
        for (&id, e) in &self.elements {
            let who = format!("{} {id}", e.category());
            let level_ok = |l: Guid| self.level(l).map(|_| ()).ok_or(format!("{who}: level {} doesn't exist", self.shown(l)));
            // A view annotation is drawn in: a plan, a section or an elevation, or with `sheets`, a sheet.
            let view_ok = |v: Guid, sheets: bool| match self.elements.get(v) {
                Some(Element::Level(_) | Element::PlanView(_) | Element::Section(_) | Element::Elevation(_)) => Ok(()),
                Some(Element::Sheet(_)) if sheets => Ok(()),
                Some(other) => Err(format!(
                    "{who}: {} {} isn't a plan, a section, an elevation{}",
                    other.category(),
                    self.shown(v),
                    if sheets { " or a sheet" } else { "" }
                )),
                None => Err(format!("{who}: view {v} doesn't exist")),
            };
            // Its values of the project's parameters, each one of its category's, of its kind; a
            // component's family's are its family's to check.
            for (k, v) in e.params().into_iter().flatten() {
                // Its phases and its design option are values it holds under names no parameter may take.
                if let Some(checked) = phasing::check_value(self, &who, k, v) {
                    checked?;
                    continue;
                }
                if let Some(checked) = crate::paint::check_value(self, &who, k, v, e) {
                    checked?;
                    continue;
                }
                if let Some(checked) = crate::massing::check_value(self, &who, k, v, e) {
                    checked?;
                    continue;
                }
                if e.family().and_then(|(f, _)| self.families.get(f)).is_some_and(|f| f.parameters.iter().any(|p| p.name == *k)) {
                    continue;
                }
                let p = self
                    .parameters
                    .get(k)
                    .filter(|p| p.categories.iter().any(|c| c == e.category()))
                    .ok_or_else(|| format!("{who}: `{k}` isn't a parameter of {}s; add it with `parameter`", e.category()))?;
                if !p.holds(v) {
                    let what = match p.kind {
                        ParameterKind::Text => "text",
                        ParameterKind::Number => "a number",
                        ParameterKind::Length => "a length",
                        ParameterKind::YesNo => "yes or no",
                    };
                    return Err(format!("{who}: `{k}` holds {what}, not {v}"));
                }
            }
            phasing::check_order(self, &who, e)?;
            match e {
                Element::Level(l) => {
                    text(&who, "name", &l.name)?;
                    coord(&format!("{who} elevation"), l.elevation)?;
                    if !level_names.insert(l.name.as_str()) {
                        return Err(format!("{who}: another level is already named `{}`", l.name));
                    }
                    range_ok(&who, &l.view_range)?;
                    if let Some([a, b]) = l.crop {
                        crop(&who, a, b)?;
                    }
                    if let Some([a, b]) = l.extent {
                        point(&format!("{who} extent"), a)?;
                        point(&format!("{who} extent"), b)?;
                        if !(a.0 < b.0 - TOL && a.1 < b.1 - TOL) {
                            return Err(format!("{who}: its extent runs from its south-west corner to its north-east one"));
                        }
                    }
                    l.graphics.check(self).map_err(|e| format!("{who}: {e}"))?;
                }
                Element::Grid(g) => {
                    text(&who, "name", &g.name)?;
                    line(&who, g.start, g.end)?;
                    if !grid_names.insert(g.name.as_str()) {
                        return Err(format!("{who}: another grid is already named `{}`", g.name));
                    }
                }
                Element::Wall(w) => {
                    known(&who, &w.type_name, &self.types.wall)?;
                    level_ok(w.level)?;
                    line(&who, w.start, w.end)?;
                    coord(&format!("{who} base_offset"), w.base_offset)?;
                    // An arc within reach: all of its circle inside the model's limits.
                    let axis = walls::Axis::of(w);
                    let reach = axis.centre().zip(axis.radius()).is_none_or(|(c, r)| c.0.abs().max(c.1.abs()) + r <= MAX_COORD);
                    if !(w.arc.is_finite() && w.arc.abs() < 360.0 && reach) {
                        return Err(format!("{who}: arc {}° isn't between -360° and 360°, reaching no farther than 1000 km", w.arc));
                    }
                    for (what, at) in [("attach_top", w.attach_top), ("attach_base", w.attach_base)] {
                        if let Some(a) = at
                            && !matches!(self.elements.get(a), Some(Element::Roof(_) | Element::Floor(_)))
                        {
                            return Err(format!("{who}: {what} {} isn't a roof or a floor", self.shown(a)));
                        }
                    }
                    match w.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), w.height)?,
                    }
                    if self.top_z(w) <= self.base_z(w) + TOL {
                        return Err(format!("{who}: its top must be above its base"));
                    }
                }
                Element::Floor(f) => {
                    point(&format!("{who} pattern origin"), f.pattern_origin)?;
                    coord(&format!("{who} pattern angle"), f.pattern_angle)?;
                    known(&who, &f.type_name, &self.types.floor)?;
                    level_ok(f.level)?;
                    boundary(&who, &f.boundary)?;
                    openings(&who, &f.boundary, &f.openings)?;
                    crate::sketch::check(&who, e.sketch().map_or(&[][..], Vec::as_slice))?;
                    coord(&format!("{who} offset"), f.offset)?;
                    if !(f.slope.is_finite() && f.slope.abs() <= MAX_SLOPE) {
                        return Err(format!("{who}: slope {}° isn't between -{MAX_SLOPE}° and {MAX_SLOPE}°", f.slope));
                    }
                    match f.arrow {
                        Some([tail, head]) => line(&format!("{who} arrow"), tail, head)?,
                        None if f.slope != 0.0 => {
                            return Err(format!(
                                "{who}: a sloped floor needs a slope arrow, from where its top is at its offset toward where it rises"
                            ));
                        }
                        None => {}
                    }
                }
                Element::Ceiling(c) => {
                    known(&who, &c.type_name, &self.types.ceiling)?;
                    level_ok(c.level)?;
                    boundary(&who, &c.boundary)?;
                    openings(&who, &c.boundary, &c.openings)?;
                    crate::sketch::check(&who, e.sketch().map_or(&[][..], Vec::as_slice))?;
                    coord(&format!("{who} offset"), c.offset)?;
                }
                Element::Shaft(sh) => {
                    level_ok(sh.level)?;
                    boundary(&who, &sh.boundary)?;
                    coord(&format!("{who} base_offset"), sh.base_offset)?;
                    crate::sketch::check(&who, e.sketch().map_or(&[][..], Vec::as_slice))?;
                    match sh.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), sh.height)?,
                    }
                    let (bottom, top) = self.span(sh.level, sh.base_offset, sh.top_level, sh.height);
                    if top <= bottom + TOL {
                        return Err(format!("{who}: its top must be above its base"));
                    }
                }
                Element::Mass(ms) => {
                    level_ok(ms.level)?;
                    boundary(&who, &ms.boundary)?;
                    text(&who, "name", &ms.name)?;
                    coord(&format!("{who} base_offset"), ms.base_offset)?;
                    match ms.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), ms.height)?,
                    }
                    let (bottom, top) = self.span(ms.level, ms.base_offset, ms.top_level, ms.height);
                    if top <= bottom + TOL {
                        return Err(format!("{who}: its top must be above its base"));
                    }
                    if let Some(crown) = &ms.top {
                        // Its points first: past that, the rings pair up and the cross sections can be worked out.
                        if crown.len() == ms.boundary.len() {
                            boundary(&format!("{who} top"), crown)?;
                        }
                        if let Some(why) = crate::massing::loft_problem(&ms.boundary, crown) {
                            return Err(format!("{who}: {why}"));
                        }
                    }
                }
                Element::Door(o) | Element::Window(o) => {
                    let w = self.wall(o.host).ok_or(format!("{who}: host wall {} doesn't exist", self.shown(o.host)))?;
                    let t = self.opening_dims(e).map_err(|e| format!("{who}: {e}"))?;
                    if let Some(f) = o.family.as_ref().and_then(|n| self.families.get(n)) {
                        self.family_values_ok(&who, o.family.as_deref().unwrap_or_default(), f, &o.params)?;
                        let values = f.values_in(&o.type_name, &o.params, self.thickness(w)).map_err(|e| format!("{who}: {e}"))?;
                        f.pieces(&values).and_then(|_| f.strokes(&values)).map_err(|e| format!("{who}: {e}"))?;
                        size(&format!("{who} width"), t.width)?;
                        size(&format!("{who} height"), t.height)?;
                    }
                    coord(&format!("{who} offset"), o.offset)?;
                    coord(&format!("{who} sill"), o.sill)?;
                    text(&who, "mark", &o.mark)?;
                    let (a, b) = (o.offset - t.width / 2.0, o.offset + t.width / 2.0);
                    let length = crate::walls::Axis::of(w).length();
                    if a < -TOL || b > length + TOL {
                        return Err(format!(
                            "{who}: {} mm wide at offset {} doesn't fit wall {}, {length} mm long",
                            t.width,
                            o.offset,
                            self.shown(o.host)
                        ));
                    }
                    // Up to the wall's top over it, which a roof it's attached to may raise.
                    let top = [a, o.offset, b].map(|s| crate::walls::top_at(self, w, s)).into_iter().fold(f64::INFINITY, f64::min);
                    if o.sill < 0.0 || self.own_base(w) + o.sill + t.height > top + TOL {
                        return Err(format!(
                            "{who}: sill {} + height {} doesn't fit wall {}'s height",
                            o.sill,
                            t.height,
                            self.shown(o.host)
                        ));
                    }
                    let span = OpeningSpan { start: a, end: b, sill: o.sill, head: o.sill + t.height, id };
                    let list = spans.entry(o.host).or_default();
                    let at = list.partition_point(|held| held.start <= a);
                    list.insert(at, span);
                }
                Element::Room(r) => {
                    level_ok(r.level)?;
                    point(&who, r.point)?;
                    text(&who, "name", &r.name)?;
                    text(&who, "number", &r.number)?;
                }
                Element::Dimension(d) => {
                    level_ok(d.level)?;
                    if let Some(v) = d.view {
                        self.plan_view_of(v, d.level)
                            .ok_or_else(|| format!("{who}: view {} isn't a plan view of its level", self.shown(v)))?;
                    }
                    point(&who, d.at)?;
                    match (d.kind, d.refs.len()) {
                        (DimKind::Aligned, 2..=MAX_POINTS) | (DimKind::Angular, 2) | (DimKind::Radial, 1) => {}
                        (DimKind::Aligned, _) => return Err(format!("{who}: a dimension measures between 2 to {MAX_POINTS} references")),
                        (DimKind::Angular, _) => return Err(format!("{who}: an angular dimension measures between two lines")),
                        (DimKind::Radial, _) => return Err(format!("{who}: a radial dimension measures one round element")),
                    }
                    let mut seen = BTreeSet::new();
                    let mut first: Option<(Reference, Line)> = None;
                    for r in &d.refs {
                        if d.kind == DimKind::Radial {
                            self.reference_arc(r).map_err(|e| format!("{who}: {e}"))?;
                            continue;
                        }
                        let line = self.reference_line(r).map_err(|e| format!("{who}: {e}"))?;
                        let what = format!("{} {}", self.cat(r.id), self.shown(r.id));
                        if !seen.insert((r.id, r.face)) {
                            return Err(format!("{who}: it measures to {what}'s {} twice", r.face.name()));
                        }
                        match first {
                            None => first = Some((*r, line)),
                            Some((f, (a, b))) => {
                                let parallel = geom::parallel(a, b, line.0, line.1);
                                let other = format!("{} {}", self.cat(f.id), self.shown(f.id));
                                if d.kind == DimKind::Aligned && !parallel {
                                    return Err(format!(
                                        "{who}: {what} isn't parallel to {other}; a dimension measures between parallel walls and grids"
                                    ));
                                }
                                if d.kind == DimKind::Angular && parallel {
                                    return Err(format!("{who}: {what} is parallel to {other}; an angle is between lines that meet"));
                                }
                            }
                        }
                    }
                }
                Element::Text(Note { view, .. })
                | Element::DetailLine(DetailLine { view, .. })
                | Element::FilledRegion(FilledRegion { view, .. }) => {
                    view_ok(*view, true)?;
                    self.annotation_ok(&who, e)?;
                }
                Element::DetailGroup(g) => {
                    view_ok(g.view, true)?;
                    point(&who, g.at)?;
                    coord(&format!("{who} angle"), g.angle)?;
                    let Some(t) = self.detail_groups.get(&g.type_name) else {
                        let there: Vec<&str> = self.detail_groups.keys().map(String::as_str).collect();
                        return Err(format!("{who}: there's no detail group `{}` (there are: {})", g.type_name, there.join(", ")));
                    };
                    placing(&who, &mut members_placed, g.view, t.members.len())?;
                }
                Element::RepeatingDetail(r) => {
                    view_ok(r.view, true)?;
                    point(&who, r.start)?;
                    point(&who, r.end)?;
                    if !(r.spacing.is_finite() && r.spacing >= 1.0 && r.spacing <= MAX_COORD) {
                        return Err(format!("{who}: spacing {} mm isn't from 1 mm to {MAX_COORD}", r.spacing));
                    }
                    if r.places().is_none() {
                        return Err(format!(
                            "{who}: it would repeat more than {} times; space it wider or draw it shorter",
                            crate::groups::MAX_REPEATS
                        ));
                    }
                    let Some(t) = self.detail_groups.get(&r.type_name) else {
                        let there: Vec<&str> = self.detail_groups.keys().map(String::as_str).collect();
                        return Err(format!("{who}: there's no detail group `{}` (there are: {})", r.type_name, there.join(", ")));
                    };
                    placing(&who, &mut members_placed, r.view, t.members.len().saturating_mul(r.count()))?;
                }
                Element::PointCloud(pc) => {
                    if !self.clouds.contains_key(&pc.cloud) {
                        return Err(format!("{who}: the project holds no scan named `{}`", pc.cloud));
                    }
                    level_ok(pc.level)?;
                    point(&format!("{who} at"), pc.at)?;
                    coord(&format!("{who} offset"), pc.offset)?;
                    coord(&format!("{who} angle"), pc.angle)?;
                }
                Element::Spot(sp) => {
                    view_ok(sp.view, false)?;
                    point(&who, sp.at)?;
                    match (self.plan_level(sp.view), sp.host) {
                        (Some(_), None) => return Err(format!("{who}: a spot elevation in a plan reads an element's top; give `host`")),
                        (_, Some(h)) if !self.elements.contains_key(h) => return Err(format!("{who}: element {h} doesn't exist")),
                        _ => {}
                    }
                }
                Element::Schedule(sc) => {
                    text(&who, "name", &sc.name)?;
                    if sc.name.trim().is_empty() {
                        return Err(format!("{who}: a view needs a name"));
                    }
                    crate::schedule::check(self, sc).map_err(|e| format!("{who} (`{}`): {e}", sc.name))?;
                    if let Some((of, name)) = crate::schedule::builtin(Some(&sc.name), None)
                        && !twins.insert(of)
                    {
                        return Err(format!(
                            "{who} (`{}`): another schedule is already the {of}s' {name}; call it something else",
                            sc.name
                        ));
                    }
                    if !view_names.insert(sc.name.as_str()) {
                        return Err(format!("{who}: another view is already named `{}`", sc.name));
                    }
                }
                Element::PlanView(v) => {
                    text(&who, "name", &v.name)?;
                    if v.name.trim().is_empty() {
                        return Err(format!("{who}: a view needs a name"));
                    }
                    level_ok(v.level)?;
                    range_ok(&who, &v.view_range)?;
                    if let Some([a, b]) = v.crop {
                        crop(&who, a, b)?;
                    }
                    v.graphics.check(self).map_err(|e| format!("{who}: {e}"))?;
                    // A level's name is its own plan's; a view of it takes another.
                    if self.levels().iter().any(|(_, l)| l.name == v.name) {
                        return Err(format!("{who}: a level is already named `{}`; a plan view of it needs another name", v.name));
                    }
                    if let Some(p) = v.parent {
                        // Of a level's own plan, or of a plan view that is no dependent itself, on the same level.
                        let primary = match self.elements.get(p) {
                            Some(Element::Level(_)) => p == v.level,
                            Some(Element::PlanView(o)) => o.parent.is_none() && o.level == v.level,
                            _ => false,
                        };
                        if !primary {
                            return Err(format!(
                                "{who}: a dependent view hangs from a plan of its own level that is no dependent itself, not {}",
                                self.shown(p)
                            ));
                        }
                    }
                    if !view_names.insert(v.name.as_str()) {
                        return Err(format!("{who}: another view is already named `{}`", v.name));
                    }
                }
                Element::Link(l) => {
                    text(&who, "name", &l.name)?;
                    if l.name.trim().is_empty() || l.path.trim().is_empty() || l.path.chars().count() > MAX_TEXT {
                        return Err(format!("{who}: a link needs a name and the path of a file, up to {MAX_TEXT} characters"));
                    }
                    point(&format!("{who} at"), l.at)?;
                    if !(l.angle.is_finite() && l.angle.abs() <= 360.0) {
                        return Err(format!("{who}: its angle {}° isn't between -360° and 360°", l.angle));
                    }
                    if !link_names.insert(l.name.as_str()) {
                        return Err(format!("{who}: another link is already named `{}`", l.name));
                    }
                }
                Element::View3D(v) => {
                    text(&who, "name", &v.name)?;
                    if v.name.trim().is_empty() {
                        return Err(format!("{who}: a view needs a name"));
                    }
                    coord(&format!("{who} azimuth"), v.azimuth)?;
                    if !(-89.0..=89.0).contains(&v.elevation) {
                        return Err(format!("{who}: elevation {}° isn't between -89° and 89°", v.elevation));
                    }
                    v.graphics.check(self).map_err(|e| format!("{who}: {e}"))?;
                    if let Some(b) = &v.section_box {
                        section_box_ok(&who, b)?;
                    }
                    if !view_names.insert(v.name.as_str()) {
                        return Err(format!("{who}: another view is already named `{}`", v.name));
                    }
                }
                Element::Section(v) | Element::Elevation(v) => {
                    text(&who, "name", &v.name)?;
                    if v.name.trim().is_empty() {
                        return Err(format!("{who}: a view needs a name"));
                    }
                    line(&who, v.start, v.end)?;
                    if let Some(d) = v.depth {
                        size(&format!("{who} depth"), d)?;
                    }
                    if let Some([bottom, top]) = v.crop {
                        coord(&format!("{who} crop"), bottom)?;
                        coord(&format!("{who} crop"), top)?;
                        if bottom >= top - TOL {
                            return Err(format!("{who}: its crop runs from a bottom up to a top above it, not from {bottom} to {top}"));
                        }
                    }
                    v.graphics.check(self).map_err(|e| format!("{who}: {e}"))?;
                    if !view_names.insert(v.name.as_str()) {
                        return Err(format!("{who}: another view is already named `{}`", v.name));
                    }
                }
                Element::Component(c) => {
                    let f = self.families.get(&c.family).ok_or_else(|| {
                        let names: Vec<&str> = self.families.keys().map(String::as_str).collect();
                        format!("{who}: family `{}` doesn't exist (there are: {})", c.family, names.join(", "))
                    })?;
                    if f.category.hosted() {
                        return Err(format!(
                            "{who}: family `{}` is a door's or a window's: it stands in a wall, placed with `door` or `window`",
                            c.family
                        ));
                    }
                    level_ok(c.level)?;
                    point(&who, c.at)?;
                    coord(&format!("{who} angle"), c.angle)?;
                    coord(&format!("{who} offset"), c.offset)?;
                    self.family_values_ok(&who, &c.family, f, &c.params)?;
                    f.values(&c.type_name, &c.params).and_then(|v| f.pieces(&v)).map_err(|e| format!("{who}: {e}"))?;
                }
                Element::Column(col) | Element::StructuralColumn(col) | Element::Footing(col) => {
                    let types = match e {
                        Element::StructuralColumn(_) => &self.types.structural_column,
                        Element::Footing(_) => &self.types.footing,
                        _ => &self.types.column,
                    };
                    known(&who, &col.type_name, types)?;
                    level_ok(col.level)?;
                    point(&who, col.at)?;
                    coord(&format!("{who} angle"), col.angle)?;
                    coord(&format!("{who} base_offset"), col.base_offset)?;
                    match col.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), col.height)?,
                    }
                    let (bottom, top) = self.span(col.level, col.base_offset, col.top_level, col.height);
                    if top <= bottom + TOL {
                        return Err(format!("{who}: its top must be above its base"));
                    }
                }
                run_arm!(r) => {
                    let (category, none) = (e.category(), BTreeMap::new());
                    let t = known(&who, &r.type_name, self.types.runs.get(category).unwrap_or(&none))?;
                    level_ok(r.level)?;
                    point(&format!("{who} start"), r.start)?;
                    point(&format!("{who} end"), r.end)?;
                    coord(&format!("{who} offset"), r.offset)?;
                    if let Some(o) = r.end_offset {
                        coord(&format!("{who} end_offset"), o)?;
                    }
                    text(&who, "system", &r.system)?;
                    crate::runs::check(&who, category, &crate::runs::Geom::of(self, r, t))?;
                }
                Element::Stair(st) => {
                    level_ok(st.level)?;
                    point(&who, st.start)?;
                    coord(&format!("{who} angle"), st.angle)?;
                    match st.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), st.height)?,
                    }
                    for (what, v) in [("width", st.width), ("riser", st.riser), ("tread", st.tread)] {
                        size(&format!("{who} {what}"), v)?;
                    }
                    let (n, _, _) = self.flight(st);
                    if !(2..=200).contains(&n) {
                        return Err(format!(
                            "{who}: {n} risers; a stair takes 2 to 200, so check its height and its riser ({} mm)",
                            st.riser
                        ));
                    }
                    landings(&who, &st.landings, Some(f64::from(n)))?;
                }
                Element::Ramp(r) => {
                    level_ok(r.level)?;
                    point(&who, r.start)?;
                    coord(&format!("{who} angle"), r.angle)?;
                    match r.top_level {
                        Some(t) => level_ok(t)?,
                        None => size(&format!("{who} height"), r.height)?,
                    }
                    size(&format!("{who} width"), r.width)?;
                    if !(1.0..=50.0).contains(&r.slope) {
                        return Err(format!("{who}: slope {}% isn't from 1% to 50%", r.slope));
                    }
                    let (bottom, top) = self.span(r.level, 0.0, r.top_level, r.height);
                    if top <= bottom + TOL {
                        return Err(format!("{who}: its top must be above its base"));
                    }
                    coord(&format!("{who} length"), (top - bottom) / r.slope * 100.0)?;
                    landings(&who, &r.landings, None)?;
                    if let Some(l) = r.landings.last().filter(|l| l.at >= top - bottom - TOL) {
                        return Err(format!("{who}: a landing {} mm up isn't below its top, {} mm up", l.at, top - bottom));
                    }
                }
                Element::Railing(rl) => {
                    let t = known(&who, &rl.type_name, &self.types.railing)?;
                    level_ok(rl.level)?;
                    coord(&format!("{who} base_offset"), rl.base_offset)?;
                    match (rl.host, rl.side) {
                        (Some(h), Some(_)) => {
                            if !matches!(self.elements.get(h), Some(Element::Stair(_) | Element::Ramp(_))) {
                                return Err(format!("{who}: host {} isn't a stair or a ramp", self.shown(h)));
                            }
                        }
                        (Some(_), None) => {
                            return Err(format!("{who}: a railing on a stair or a ramp runs along its left or its right side"));
                        }
                        (None, _) => {
                            if rl.path.len() < 2 || rl.path.len() > MAX_POINTS {
                                return Err(format!("{who}: a path needs 2 to {MAX_POINTS} points"));
                            }
                            for p in &rl.path {
                                point(&who, *p)?;
                            }
                        }
                    }
                    let line = crate::stairs::railing(self, rl);
                    let length: f64 = line.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| a.0.dist(b.0))).sum();
                    if length <= TOL {
                        return Err(format!("{who}: its path has no length"));
                    }
                    if t.spacing > 0.0 && length / t.spacing > MAX_BALUSTERS {
                        return Err(format!("{who}: {:.0} balusters, more than {MAX_BALUSTERS}; space them wider", length / t.spacing));
                    }
                }
                Element::Roof(r) => {
                    known(&who, &r.type_name, &self.types.roof)?;
                    level_ok(r.level)?;
                    coord(&format!("{who} overhang"), r.overhang)?;
                    coord(&format!("{who} offset"), r.offset)?;
                    match &r.extrusion {
                        Some(x) => {
                            line(&who, x.line[0], x.line[1])?;
                            if !(2..=MAX_POINTS).contains(&x.profile.len()) {
                                return Err(format!("{who}: an extrusion's profile needs 2 to {MAX_POINTS} points"));
                            }
                            for p in &x.profile {
                                point(&who, *p)?;
                            }
                            if x.profile.windows(2).any(|w| w.first().zip(w.get(1)).is_none_or(|(a, b)| b.0 - a.0 <= TOL)) {
                                return Err(format!("{who}: its profile runs along its line one way, each point past the one before"));
                            }
                            coord(&format!("{who} from"), x.from)?;
                            coord(&format!("{who} to"), x.to)?;
                            if x.to - x.from <= TOL {
                                return Err(format!("{who}: it's swept across its line from `from` to `to`, further to the left"));
                            }
                        }
                        None => {
                            boundary(&who, &r.boundary)?;
                            if !(0.0..=MAX_SLOPE).contains(&r.slope) {
                                return Err(format!("{who}: slope {}° isn't from 0° (flat) to {MAX_SLOPE}°", r.slope));
                            }
                        }
                    }
                    if r.openings.len() > MAX_ROOF_POINTS {
                        return Err(format!("{who}: {} openings, more than {MAX_ROOF_POINTS}", r.openings.len()));
                    }
                    for o in &r.openings {
                        boundary(&format!("{who} opening"), o)?;
                    }
                    if r.extrusion.is_some() && !r.courtyards.is_empty() {
                        return Err(format!("{who}: courtyards are holes in a roof's footprint; cut an extrusion roof with `openings`"));
                    }
                    if r.courtyards.iter().map(Vec::len).sum::<usize>() > MAX_ROOF_POINTS {
                        return Err(format!("{who}: its courtyards have more than {MAX_ROOF_POINTS} points between them"));
                    }
                    for (i, c) in r.courtyards.iter().enumerate() {
                        boundary(&format!("{who} courtyard {i}"), c)?;
                    }
                    holes(&who, "courtyard", &r.boundary, &r.courtyards)?;
                    crate::sketch::check(&who, &r.sketch)?;
                    self.roof_pieces(r).map_err(|e| format!("{who}: {e}"))?;
                }
                Element::Sheet(sh) => {
                    text(&who, "number", &sh.number)?;
                    text(&who, "name", &sh.name)?;
                    if sh.number.trim().is_empty() {
                        return Err(format!("{who}: a sheet needs a number"));
                    }
                    if !sheet_numbers.insert(sh.number.as_str()) {
                        return Err(format!("{who}: another sheet is already numbered `{}`", sh.number));
                    }
                    if paper(&sh.size).is_none() {
                        return Err(format!("{who}: size `{}` isn't A0, A1, A2, A3 or A4", sh.size));
                    }
                }
                Element::Viewport(v) => {
                    if self.sheet(v.sheet).is_none() {
                        return Err(format!("{who}: {} {} isn't a sheet", self.cat(v.sheet), self.shown(v.sheet)));
                    }
                    let name = self.view_name(v.view).ok_or(format!(
                        "{who}: {} {} isn't a plan, a section, an elevation, a schedule or a 3D view",
                        self.cat(v.view),
                        self.shown(v.view)
                    ))?;
                    point(&who, v.at)?;
                    if !(v.scale.is_finite() && (1.0..=100_000.0).contains(&v.scale)) {
                        return Err(format!("{who}: scale 1 : {} isn't between 1 : 1 and 1 : 100000", v.scale));
                    }
                    if let Some(other) = placed.insert(v.view, v.sheet) {
                        let number = self.sheet(other).map_or("", |s| s.number.as_str());
                        return Err(format!("{who}: {name} is on sheet {number} already; a view goes on one sheet"));
                    }
                }
                Element::Tag(t) => {
                    let Some(host) = self.elements.get(t.host) else {
                        return Err(format!("{who}: element {} doesn't exist", t.host));
                    };
                    if let Some(v) = t.view {
                        let level = self.level_of(e);
                        level
                            .and_then(|l| self.plan_view_of(v, l))
                            .ok_or_else(|| format!("{who}: view {} isn't a plan view of its host's level", self.shown(v)))?;
                    }
                    // A door or window whose wall is gone is the wall's to complain of; anything else needs a place in a plan.
                    if !matches!(host, Element::Door(_) | Element::Window(_)) && self.anchor(t.host).is_none() {
                        return Err(format!(
                            "{who}: tags name what has a place in a plan (doors, windows, walls, columns, floors, rooms and the like), not {} {}",
                            host.category(),
                            t.host
                        ));
                    }
                    if let Some(label) = &t.label {
                        let cat = host.category();
                        let fields = crate::schedule::fields(self, cat);
                        if !fields.iter().any(|(name, _)| name == label) {
                            let names: Vec<String> = fields.into_iter().map(|(name, _)| name).collect();
                            return Err(format!("{who}: {cat} {} has no field `{label}`; its fields are: {}", t.host, names.join(", ")));
                        }
                    }
                    point(&format!("{who} offset"), t.offset)?;
                }
            }
        }
        if let Some(n) = view_names.intersection(&level_names).next() {
            return Err(format!("a view is named `{n}`, as a level is; name it otherwise"));
        }
        for (host, v) in spans {
            // Openings share a wall's place only where they share its height too: one over the other is not an overlap, and a
            // storefront wall with a window a storey is a wall whose windows overlap in plan alone.
            for (i, a) in v.iter().enumerate() {
                for b in v.iter().skip(i + 1) {
                    if b.start > a.end + TOL {
                        break;
                    }
                    if a.end > b.start + TOL && a.head > b.sill + TOL && b.head > a.sill + TOL {
                        return Err(format!("{} {} and {} {} overlap in wall {host}", self.cat(a.id), a.id, self.cat(b.id), b.id));
                    }
                }
            }
        }
        // Last, so what a rule above says of a reference comes first.
        self.check_ids()
    }

    fn cat(&self, id: impl Key) -> &'static str {
        self.elements.get(id).map_or("element", Element::category)
    }
}

/// The element a [`Model::check`] error is about: its messages about an element start with the
/// element's category and id, `door 12: …`.
pub fn culprit(error: &str) -> Option<Id> {
    let mut words = error.split_whitespace();
    words.next()?;
    words.next()?.trim_end_matches(':').parse().ok()
}

fn coord(what: &str, v: f64) -> Result<(), String> {
    if v.is_finite() && v.abs() <= MAX_COORD { Ok(()) } else { Err(format!("{what}: {v} is not a usable number")) }
}

/// Whether `b` is a section box: usable numbers, and some room between its planes on each axis.
fn section_box_ok(who: &str, b: &SectionBox) -> Result<(), String> {
    for (axis, (lo, hi)) in ["x", "y", "z"].into_iter().zip(b.min.into_iter().zip(b.max)) {
        coord(&format!("{who} section box {axis}"), lo)?;
        coord(&format!("{who} section box {axis}"), hi)?;
        if hi - lo < MIN_SECTION_BOX {
            return Err(format!(
                "{who}: its section box runs from {lo} to {hi} mm in {axis}; its max is at least {MIN_SECTION_BOX} mm past its min on each axis"
            ));
        }
    }
    Ok(())
}

/// Whether `r` is a view range that makes sense: usable numbers running top ≥ cut plane ≥ bottom ≥
/// view depth.
pub(crate) fn range_ok(who: &str, r: &ViewRange) -> Result<(), String> {
    for (what, v) in [("cut", r.cut), ("bottom", r.bottom), ("depth", r.depth)].into_iter().chain(r.top.map(|t| ("top", t))) {
        coord(&format!("{who} view range {what}"), v)?;
    }
    if !(r.depth <= r.bottom && r.bottom <= r.cut && r.top.is_none_or(|t| r.cut <= t)) {
        return Err(format!(
            "{who}: its plan's view range runs top ≥ cut plane ≥ bottom ≥ view depth, not top {}, cut {}, bottom {}, depth {}",
            r.top.map_or_else(|| "at the level above".into(), |t| t.to_string()),
            r.cut,
            r.bottom,
            r.depth
        ));
    }
    Ok(())
}

fn size(what: &str, v: f64) -> Result<(), String> {
    coord(what, v)?;
    if v > 0.0 { Ok(()) } else { Err(format!("{what} must be above zero, got {v}")) }
}

fn point(who: &str, p: P2) -> Result<(), String> {
    coord(&format!("{who} x"), p.0)?;
    coord(&format!("{who} y"), p.1)
}

/// `n` more members placed in `view` (by a detail group or a repeating detail), of the `placed` already: an error naming
/// `who` when the view would draw more than [`crate::groups::MAX_PLACED`] of them.
fn placing(who: &str, placed: &mut BTreeMap<Guid, usize>, view: Guid, n: usize) -> Result<(), String> {
    let total = placed.entry(view).or_default();
    *total = total.saturating_add(n);
    match *total > crate::groups::MAX_PLACED {
        true => Err(format!(
            "{who}: its view would draw {total} group members (a group places each of its type's members, a repeating detail \
             once for each repeat); a view draws up to {}, so use fewer repeats or a smaller group",
            crate::groups::MAX_PLACED
        )),
        false => Ok(()),
    }
}

/// A crop region from its corner `a` to `b`: finite, and `a` south-west of `b` (in a section, below
/// and before it).
fn crop(who: &str, a: P2, b: P2) -> Result<(), String> {
    point(&format!("{who} crop"), a)?;
    point(&format!("{who} crop"), b)?;
    if a.0 < b.0 - TOL && a.1 < b.1 - TOL {
        Ok(())
    } else {
        Err(format!("{who}: a crop region runs from its south-west corner to its north-east one (in a section, bottom left to top right)"))
    }
}

/// Landings between runs, going up: each a run of two risers or more from the last and to the
/// next (on a stair of `risers`), after a whole number of them; or on a ramp, up from its foot;
/// each turning square or back.
fn landings(who: &str, landings: &[Landing], risers: Option<f64>) -> Result<(), String> {
    if landings.len() > 100 {
        return Err(format!("{who}: more than 100 landings"));
    }
    let mut last = 0.0;
    for l in landings {
        coord(&format!("{who} landing"), l.at)?;
        if !matches!(l.turn, 0.0 | 90.0 | -90.0 | 180.0 | -180.0) {
            return Err(format!(
                "{who}: a landing turns 0 (straight on), 90 or -90 (left or right), or 180 or -180 (back beside the run, to its left or \
                 its right), not {}",
                l.turn
            ));
        }
        match risers {
            Some(_) if l.at.fract() != 0.0 => return Err(format!("{who}: a landing goes after a whole number of risers, not {}", l.at)),
            Some(n) if l.at < last + 2.0 || l.at > n - 2.0 => {
                return Err(format!(
                    "{who}: a landing after {} risers leaves a run of fewer than 2 risers; each run needs 2 or more",
                    l.at
                ));
            }
            None if l.at <= last + TOL => {
                return Err(format!("{who}: a landing {} mm up isn't above its foot or the landing before it", l.at));
            }
            _ => {}
        }
        last = l.at;
    }
    Ok(())
}

/// `ring` counter-clockwise.
fn ccw(ring: &[P2]) -> Vec<P2> {
    if geom::area(ring) < 0.0 { ring.iter().rev().copied().collect() } else { ring.to_vec() }
}

/// What lies inside `loops` (an outline, then holes in it) and outside `holes`: the outline as it
/// is, or convex pieces once a hole cuts it.
fn solid(loops: &[Vec<P2>], holes: &[Vec<P2>]) -> Vec<Vec<P2>> {
    let Some((outline, inner)) = loops.split_first() else { return Vec::new() };
    inner.iter().chain(holes).fold(vec![outline.clone()], |parts, hole| walls::minus(&parts, hole))
}

/// An outline: 3 to [`MAX_POINTS`] points around some area.
fn boundary(who: &str, ring: &[P2]) -> Result<(), String> {
    if ring.len() < 3 || ring.len() > MAX_POINTS {
        return Err(format!("{who}: a boundary needs 3 to {MAX_POINTS} points"));
    }
    for p in ring {
        point(who, *p)?;
    }
    if geom::area(ring).abs() <= TOL * TOL {
        return Err(format!("{who}: its boundary encloses no area"));
    }
    Ok(())
}

/// A floor's or a ceiling's openings: at most [`MAX_POINTS`] points between them, each an outline
/// inside `outline`, clear of it and of the others.
fn openings(who: &str, outline: &[P2], rings: &[Vec<P2>]) -> Result<(), String> {
    if rings.iter().map(Vec::len).sum::<usize>() > MAX_POINTS {
        return Err(format!("{who}: its openings have more than {MAX_POINTS} points between them"));
    }
    for (i, o) in rings.iter().enumerate() {
        boundary(&format!("{who} opening {i}"), o)?;
    }
    holes(who, "opening", outline, rings)
}

/// Holes in an outline (a roof's courtyards, a floor's or a ceiling's openings): each inside
/// `outline`, clear of it and of the others.
fn holes(who: &str, what: &str, outline: &[P2], rings: &[Vec<P2>]) -> Result<(), String> {
    // Whether `p` is inside `ring` (or outside it), more than TOL from its edges.
    let clear = |ring: &[P2], p: P2, inside: bool| {
        geom::contains(ring, p) == inside && geom::ring_edges(ring).all(|(a, b)| geom::dist_to_segment(p, a, b) > TOL)
    };
    let cross =
        |r: &[P2], s: &[P2]| geom::ring_edges(r).any(|(a, b)| geom::ring_edges(s).any(|(c, d)| geom::crossing(a, b, c, d).is_some()));
    for (i, c) in rings.iter().enumerate() {
        if !c.iter().all(|p| clear(outline, *p, true)) || cross(c, outline) {
            return Err(format!("{who}: {what} {i} isn't inside its outline"));
        }
        for (j, o) in rings.iter().enumerate().skip(i + 1) {
            if !c.iter().all(|p| clear(o, *p, false)) || !o.iter().all(|p| clear(c, *p, false)) || cross(c, o) {
                return Err(format!("{who}: {what}s {i} and {j} overlap"));
            }
        }
    }
    Ok(())
}

fn line(who: &str, a: P2, b: P2) -> Result<(), String> {
    point(who, a)?;
    point(who, b)?;
    if a.dist(b) > TOL { Ok(()) } else { Err(format!("{who}: too short ({} mm)", a.dist(b))) }
}

fn text(who: &str, what: &str, s: &str) -> Result<(), String> {
    if s.chars().count() <= MAX_TEXT { Ok(()) } else { Err(format!("{who}: {what} is longer than {MAX_TEXT} characters")) }
}

/// The type named `name`, or an error listing the types there are.
fn known<'a, T>(who: &str, name: &str, types: &'a BTreeMap<String, T>) -> Result<&'a T, String> {
    types.get(name).ok_or_else(|| {
        let names: Vec<&str> = types.keys().map(String::as_str).collect();
        format!("{who}: type `{name}` doesn't exist (there are: {})", names.join(", "))
    })
}

/// SplitMix64: spreads a seed over all 64 bits.
pub fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn fresh_uid() -> u64 {
    crate::ids::seed()
}

/// A file from before pens (format 3 and older) as one after: each line weight of a view's, a filter's or a phase's
/// overrides (the `weight` of a `cut` or `projection` style) was pixels of a sheet's drawing, [`crate::sheet::DENSITY`] to
/// the mm, and is the pen nearest that wide on paper. A weight that isn't a number is left for reading to refuse.
fn pens_from_pixels(v: &mut serde_json::Value) {
    use serde_json::Value;
    match v {
        Value::Object(o) => {
            for (k, v) in o.iter_mut() {
                if let ("cut" | "projection", Some(px)) = (k.as_str(), v.get("weight").and_then(Value::as_f64)) {
                    v["weight"] = crate::pen::Pen::nearest(px / crate::sheet::DENSITY).0.into();
                }
                pens_from_pixels(v);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(pens_from_pixels),
        _ => {}
    }
}
/// A door's or a window's place in its wall: how far along it the opening runs, the sill and the head above
/// the wall's own base, and which opening it is.
struct OpeningSpan {
    start: f64,
    end: f64,
    sill: f64,
    head: f64,
    id: Id,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(level: Guid, a: P2, b: P2) -> Element {
        Element::Wall(Wall {
            params: Default::default(),
            type_name: "Generic - 200mm".into(),
            level,
            start: a,
            end: b,
            top_level: None,
            height: 3000.0,
            base_offset: 0.0,
            arc: 0.0,
            location: Location::Centre,
            attach_top: None,
            attach_base: None,
        })
    }

    /// A 6 x 4 m box of walls on Level 1 (id 100000), returning the bottom wall's id.
    fn boxed(m: &mut Model) -> Id {
        let (a, b, c, d) = (P2(0.0, 0.0), P2(6000.0, 0.0), P2(6000.0, 4000.0), P2(0.0, 4000.0));
        let bottom = m.insert(wall(m.guid(100000), a, b));
        for (s, e) in [(b, c), (c, d), (d, a)] {
            m.insert(wall(m.guid(100000), s, e));
        }
        bottom
    }

    fn door(host: Guid, offset: f64) -> Element {
        Element::Door(Opening {
            params: Default::default(),
            family: None,
            type_name: "Single 900 x 2100".into(),
            host,
            offset,
            sill: 0.0,
            mark: String::new(),
            flip_facing: false,
            flip_hand: false,
        })
    }

    #[test]
    fn a_new_model_has_two_levels_and_checks_clean() {
        let m = Model::new("House");
        let names: Vec<&str> = m.levels().iter().map(|(_, l)| l.name.as_str()).collect();
        assert_eq!(names, ["Level 1", "Level 2"]);
        assert_eq!(m.check(), Ok(()));
    }

    #[test]
    fn walls_constrained_to_a_level_take_its_height() {
        let mut m = Model::new("t");
        let id = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
        let two = m.guid(100001);
        if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
            w.top_level = Some(two);
            w.base_offset = 100.0;
        }
        let w = m.wall(id).unwrap();
        assert_eq!((m.base_z(w), m.top_z(w)), (100.0, 3000.0));
    }

    #[test]
    fn plans_cut_the_walls_that_span_their_cut_plane() {
        let mut m = Model::new("t");
        let one = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
        let tall = m.insert(wall(m.guid(100000), P2(0.0, 1000.0), P2(5000.0, 1000.0)));
        if let Some(Element::Wall(w)) = m.elements.get_mut(tall) {
            w.height = 6000.0;
        }
        let (one, tall) = (m.wall(one).unwrap(), m.wall(tall).unwrap());
        assert!(m.cut_by(one, 100000) && !m.cut_by(one, 100001));
        assert!(m.cut_by(tall, 100000) && m.cut_by(tall, 100001));
    }

    #[test]
    fn walls_join_where_they_meet_but_not_across_levels() {
        let mut m = Model::new("t");
        let bottom = boxed(&mut m);
        // Mitred at the corners: the outer corner where the outer faces cross.
        assert!(walls::cap(&m, bottom, false).contains(&P2(-100.0, -100.0)));
        // A partition ending mid-span on the bottom wall stops at its face.
        let t = m.insert(wall(m.guid(100000), P2(3000.0, 0.0), P2(3000.0, 4000.0)));
        assert_eq!(walls::cap(&m, t, false), [P2(3100.0, 100.0), P2(2900.0, 100.0)]);
        // A wall upstairs meeting the same corner doesn't join walls downstairs.
        let up = m.insert(wall(m.guid(100001), P2(0.0, 0.0), P2(0.0, -3000.0)));
        assert_eq!(walls::cap(&m, up, false), [P2(-100.0, 0.0), P2(100.0, 0.0)]);
    }

    #[test]
    fn rooms_are_bounded_by_the_walls_their_level_cuts() {
        let mut m = Model::new("t");
        boxed(&mut m);
        let room =
            Room { params: Default::default(), level: m.guid(100000), point: P2(3000.0, 2000.0), name: "Hall".into(), number: "1".into() };
        assert_eq!(m.room_region(&room).map(|r| r.area()), Some(24e6));
        let upstairs = Room { level: m.guid(100001), ..room };
        assert_eq!(m.room_region(&upstairs), None, "Level 2 has no walls");
    }

    #[test]
    fn check_names_unknown_types_and_lists_the_known_ones() {
        let mut m = Model::new("t");
        let id = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
        if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
            w.type_name = "Brick".into();
        }
        let e = m.check().unwrap_err();
        assert!(e.contains(&format!("wall {id}")) && e.contains("Brick") && e.contains("Generic - 200mm"), "{e}");
    }

    #[test]
    fn check_refuses_doors_that_do_not_fit_their_wall() {
        let mut m = Model::new("t");
        let host = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
        m.insert(door(m.guid(host), 300.0));
        let e = m.check().unwrap_err();
        assert!(e.contains("fit"), "{e}");
    }

    #[test]
    fn check_refuses_overlapping_openings() {
        let mut m = Model::new("t");
        let host = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
        m.insert(door(m.guid(host), 1000.0));
        m.insert(door(m.guid(host), 2000.0));
        assert_eq!(m.check(), Ok(()));
        m.insert(door(m.guid(host), 2600.0));
        let e = m.check().unwrap_err();
        assert!(e.contains("overlap"), "{e}");
    }

    #[test]
    fn check_allows_openings_stacked_in_one_wall_and_still_refuses_ones_side_by_side() {
        // A wall six metres tall with a door and, over it, another: the Snowdon sample's storefront walls hold a window per
        // storey at the same place along the wall.
        let mut m = Model::new("t");
        let host = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
        if let Some(Element::Wall(w)) = m.elements.get_mut(host) {
            w.height = 6000.0;
        }
        m.insert(door(m.guid(host), 1000.0));
        let mut above = door(m.guid(host), 1000.0);
        if let Element::Door(o) = &mut above {
            o.sill = 2500.0;
        }
        m.insert(above);
        assert_eq!(m.check(), Ok(()), "openings that share their place but not their height do not overlap");
        m.insert(door(m.guid(host), 1000.0));
        let e = m.check().unwrap_err();
        assert!(e.contains("overlap"), "{e}");
    }

    #[test]
    fn check_refuses_dangling_references_and_bad_numbers() {
        let mut m = Model::new("t");
        m.insert(wall(Guid::of_id(99), P2(0.0, 0.0), P2(5000.0, 0.0)));
        assert!(m.check().unwrap_err().contains("level 99 doesn't exist"));
        let mut m = Model::new("t");
        let nowhere = Guid::new();
        m.insert(wall(nowhere, P2(0.0, 0.0), P2(5000.0, 0.0)));
        assert!(m.check().unwrap_err().contains(&format!("level {nowhere} doesn't exist")));
        // An element pointing at another by its id, not its GUID, is one a command forgot to resolve.
        let mut m = Model::new("t");
        m.insert(wall(Guid::of_id(100_000), P2(0.0, 0.0), P2(5000.0, 0.0)));
        assert!(m.check().unwrap_err().contains("points at element 100000 by its id, not its GUID"));
        m.resolve();
        assert_eq!(m.check(), Ok(()));
        let mut m = Model::new("t");
        m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(f64::NAN, 0.0)));
        assert!(m.check().is_err());
        let mut m = Model::new("t");
        m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(0.5, 0.0)));
        assert!(m.check().unwrap_err().contains("short"));
    }

    #[test]
    fn element_json_rejects_unknown_fields() {
        let ok = r#"{"category":"level","name":"Roof","elevation":6000}"#;
        assert!(serde_json::from_str::<Element>(ok).is_ok());
        let typo = r#"{"category":"level","name":"Roof","elevaton":6000}"#;
        let e = serde_json::from_str::<Element>(typo).unwrap_err().to_string();
        assert!(e.contains("elevaton"), "{e}");
    }

    #[test]
    fn new_elements_default_to_the_types_a_new_model_starts_with() {
        let mut m = Model::new("t");
        let defaults = ["wall", "floor", "door", "window"].map(|c| m.default_type(c));
        assert_eq!(defaults, [Some("Generic - 200mm"), Some("Generic - 200mm"), Some("Single 900 x 2100"), Some("Fixed 1200 x 1500")]);
        assert_eq!(m.type_names("doors"), ["Single 800 x 2100", "Single 900 x 2100"], "any case, singular or plural");
        m.types.wall.remove("Generic - 200mm");
        assert_eq!(m.default_type("wall"), Some("Exterior - 300mm"), "else the first by name");
        assert_eq!(m.default_type("room"), None, "rooms have no types");
    }

    #[test]
    fn insert_never_reuses_an_id_from_a_stale_counter() {
        let mut m = Model::new("t");
        m.next_id = 1;
        let id = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
        assert_eq!(id, 100_002, "past the levels, and never under six digits");
    }
}
