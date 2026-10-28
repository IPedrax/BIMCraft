//! The command set: everything the CLI, scripts and MCP can do. Parameters are JSON, lengths are
//! millimetres. Commands build elements; [`Model::check`] then validates the whole model, so a
//! command only has to read its parameters carefully.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::cloud::{self, Cloud};
use crate::engine::{Command, Run, Session};
use crate::favorites::{self, FamilyEntry, Favorites, MaterialEntry};
use crate::fills::{FillPattern, FilledRegion, FilledRegionType, Target};
use crate::geom::{self, P2};
use crate::graphics::{self, Graphics, Group, Look, Override, ViewTemplate};
use crate::language::Language;
use crate::library::Kind as LibKind;
use crate::lines::{DetailLine, LineStyle};
use crate::massing;
use crate::model::{
    CEILING_HEIGHT, Ceiling, CeilingType, Column, ColumnType, Component, DEFAULT_HEIGHT, DimKind, Dimension, Element, Extrusion, Face,
    Facing, Floor, FloorType, Grid, Guid, Id, Key, Landing, Layer, Level, Line, Location, MAX_COORD, MAX_POINTS, MAX_SLOPE, MAX_TEXT, Mass,
    Model, NOTE_SIZE, Note, Opening, OpeningType, PARAMETER_CATEGORIES, ParameterKind, Pattern, PlanView, PointCloud, ProjectParameter,
    RAMP_SLOPE, RAMP_WIDTH, RISER, Railing, RailingType, Ramp, Reference, Roof, RoofType, Room, STAIR_WIDTH, Schedule, Section, SectionBox,
    Shaft, Sheet, Side, Spot, Stair, TOL, TREAD, Tag, View3D, Viewport, Wall, WallType, heading, paper,
};
use crate::paint;
use crate::phasing;
use crate::render::Drawing;
use crate::sketch::{self, SketchLine};
use crate::texture::{self, Kind, Picture, Texture};
use crate::units::Units;
use crate::walls::{self, Axis, stacked};
use crate::{pdf, render, schedule, sheet, snap, stairs, view3d};

type R = Result<Value, String>;

const fn edit(id: &'static str, label: &'static str, aliases: &'static [&'static str], params: &'static str, run: Run) -> Command {
    Command { id, label, aliases, params, run, undoable: true }
}

/// A command that is no undo step: it only reads, or replaces the whole project.
const fn other(id: &'static str, label: &'static str, params: &'static str, run: Run) -> Command {
    Command { id, label, aliases: &[], params, run, undoable: false }
}

pub static ALL: &[Command] = &[
    other(
        "new",
        "New",
        "{name?, language?: en|pt|es} an empty project with Level 1 (0) and Level 2 (3000), named in its language (Nível 1, \
         Nível 2 in Portuguese; Nivel 1, Nivel 2 in Spanish)",
        new,
    ),
    other("open", "Open", "{path} a .bimcraft project", open),
    other("highrise", "Synthetic Highrise", "{storeys?: 20} replace the project with an original 1–80 storey performance sample", highrise),
    other("autosave", "Autosave", "{path} atomically write a recovery snapshot, keeping the project path and undo history", autosave),
    other(
        "recover",
        "Recover Autosave",
        "{path, project?} open and validate a recovery snapshot; Save targets `project`, or asks for Save As without it",
        recover,
    ),
    edit(
        "insert",
        "Insert from File",
        &[],
        "{path, ids?: [id, …]} elements of another project (.bimcraft or .ifc) brought into this one: those `ids` names \
         (their ids there), else all of them, and what they point at that this project hasn't (a level they stand on is \
         this project's of the same name, where it has one). Each keeps its id unless this project has that id, and its GUID \
         unless this project has that GUID: then it's a copy, with a GUID of its own (the same file inserted into itself is \
         copied whole). What they point at among themselves follows them. A name or a number taken here gets ` 2`, ` 3`…; \
         types, families, materials and parameters this project hasn't come by name, and so do the line styles (with their line \
         patterns), fill patterns, filled region types and detail groups a detail line, a filled region or a group uses. Returns `ids` {there: here}, \
         `copies`, `library`, `left_out`",
        insert_from,
    ),
    edit(
        "link",
        "Link Model",
        &["LK"],
        "{path, name?, at?: [x, y], angle?: degrees} another project (.bimcraft or .ifc) shown in this one as a reference that \
         can't be edited, as Revit's Link Revit: its walls, floors, roofs, columns, stairs and the rest of what it builds are \
         drawn in the plans, sections and 3D views, halftone, and snapped to; nothing of it is selected, scheduled, checked or \
         exported. It stands with its origin at `at` (default [0, 0]) turned `angle` about it; its levels stand at their own \
         elevations, on this project's levels at the same height. `path` is read now and again by `reload`, and kept as given: \
         a relative one is relative to this project's file. `name` defaults to the file's. Returns `id`, `name`, `elements`",
        link_model,
    ),
    other(
        "reload",
        "Reload Links",
        "{id?} read the file of the link `id`, or of every link, again: for a project that changed on disk. Returns `reloaded` \
         (the names) and `failed` ({name, error})",
        reload,
    ),
    other(
        "save",
        "Save",
        "{path?} .bimcraft saves the project (Save As with a path); .ifc exports IFC4; .svg or .png exports the lowest level's \
         plan; .pdf prints every sheet, a page each",
        save,
    ),
    other("undo", "Undo", "{count?} takes back the last `count` steps (1 by default)", undo),
    other("redo", "Redo", "{count?} does again the last `count` steps undone (1 by default)", redo),
    edit(
        "level",
        "Level",
        &["LL"],
        "{elevation, name?, extent?: [[x,y],[x,y]]} a level at an elevation in mm; `extent`, the rectangle in plan its \
         plane stretches over (south-west corner, then north-east), where sections, elevations and the 3D view show it \
         (without one, the model's, a metre past it)",
        level,
    ),
    edit("grid", "Grid", &["GR"], "{start: [x,y], end: [x,y], name?} names follow the last grid: 1, 2… or A, B…", grid),
    edit(
        "point_cloud",
        "Point Cloud",
        &[],
        "{path | cloud, name?, level?, at?: [x, y], offset?, angle?, units?, spacing?, origin?} brings in a laser scan (`path`: \
         PTS, XYZ, PLY, LAS or E57 (each of its scans placed by its pose), or a list of the files of one scan's stations, brought in as one; `units` of its coordinates mm, \
         cm, m (the default), in or ft; one point kept in each cube `spacing` mm wide) or places one the project holds again \
         (`cloud`, its name), as an element: its middle on `at` (the origin) and its foot on the level, or with `origin: file` \
         where its file's coordinates put it; `offset` raises its middle over the level, `angle` turns it. Returns its id, the \
         scan's name, the points kept and read, and where its middle was in the file",
        point_cloud,
    ),
    edit(
        "wall",
        "Wall",
        &["WA"],
        "{points: [[x,y],…], closed?, type?, level?, top_level?, height?, base_offset?, arc?, location?, attach_top?, \
         attach_base?} one wall per segment of its location line; up to the next level, or `height` mm tall (unconnected), or up \
         to `top_level`. `arc`: degrees each turns from its start to its end, counter-clockwise (180, a half circle bulging to \
         its right). `location`: the line of it the points are on: centre (the default), core-centre, exterior-finish, \
         interior-finish, exterior-core or interior-core; its exterior side is to the left of its start to its end, so walls \
         drawn clockwise face out. `attach_top`, `attach_base`: a roof or floor its top follows from below, or its base stands \
         on",
        wall,
    ),
    edit(
        "door",
        "Door",
        &["DR"],
        "{at: [x,y] | wall: id, offset?, type?, family?, level?, mark?, flip_facing?, flip_hand?} in the wall under `at`, \
         or in `wall` centred `offset` mm from its start; with `family`, a door family's `type` (its first by default), \
         which sizes the opening and draws the door",
        door,
    ),
    edit(
        "window",
        "Window",
        &["WN"],
        "{at: [x,y] | wall: id, offset?, sill?, type?, family?, level?, mark?, flip_facing?} like door; `sill` defaults to the \
         type's (a family's `Sill Height`)",
        window,
    ),
    edit(
        "floor",
        "Floor",
        &[],
        "{boundary?: [[x,y],…], openings?: [[[x,y],…],…] | sketch?: [[[x,y],[x,y]],…], level?, type?, offset?, slope?, arrow?: \
         [[x,y],[x,y]], structural?} a slab (a structural one with `structural`, as Revit's Floor: Structural), its top at the level, or `offset` mm above it, open through `openings` (holes inside it, apart); no boundary = the outline \
         of the level's walls along their centre lines, open over each loop of them standing free inside, joined to none of \
         the outer ones and with no room in it (a courtyard). Sloped: its top rises `slope` degrees (negative, falls) along \
         the slope arrow, from its tail, where it's at the offset, toward its head. Its top's model pattern starts at \
         `pattern_origin` and turns by `pattern_angle` (see modify). An outline, and each opening, is simple: \
         no edge across another or along it, none ending part way along one, no spike or keyhole slit (`ceiling`, `roof` \
         and `shaft` too, and `modify`, `move` and `edit_boundary` of any of them: they are refused, saying which edges); a \
         file that has one still opens, and works until a command changes its outline",
        floor,
    ),
    edit(
        "ceiling",
        "Ceiling",
        &[],
        "{point: [x,y] | boundary: [[x,y],…], openings?: [[[x,y],…],…] | sketch: [[[x,y],[x,y]],…], level?, type?, offset?} a ceiling, its underside \
         `offset` mm (2600) above the level, open through `openings` (holes inside it, apart): in the room around `point`, out \
         to its walls' faces and round the walls standing inside it, or along `boundary`. Reflected ceiling plans show it",
        ceiling,
    ),
    other(
        "sketch",
        "Sketch",
        "{lines: [[[x,y],[x,y]],…]} solves a sketch without making anything, as Finish Edit Mode does: lines in any order, \
         either way round, joined end to end (within 1 mm) into closed loops; a loop inside no other (or inside a hole) an \
         outline, counter-clockwise, one inside an outline a hole in it, clockwise. Gives each region's `outline`, `holes` and \
         `area_m2`, or every problem with the lines it's about (numbered from 0): ends no other line meets, lines that \
         intersect or overlap (a line too: a polyline round a bow tie crosses itself), three or more lines at a point. \
         A sketch is solved as the straight pieces its lines are drawn with (an arc, a circle or a curve is cut into chords, \
         a circle into up to 8,193): past 200,000 of them, or when they lie so close together that telling which cross \
         takes more than six million steps, it is refused at once. `sketch` on floor, ceiling, roof and shaft, and \
         edit_boundary, take the same lines for one outline and its holes",
        sketch_lines,
    ),
    edit(
        "shaft",
        "Shaft Opening",
        &[],
        "{boundary: [[x,y],…] | sketch: [[[x,y],[x,y]],…], level?, top_level?, height?, base_offset?} a hole through the \
         floors, ceilings and roofs it passes through, from `base_offset` above the level up to the next level, or `height` \
         mm, or up to `top_level`",
        shaft,
    ),
    edit(
        "edit_boundary",
        "Edit Boundary",
        &[],
        "{id, sketch?: [[[x,y],[x,y]],…]} redraws a floor's, a ceiling's, a roof's footprint's or a shaft's outline and its \
         holes (a floor's or a ceiling's openings, a roof's courtyards; a shaft has none) as a sketch, all at once (see \
         `sketch`); a roof's gable edges stay gables where the new footprint has the same edge. Without `sketch`, gives the \
         outline and holes it has now, as the lines to edit. The outline must be simple, as `floor` says; a line that \
         crosses itself is found as two that cross are",
        edit_boundary,
    ),
    edit(
        "mass",
        "Mass",
        &["MS"],
        "{boundary: [[x,y],…], level?, top_level?, height?, base_offset?, top?: [[x,y],…], lean?: [dx,dy], name?} a mass, Revit's \
         conceptual volume: the footprint extruded from `base_offset` above the level up to the next level, or `height` mm, or up \
         to `top_level`. With `top`, an outline of as many points as the footprint (twin for twin, running the same way round), \
         it is lofted straight from the footprint to it; `lean` makes one by moving the footprint sideways by [dx, dy] for its \
         top, a sloped mass. Drawn dashed in plans and sections (the outline at the height they cut it, between the two when \
         lofted), a see-through solid in 3D; its own category, `mass`, which views may hide. Its areas and volumes, per level \
         too, are those of the cross sections between its outlines, exactly. Make \
         floors from it with `mass_floors`, walls, a floor and a roof from its faces with `wall_by_face`, `floor_by_face` \
         and `roof_by_face`",
        mass,
    ),
    edit(
        "mass_floors",
        "Mass Floors",
        &["MF"],
        "{mass?, type?, prune?} a floor of the footprint at each level the mass crosses, from the one at its base up to, not \
         including, the one at its top; every mass when no `mass`. The floors it makes hold their mass and level (`Mass Floor`), \
         so a second run makes no second floor: it gives the ones there the mass's footprint now (and the `type` if given), \
         and lists in `stale` those on a level the mass no longer crosses, which `prune` deletes. The reply has `ids` (every \
         floor of the mass), `made`, `updated`, `stale` and `removed`. `schedule {category: massfloors}` lists each mass's \
         floor area and volume on each level",
        mass_floors,
    ),
    edit(
        "wall_by_face",
        "Wall by Face",
        &["WB"],
        "{mass, faces?: [edge, …], type?, location?} a wall on each edge of the mass's footprint (numbered from its first \
         point), or the `faces` given, as tall as the mass; its exterior finish on the edge, facing out (`location` changes \
         that line). Ordinary walls from then on",
        wall_by_face,
    ),
    edit(
        "floor_by_face",
        "Floor by Face",
        &["FB"],
        "{mass, type?} a floor on the mass's top face: its top at the mass's top level, or `height` over its base. An ordinary \
         floor from then on",
        floor_by_face,
    ),
    edit(
        "roof_by_face",
        "Roof by Face",
        &["RB"],
        "{mass, type?, slope?, overhang?} a roof on the mass's top face, flat unless it has a `slope` (degrees), over the mass's \
         top level or `height` over its base. An ordinary roof from then on",
        roof_by_face,
    ),
    edit(
        "update_by_face",
        "Update to Face",
        &["UF"],
        "{mass?, ids?} rebuilds what `wall_by_face`, `floor_by_face` and `roof_by_face` made from the mass as it is now (Revit's \
         Update to Face): the walls on its edges, as tall as it is, a floor and a roof on its top face. The ones of `mass`, or the \
         `ids` given, or every one in the project; what was done to them since (their type, a roof's slope) stays. A face that \
         is gone (a footprint with fewer corners) refuses the lot and says which; a deleted mass's are ordinary elements. Revit \
         has no default shortcut for it: UF is BIMCraft's. The reply has `updated` and `unchanged`",
        update_by_face,
    ),
    edit(
        "show_mass",
        "Show Mass",
        &["MH"],
        "{view?, show?} hides the masses in a view if it shows them, shows them if it hides them (or as `show`, a yes or no, \
         says): the `mass` category of `graphics {hide|show}`, in one step. The reply has `shown` and `hidden`, the categories \
         the view hides. Revit's Show Mass has no default shortcut; MH is BIMCraft's",
        show_mass,
    ),
    edit("room", "Room", &["RM"], "{point: [x,y], level?, name?, number?} bounded by the walls around the point", room),
    edit(
        "column",
        "Column",
        &["CL"],
        "{at: [x,y], type?, angle?, level?, top_level?, height?, base_offset?} a column, its width along `angle` degrees; up to \
         the next level, or `height` mm tall, or up to `top_level`",
        column,
    ),
    edit(
        "structural_column",
        "Structural Column",
        &["CL"],
        "{at: [x,y], type?, angle?, level?, top_level?, height?, base_offset?} a column of the structure, as `column` makes one but \
         in its own category with its own types (`type` with category structural column): up to the next level, or `height` mm \
         tall, or up to `top_level`. CL is `column`'s too; on the desktop's Structure tab it is this one",
        structural_column,
    ),
    edit(
        "footing",
        "Isolated Footing",
        &["FI"],
        "{at: [x,y], type?, thickness?, offset?, angle?, level?} an isolated (pad) footing, its plan the type's (category \
         footing), `thickness` mm thick (400) with its top `offset` mm over the level (0: at it), so it hangs below",
        footing,
    ),
    edit(
        "beam",
        "Beam",
        &["BM"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a beam for each \
         segment of the points: its section the type's (category beam: `width` across, `depth` deep, or round), its `justify` \
         line (top by default) `offset` mm over the level at its start (0: at the level, so it hangs below) and `end_offset` at \
         its end (the same: level), for a single segment; `system` names the system it belongs to. It slopes up to 75°",
        beam,
    ),
    edit(
        "strip_footing",
        "Wall Footing",
        &["FW"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, system?} a continuous footing under a wall for each segment of the \
         points, its section the type's (category stripfooting), its top `offset` mm over the level (0: at it)",
        strip_footing,
    ),
    edit(
        "pipe",
        "Pipe",
        &["PI"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a pipe for each \
         segment of the points: its section the type's (category pipe: round, `width` the outside diameter), its `justify` line \
         (centre by default) `offset` mm over the level at its start (0: at the level) and `end_offset` at its end (the same: level), \
         for a single segment, so a drain falls and a riser rises; the same point twice and two heights make one stand upright. \
         `system` names the system it belongs to (\"Água fria\", \"Esgoto\")",
        pipe,
    ),
    edit(
        "conduit",
        "Conduit",
        &["CN"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a conduit (eletroduto) \
         for each segment of the points, as `pipe` lays them: round, `width` its outside diameter; `system` names its circuit",
        conduit,
    ),
    edit(
        "cable_tray",
        "Cable Tray",
        &["CT"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a cable tray (eletrocalha) \
         for each segment of the points, as `pipe` lays them: its section the type's `width` across by `depth` deep",
        cable_tray,
    ),
    edit(
        "wire",
        "Wire",
        &["EW"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a wire (condutor) for \
         each segment of the points, as `pipe` lays them: round, `width` its outside diameter; `system` names its circuit",
        wire,
    ),
    edit(
        "down_conductor",
        "Down Conductor",
        &["DC"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a down conductor \
         (condutor de descida, NBR 5419) for each segment of the points, as `pipe` lays them: round, or a flat strip; the same point \
         twice and two heights make one stand upright down a wall; `system` names the protection system it belongs to",
        down_conductor,
    ),
    edit(
        "ground_conductor",
        "Earthing Conductor",
        &["GC"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} an earthing conductor \
         (condutor de aterramento, the ring or mesh of NBR 5419) for each segment of the points, as `pipe` lays them, usually under \
         the level (a negative `offset`)",
        ground_conductor,
    ),
    edit(
        "cabling",
        "Cabling",
        &["CB"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} structured cabling \
         (cabeamento estruturado, NBR 14565) for each segment of the points, as `pipe` lays them: round, `width` the cable's outside \
         diameter; `system` names the link or the network it belongs to",
        cabling,
    ),
    edit(
        "stair",
        "Stair",
        &[],
        "{start: [x,y], end?: [x,y] | angle?, level?, top_level?, height?, width?, riser?, tread?, landings?: [{at, turn?}, …], \
         railings?} a stair from the middle of its first riser at `start`, climbing toward `end` (or `angle` degrees) up to the next level: \
         as many risers as keep each at most `riser` mm (180), treads `tread` deep (280), `width` wide (1000). One straight \
         run, as long as that makes it; or with landings (each as deep as the stair is wide) after `at` risers, the next run \
         going on as `turn` says: 0 straight on, 90 or -90 turned left or right, 180 or -180 back beside it, to its left or right. \
         With `railings`, a railing down each side too",
        stair,
    ),
    edit(
        "ramp",
        "Ramp",
        &[],
        "{start: [x,y], end?: [x,y] | angle?, level?, top_level?, height?, width?, slope?, landings?: [{at, turn?}, …], railings?} a ramp \
         from the middle of its foot at `start`, climbing toward `end` (or `angle` degrees) up to the next level, or `height` mm, \
         `width` wide (1200), at `slope` percent (8.33, 1 in 12), as long as that makes it; with landings `at` mm up, turning \
         as a stair's do, and with `railings`, a railing down each side. Solid down to its level",
        ramp,
    ),
    edit(
        "railing",
        "Railing",
        &[],
        "{host: a stair or a ramp, side?: left|right, type?} | {path: [[x,y],…], level?, base_offset?, type?} a railing along \
         the stair's or the ramp's sides (both, or the one going up it on `side`), 50 mm in from its edges and climbing with it, \
         going where it goes; or along `path`, `base_offset` mm above the level. Its type sets how high its rail stands over \
         what it runs along, and how far apart its balusters are",
        railing,
    ),
    edit(
        "roof",
        "Roof",
        &[],
        "{level?, boundary?: [[x,y],…], courtyards?: [[[x,y],…],…] | sketch?: [[[x,y],[x,y]],…], slope?, overhang?, gables?: [edge, …], gable?, offset?, \
         extrusion?, openings?: [[[x,y],…],…], join?, type?} a roof on `level` (by default the one above the lowest), its \
         base `offset` mm above it. Over `boundary` or else the walls of the level below, `overhang` mm (500) past their \
         faces, round any `courtyards` (holes in it, inside it and apart, with eaves of their own; over the walls, by \
         default each loop of them standing free inside, joined to none of the outer ones and with no room in it): flat \
         at `slope` 0, else every edge sloping up at `slope` degrees (30), hips, valleys and ridges where the slopes meet, \
         but the edges in `gables` (edge i runs from point i to the next, the courtyards' edges numbered on after the \
         boundary's) left upright, gable ends; `gable` leaves a rectangle's short ends upright. Or with `extrusion` {line: \
         [[x,y],[x,y]], profile: [[along, up],…], from?, to}: its underside's profile in the upright plane through `line`, \
         along it from its start and up from the base, swept square to it from `from` (0) to `to` mm to its left. \
         `openings` are holes through it in plan; with `join`, a roof it runs into, it ends where it meets that one's top",
        roof,
    ),
    edit(
        "dormer",
        "Dormer",
        &[],
        "{host, at: [x, y], width?, height?, slope?, window?, type?} a dormer on roof `host`, its front wall across the \
         slope through `at`: walls of `type` `width` mm (1600) apart, standing on the roof and `height` mm (1200) tall at \
         the front, under a gable roof of its own sloping `slope` degrees (30) that runs back into the host; the host open \
         under it, and with `window` (a window type) a window in its front. Replies with its roof, walls and window",
        dormer,
    ),
    edit(
        "dimension",
        "Aligned Dimension",
        &["DI"],
        "{refs: [ref, …], at: [x, y], level?, kind?: aligned|angular|radial} aligned (the default): the distances between \
         parallel references, along a line through `at`; angular: the angle between two references that meet, on the side \
         of `at`; radial: a round column's radius, toward `at`. A ref is {id, face?}: a wall's centre (the default), left or \
         right face (of its start-to-end direction), start or end; a door's or window's centre, or its jamb toward its wall's \
         start or end; a grid's line; a straight detail line, or its start or end (radial: a circle or an arc of one). Or a \
         point [x, y] on one of those (for radial, on a round column's edge). It follows what it measures",
        dimension,
    ),
    edit(
        "text",
        "Text",
        &["TX"],
        "{text, at: [x, y], view?, size?, angle?, leader?: [x, y]} a text note, its first line's top left at `at`, in a view \
         by name or id: a level's plan (the lowest by default; `at` in plan mm), a section or an elevation (`at` along its \
         line from its start, and up), or a sheet by number (`at` in paper mm). `size` is its capitals' height on paper, \
         mm (2.5); `angle` degrees counter-clockwise; with `leader`, a line to that point. Lines break at \\n",
        text_note,
    ),
    edit(
        "spot",
        "Spot Elevation",
        &["EL"],
        "{at: [x, y], view?, host?} a spot elevation: in a level's plan (the lowest by default), the height of the top of \
         what stands on the level at `at` (or of `host`), following it; in a section or an elevation, of its point `at`, \
         along and up",
        spot,
    ),
    edit(
        "tag",
        "Tag by Category",
        &["TG"],
        "{ids?: [id,…], label?, level?, category?, offset?: [dx, dy], leader?} tags elements with a field of theirs: a door's or \
         window's mark, a room's name, else the type, or `label`, any field its category's schedule has (`Height`, `Area`, a \
         project parameter); doors, windows, walls, columns, components, floors, ceilings, roofs, rooms, runs and stairs. `offset` \
         mm from the point it names (beside the wall for a door or window, else up and right); without ids, every door and window \
         on the level not tagged yet, or of `category`",
        tag,
    ),
    edit(
        "section",
        "Section",
        &["CR"],
        "{start: [x,y], end: [x,y], depth?, name?} a section view along the line, looking to its left (drawn west to east, it \
         looks north): it cuts the model at the line and sees `depth` mm past it, or everything",
        section,
    ),
    edit(
        "elevation",
        "Elevation",
        &[],
        "{facing?: north|south|east|west, at?: [x,y], start?, end?, depth?, name?} an elevation view of the building's side \
         `facing` (North shows the north side, looking south), from 3 m out, or through `at`; with `at` alone, of the side it's \
         on; with `start` and `end`, along that line, looking to its left; with none, the four not there yet",
        elevation,
    ),
    edit(
        "sheet",
        "Sheet",
        &[],
        "{number?, name?, size?: A0|A1|A2|A3|A4} a sheet of paper, landscape (A3 by default), with a title block; numbers \
         run A101, A102…",
        new_sheet,
    ),
    edit(
        "place",
        "Place View",
        &[],
        "{sheet: number or id, view: a level's, section's, elevation's, schedule's or 3D view's name or id, at?: [x, y], \
         scale?, azimuth?, elevation?} puts the view on the sheet at 1 : `scale` (by default the view's own, see `scale`, 1 : 100 when it has none of its own; a schedule keeps its size), \
         its middle `at` mm from the paper's bottom left corner (by default right of the views there, or in the middle); a \
         view goes on one sheet. A schedule (Room Schedule, Door Schedule…) or the 3D view ({3D}, seen from `azimuth` and \
         `elevation` degrees) comes to be as it's first placed",
        place,
    ),
    edit(
        "crop",
        "Crop View",
        &[],
        "{view, region?: [[x, y], [x, y]] | null} crops a level's plan to the region's south-west and north-east corners \
         (plan mm), or a section or an elevation to its bottom left and top right (along its line from its start, and up: \
         its line then runs between them, and the notes in it stay put); null uncrops it; without a region, it crops to \
         what the view shows now",
        crop,
    ),
    edit(
        "duplicate_view",
        "Duplicate View",
        &["DP"],
        "{view, mode?: duplicate|detailing|dependent, name?} a new plan view of a plan's level (a level's own plan, or a plan \
         view; sections, elevations, schedules, sheets and the 3D view aren't duplicated yet): `duplicate` (the default) has \
         the same view range, crop and graphics and no annotation of its own; `detailing` copies the dimensions, tags, notes \
         and spot elevations in it too, as its own; `dependent` follows its parent's view range, graphics, scale and detail \
         (change those in the parent), has a crop of its own and shows its parent's annotation as well as its own. Named \
         `name`, else `<view> Copy 1` (`<view> Dependent 1`). Commands that take a level or a view take it by name or id, \
         and what is drawn, dimensioned, tagged or noted there is in it",
        duplicate_view,
    ),
    edit(
        "scale",
        "View Scale",
        &[],
        "{view, scale?: 1 | … | 100000} a plan's, section's or elevation's scale, 1 : `scale`: the paper the view stands for, \
         1 mm of paper being `scale` mm of the model. Its annotation (lettering, grid bubbles, tags, dimension ticks…) and its \
         line weights (pens) are that paper's, so a screen shows them as the paper would at its zoom. Every view has one: \
         1 : 100 until it is given another, and null is refused. A view placed on a sheet takes it for the viewport's scale. \
         Without `scale` it only says what the view has",
        view_scale,
    ),
    edit(
        "detail",
        "Detail Level",
        &[],
        "{view, detail?: coarse | medium | fine} how much a plan, a section or an elevation draws of what it cuts: coarse, one \
         poché and openings as their gaps; medium, compound layers with the lines between them and an opening's leaf, swing and \
         glass; fine (the default), the layers' patterns and a floor's surface pattern too. Without `detail` it only says what \
         the view has",
        view_detail,
    ),
    edit(
        "visual_style",
        "Visual Style",
        &[],
        "{view?: \"{3D}\" | a 3D view's name or id, style?: shaded | hidden_line | wireframe} how a 3D view draws its surfaces: \
         shaded (lit, in their materials' colours and textures, the default); hidden_line (paper-white with their edges, what \
         a face hides staying hidden); wireframe (every edge, seen through the faces). Without `style` it only says what the \
         view has; the default 3D view comes to be as it is first changed",
        visual_style,
    ),
    edit(
        "type",
        "Edit Type",
        &[],
        "{category: wall|floor|ceiling|door|window|column|structuralcolumn|footing|roof|railing|beam|stripfooting|pipe|conduit|\
         cabletray|wire|downconductor|groundconductor|cable, name, thickness? | width?, height?, sill? | width?, depth?, round?, \
         material? | height?, spacing?} creates the type, or changes it everywhere it's used. A column's, a structural column's or \
         a footing's type is a `width` by `depth`, or `round` (`width` across); a run's (a beam's, a pipe's, a cable tray's…) too, \
         made of a `material` of the library. A wall type may have `layers` [{function?: structure|\
         substrate|thermal|membrane|finish, material?, thickness}, …] from its exterior side in: it's as thick as they are, \
         its core from its first structure layer to its last, and a new `thickness` goes to its structure. A ceiling type may \
         have `grid`: its square tiles' width, mm, which reflected ceiling plans draw (0, none); a railing type, `height` (its \
         rail over what it runs along) and `spacing` (its balusters' centres, 0 for none). Floor, roof and ceiling types may \
         have `layers` too, from the top down; a layer's `material` is one of the library's (see `material`)",
        edit_type,
    ),
    edit(
        "material",
        "Materials",
        &[],
        "{name, color?: \"#rrggbb\" | [r, g, b], cut?, surface?, texture?: {kind?, size?: [w, h], angle?, offset?: [x, y], path?, \
         image?} | null, rename?, delete?: true} a material of the project's library, created or changed: its colour in shaded 3D \
         views, the pattern its cut shows in plans and sections (where a layer is wide enough to hold it), and the one its \
         surfaces show at its size in the building (a floor's top in plan, a ceiling's underside in its ceiling plan). Patterns: \
         none, solid, diagonal, crosshatch, horizontal, vertical, grid, brick, concrete, insulation, or the name of one of the \
         project's fill patterns (see `fill_pattern`), kept while the material shows it and renamed with it. Its `texture` is what its \
         surfaces look like in the 3D view, over its colour, a tile of it `size` mm across and up, turned `angle` degrees and \
         moved `offset` mm: a pattern drawn in code, `kind` brick, block, boards, concrete, plaster, tile, roof, fabric, metal \
         or insulation (at its natural size unless `size` says); or `image`, a picture: the PNG or JPEG file at `path`, taken \
         into the project (up to 1024 px a side, 24 pictures), or one the project holds (`image`, its id from `inspect`); null \
         takes the texture away. `rename` renames it, the layers using it following; `delete` takes it away, while no layer \
         uses it. `inspect` lists the library and the pictures",
        material,
    ),
    edit(
        "favorite",
        "Favorites",
        &[],
        "{action?: list|save|load|delete, kind?: material|family, name?, as?, replace?, path?} the user's favorites, kept in a file \
         of their own (`path`; by default in their configuration folder) for every project to use: `save` the project's \
         material or family `name` (a material with the picture its texture is made of, a family with the materials it is \
         made of), under another name with `as`; `load` a favorite into the project, under another name with `as`, or over \
         one it has already with `replace`; `delete` one from the favorites; `list` them (the default)",
        favorite,
    ),
    edit(
        "paint",
        "Paint",
        &["PT"],
        "{id | ids, face?, material} paints a face of the elements with a material of the library, as Revit's Paint does: its \
         colour and texture show in the 3D view over what their types are made of. `face`: a wall's `exterior` or `interior`, a \
         floor's, roof's or ceiling's `top` or `bottom`, and `all` for a column, a component, a door, a window, a stair, a ramp or \
         a railing; without it, every face the elements have. `material` null takes the paint off",
        paint,
    ),
    edit(
        "parameter",
        "Project Parameters",
        &[],
        "{name, kind?: text|number|length|yesno, categories?: [wall|floor|ceiling|roof|door|window|room|column|component|stair|\
         ramp|railing|mass, …], rename?, delete?: true} a project parameter, as Revit's Manage > Project Parameters makes one: a value \
         of its kind each element of its categories may have of its own, set with `modify {id, params: {name: value}}` (lengths \
         in mm, yes or no as true or false). Off a category, that category's elements lose their values of it; deleted, every \
         element does; renamed, they follow it. `inspect` lists them",
        parameter,
    ),
    edit(
        "family",
        "Load Family",
        &[],
        "{name, category?: furniture|casework|plumbing|lighting|generic|door|window, parameters?: [{name, kind?: length|number|\
         angle|yesno|material, instance?, value?, formula?}], types?: {type: {parameter: value}}, forms?: [{plane?: xy|xz|yz, \
         profile?: [[a, b], …], circle?: {centre: [a, b], radius}, axis?: [[a, b], [a, b]], from?, to, material?, visible?, \
         void?}], symbols?: [{line?: [[x, y], …] | arc?: {centre: [x, y], radius, from?, to}, visible?}], rename?, delete?: \
         true, path?, save?} a family of \
         the project's, BIMCraft's own parametric component: created, changed, or loaded from the .json file at `path` (a family \
         as `inspect` shows it, with its `name`). Its forms are profiles drawn in its own axes (x along a component's angle, y \
         to its left, z up from its base) and swept: `xy` up from `from` to `to`, `xz` along y, `yz` along x. With an `axis`, a \
         line in that plane, the profile is turned about it instead, `from` to `to` degrees (up to 360, right-handed from the \
         axis' first point toward its second; it can't cross the axis): a pipe, a bowl, a lamp shade. A `void` is no solid: it \
         cuts the solid forms listed before it that it passes through, a hole, a notch, a sink's bowl, and is never seen \
         itself (forms listed after it are whole). A `door` or `window` family stands in a wall, placed with `door` or `window` \
         and a `family`: its forms are drawn in the opening's axes (x along the wall from the middle of the opening, y across \
         it, 0 in the middle of the wall and + toward its exterior, z up from the sill), it must have length parameters \
         `Width` and `Height` (the opening it makes; `Sill Height` is a window's default sill), and its formulas read \
         `Wall Thickness`, the wall's. Its `symbols` are the lines its plan draws (a door's swing: a `line`, an `arc` in \
         degrees from +x counter-clockwise). Sizes are mm, or \
         formulas over its parameters as Revit's are: `Width / 2`, `if(Depth > 600, 4, 2)`, with + - * / ^, < > = <= >=, if, \
         and, or, not, min, max, abs, sqrt, round, roundup, rounddown, sin, cos, tan (degrees). A parameter is each type's, or \
         with `instance` each component's. `types` merges into its types (a null takes one away); `parameters` and `forms` \
         replace its own; `material` is a material's name or a material parameter's; `visible` a yes/no parameter or formula. \
         `inspect` lists the families",
        family,
    ),
    edit(
        "component",
        "Component",
        &["CM"],
        "{family, type?, at: [x,y], level?, angle?, offset?, mirrored?, params?: {parameter: value}} a family's type placed, its \
         first by default: at a point on a level, turned `angle` degrees counter-clockwise, `offset` mm up from the level, and \
         with `mirrored` flipped across its own x axis; `params` sets its instance parameters (`modify` merges into them, a null \
         taking one back to the family's)",
        component,
    ),
    edit(
        "flip",
        "Flip",
        &[],
        "{ids: [wall,…]} turns walls round about their location lines, as Revit's flip arrows do: their exterior sides go to the \
         other side, their doors and windows stay where they are",
        flip,
    ),
    edit(
        "modify",
        "Modify",
        &[],
        "{id | ids: [id,…], …fields} change fields as `elements` shows them, of one element or several; `level` and `top_level` may be names. \
         A filled region's or a floor's model pattern is set by `pattern_origin` [x, y], plan mm, where its lines start, and \
         `pattern_angle`, degrees counter-clockwise (both shown by `elements`); to put a pattern line of one on a reference \
         line, Model::align_pattern gives the `pattern_origin` that does. A floor's, a ceiling's, a roof's or a shaft's \
         outline it sets is held to what `floor` says of it",
        modify,
    ),
    edit(
        "match_properties",
        "Match Properties",
        &["MA"],
        "{from: id, ids: [id,…]} gives the elements in `ids` the type of `from` (and, for components, its family), as Revit's Match \
         Properties does; only elements of `from`'s category take it, the rest come back in `skipped`. Returns {modified, skipped}",
        match_properties,
    ),
    edit(
        "move",
        "Move",
        &["MV"],
        "{ids: [id,…], by: [dx, dy], end?: start|end} moves elements in plan; walls joined to a moved wall stretch to stay \
         joined, and doors and windows slide along their walls. With `end`, only that end of one wall moves",
        move_elements,
    ),
    edit(
        "copy",
        "Copy",
        &["CO"],
        "{ids: [id,…], by: [dx, dy]} copies elements, with the doors and windows of copied walls; returns the copies' ids",
        copy,
    ),
    edit(
        "rotate",
        "Rotate",
        &["RO"],
        "{ids: [id,…], about: [x, y], angle, copy?} turns elements about a point by `angle` degrees, counter-clockwise; walls \
         joined to them stay joined, and doors and windows turn with their walls",
        rotate,
    ),
    edit(
        "mirror",
        "Mirror",
        &["MM", "DM"],
        "{ids: [id,…], axis: [[x, y], [x, y]], copy?} mirrors elements across the line through two points; mirrored doors \
         and windows face the other way",
        mirror,
    ),
    edit(
        "corner",
        "Trim/Extend to Corner",
        &["TR"],
        "{ids: [a, b], keep?: [[x, y], [x, y]]} trims or extends two walls, or two detail lines (lines or arcs), to meet where \
         their lines (or circles) cross; `keep` is a point on the part of each to keep",
        corner,
    ),
    edit(
        "offset",
        "Offset",
        &["OF"],
        "{id, distance, toward: [x, y], copy?} a detail line drawn again `distance` mm beside itself on the side `toward` is, \
         arcs round the same centres, an ellipse or a spline as a spline through points within 0.05 mm of the true offset \
         (which Revit can't): a copy (the default), or with `copy` false moved",
        offset,
    ),
    edit(
        "trim_extend",
        "Trim/Extend Single Element",
        &[],
        "{boundary, ids: [id, …], keep: [[x, y], …]} trims or extends each detail line of `ids` (lines and arcs) to where it \
         meets `boundary` (a detail line, a wall's location line or a grid), keeping the part of each its `keep` point is on: \
         Revit's Trim/Extend Single Element, and Multiple Elements with several",
        trim_extend,
    ),
    edit(
        "split",
        "Split",
        &["SL"],
        "{id, at: [x, y]} splits a wall or a detail line in two at `at`: a wall's doors and windows go with their part; a \
         detail line of any shape splits where it passes nearest `at` (a spline by its control points exactly, one through \
         points into two through points within 0.05 mm of it), and a closed one (a circle, a whole ellipse, a closed \
         polyline) opens there instead, into one that starts and ends at `at`",
        split,
    ),
    edit(
        "join_polyline",
        "Join into Polyline",
        &["PJ"],
        "{ids: [id, …]} joins detail lines of one view, lines, arcs and polylines that meet end to end, into one polyline in \
         the first's line style (closed when they come round); the first keeps its id, the rest are deleted, and a dimension \
         that measured one of the rest loses that reference (and is deleted, left with fewer than two): the reply's `notes` \
         say which",
        join_polyline,
    ),
    edit(
        "fillet",
        "Fillet Arc",
        &["FL"],
        "{ids: [a, b], radius} rounds the corner where two straight detail lines, or two straight walls, meet (or would, \
         extended) with an arc of `radius` mm, in the first's line style or of the first's type: each is cut back to the arc",
        fillet,
    ),
    edit(
        "vertex",
        "Add/Delete Vertex",
        &[],
        "{id, at: [x, y], delete?} a detail line (a polyline, or a spline of either kind) with a vertex more where it passes \
         nearest `at` (a spline by its control points gains a control point and keeps its shape exactly), or with `delete` \
         one fewer: the corner, control point or point passed through nearest `at`; `at` is within 100 mm of the line (of a \
         vertex, to delete one), or it is refused",
        vertex,
    ),
    edit(
        "convert_spline",
        "Convert Spline",
        &[],
        "{id} a detail line that is a spline through points becomes the same curve by its control points, exactly; one by \
         its control points becomes a spline through points within 0.05 mm of it",
        convert_spline,
    ),
    edit(
        "explode",
        "Explode Polyline",
        &["EX"],
        "{id} takes a detail line that is a polyline apart into a line or an arc for each segment, in its view and style; \
         the first keeps its id. Revit has no shortcut for it: EX is ours, PX being the Plumbing Fixture's on the desktop",
        explode,
    ),
    edit(
        "delete",
        "Delete",
        &["DE"],
        "{ids: [id,…]} deleting a wall deletes its doors and windows, deleting those their tags; a view takes its notes and \
         spot elevations, and what a spot elevation reads takes it; dimensions lose what's deleted",
        delete,
    ),
    edit(
        "line_pattern",
        "Line Patterns",
        &[],
        "{name?, segments?: [{dash: mm} | {space: mm} | \"dot\", …], rename?, delete?: true} a line pattern of the project's: \
         a dash or a dot then a space, again and again, each dash and space 0.05 to 100 mm long on paper, so it reads the \
         same at any scale. A project starts with Dash, Long Dash, Dot, Dash Dot, Hidden and Centre; Solid is a line with \
         none. `rename` renames it, the line styles using it following; `delete` takes it away, while no style uses it. \
         Without `name`, it lists every pattern",
        line_pattern,
    ),
    edit(
        "line_style",
        "Line Styles",
        &[],
        "{name?, pen?, color?: \"#rrggbb\" | [r, g, b], pattern?: name | \"Solid\", rename?, delete?: true} a line style of \
         the project's, as Revit's: the pen (0, a hairline, to 16) its lines draw in, their colour (black unless told) and \
         their line pattern. A new one needs `pen`; one changed keeps what it isn't given. A project starts with Hairline, \
         Thin Lines, Medium Lines, Wide Lines, Hidden Lines, Centre Lines and Overhead Lines. `rename` renames it, the lines \
         drawn in it following; `delete` takes it away, while no line is drawn in it. Without `name`, it lists every style",
        line_style,
    ),
    edit(
        "fill_pattern",
        "Fill Patterns",
        &[],
        "{name?, target?: drafting|model, lines?: [{angle, spacing, origin?: [x, y], shift?, dashes?: [mm, -mm, …]}, …], rename?, \
         delete?: true} a fill pattern of the project's, as Revit's (and a PAT file's): families of parallel lines, each at \
         `angle` degrees through `origin`, its lines `spacing` apart and each `shift` along from the last, whole or dashed \
         (`dashes`: a dash's length, a space's as a negative one, 0 a dot); no lines, it fills solid. A drafting pattern's \
         lengths are mm on paper, the same at any scale; a model pattern's mm in the building. A project starts with Solid \
         fill, Diagonal Up and Down, Crosshatch, Diagonal Crosshatch, Horizontal, Vertical, Sand, Brick 225 x 75 and Tile 300. \
         `rename` renames it, the region types filling with it following; `delete` takes it away while no type uses it. \
         Without `name`, it lists every pattern",
        fill_pattern,
    ),
    edit(
        "linework",
        "Linework",
        &["LW"],
        "{view?, id, at: [x, y], style? | invisible?: true, remove?: true} Revit's Linework: the edge of element `id` that a \
         plan (`view`, a level's or a plan view; the lowest level's by default) draws nearest `at` (within 200 mm), drawn in \
         that view only in line style `style`, or not at all with `invisible`; `remove` takes the Linework nearest `at` off \
         that edge again (one edge, even where another lies within reach; where that edge has none, the nearest entry within \
         reach on an edge the element no longer draws, one a call). A view holds at most 2000 edges. It returns the \
         edge, `from` and `to`, or with `remove` how many were `removed`",
        linework,
    ),
    edit(
        "import_fill_patterns",
        "Import Fill Patterns",
        &[],
        "{path | text, replace?: false} the fill patterns of a PAT file (AutoCAD's and Revit's), from the file at `path` or \
         as `text`: each `*Name` and its families of lines, in mm unless the file says `;%UNITS=INCH`, drafting unless a \
         pattern says `;%TYPE=MODEL`. A pattern the project already has is left as it is unless `replace`; one that can't \
         be a pattern (no lines, an offset of 0, dashes that are all spaces, too many families) is left out. A file is read \
         up to its first 2000 patterns. It returns those `imported`, those `left_out` (the first 100, each with why) and \
         `left_out_count`, all of them",
        import_fill_patterns,
    ),
    other(
        "export_fill_patterns",
        "Export Fill Patterns",
        "{path?, names?: [name, …]} the project's fill patterns (those `names` says, else all but the solid ones, which a PAT \
         file can't hold) as a PAT file in mm, written to `path`, or returned as `text` without it",
        export_fill_patterns,
    ),
    edit(
        "filled_region_type",
        "Filled Region Types",
        &[],
        "{name?, foreground?: pattern | null, foreground_color?, background?: pattern | null, background_color?, masking?, \
         rename?, delete?: true} a filled region type of the project's: its foreground pattern over its background one, each \
         in its colour (\"#rrggbb\" or [r, g, b], black unless told), over the paper when `masking` (it hides what's under \
         it). A new one starts with none. A project starts with Solid Black, Diagonal Up, Crosshatch, Sand, Brick and Masking. \
         `rename` renames it, its regions following; `delete` takes it away while no region is of it. Without `name`, it \
         lists every type",
        filled_region_type,
    ),
    edit(
        "filled_region",
        "Filled Region",
        &["FR"],
        "{sketch: [[[x,y],[x,y]],…], view?, type?, line_style?: name | null} a filled region in a view (a plan, a section, an \
         elevation or a sheet, in its own terms; the lowest plan by default): the loops its sketch closes into (see \
         `sketch`: any number of outlines, a loop inside one a hole, one inside that an island), filled as type `type` \
         (Solid Black unless told) says, its boundary in line style `line_style` (Thin Lines unless told; null, unseen: a \
         masking region's way). It draws over the model, under the annotation; edit its sketch with edit_boundary. A model \
         pattern starts at the region's `pattern_origin` and turns by its `pattern_angle` (see modify)",
        filled_region,
    ),
    edit(
        "create_group",
        "Create Group",
        &["GP"],
        "{ids, name?} a detail group made of detail lines, filled regions and text notes of one view, as Revit's Create \
         Group: they become type `name`'s members (Group 1, 2… unless told), and one group of it takes their place, its \
         origin the middle of them. Place it again with detail_group; ungroup gives its members back. Returns {id, name}",
        create_group,
    ),
    edit(
        "detail_group",
        "Place Detail Group",
        &[],
        "{type, at: [x,y], angle?, mirrored?, view?} a detail group of type `type` placed in a view (the lowest plan by \
         default): its members drawn with their origin at `at`, turned `angle` degrees counter-clockwise, mirrored across \
         their x axis first when `mirrored`. Move, copy, rotate and mirror take it whole",
        detail_group,
    ),
    edit(
        "repeating_detail",
        "Repeating Detail",
        &[],
        "{type, start: [x,y], end: [x,y], spacing, mirrored?, view?} detail group `type` repeated along the line from \
         `start` to `end`, every `spacing` mm from `start` (at most 2000 of them), each turned along the line, as Revit's \
         Repeating Detail Component: brick coursing, blocking, batt ties. A view draws up to 10000 group members in all (a \
         repeat places each member of the type again), so a type of 100 members repeats up to 100 times. Returns {id, count}",
        repeating_detail,
    ),
    edit(
        "ungroup",
        "Ungroup",
        &["UG"],
        "{id} the members of detail group `id` as detail lines, filled regions and text notes of its view again, where it \
         drew them; the group goes, its type stays. Returns {ids}",
        ungroup,
    ),
    edit(
        "group_type",
        "Detail Groups",
        &[],
        "{name?, rename?, delete?: true} a detail group type: `rename` renames it, its groups following; `delete` takes it \
         away while none of it is placed. Without `name`, it lists each with its members and how many are placed",
        group_type,
    ),
    edit(
        "detail_line",
        "Detail Line",
        &["DL"],
        "{view?, style?, and a curve: `points` [[x, y], …] (with `arcs`, each segment's turn in degrees counter-clockwise, \
         and `closed`), `start` and `end` (with `arc` degrees or a `through` point), `centre` and `radius` (a circle), \
         `centre` and `radii` [a, b] (an ellipse, with `angle` degrees and `span` [from, to] degrees for part of one), \
         `control` [[x, y], …] (a spline by its control points, with `knots`), or `fit` [[x, y], …] (a spline through them, \
         with `start_tangent` and `end_tangent`)} a detail line in a view, by name or id, in the view's own terms: a \
         level's plan (the lowest by default; plan mm), a section or an elevation (along its line from its start, and up) \
         or a sheet by number (paper mm). It draws in line style `style` (Thin Lines unless told), only in its view, and \
         goes with it; with `insulation`, a width in mm, it draws as batt insulation along its curve, as Revit's Insulation; \
         with `centre_mark` true, a small cross marks the centre of its arcs, circle or ellipse; with `cloud`, an arc length \
         in mm, it draws as a revision cloud's scallops along it (outward round a loop, a scallop ending at each corner), of \
         `revision` (any text, a label kept with the cloud and listed by `elements`; nothing else reads it); a line is \
         insulation or a cloud, not both. An \
         arc, a circle or an ellipse lies within 1,000 km of the origin (an arc of 359.9999999999° is a circle of radius \
         5.7e14 mm, and is refused), and a polyline's arcs take no more than 8,192 points to draw within 0.25 mm: 10,000 \
         arcs of 175° are refused",
        detail_line,
    ),
    edit(
        "delete_type",
        "Delete Type",
        &[],
        "{category?: wall|floor|door|window|column|roof|ceiling|railing|…, family?, type?} deletes a type of a category \
         (`category` and `type`), a family (`family`) or one of a family's types (`family` and `type`), as Revit's Delete on one \
         in the Project Browser does; refused while an element uses it (it says how many), and for the last type of a \
         category or of a family",
        delete_type,
    ),
    edit(
        "purge",
        "Purge Unused",
        &["PU"],
        "{kinds?: [type|family|material|picture|filter|template, …], dry_run?} takes out what nothing uses, again for what that \
         leaves unused: types and families unless `kinds` says; a category keeps a type and a family a type; with `dry_run` it \
         lists what would go and changes nothing",
        purge,
    ),
    edit(
        "line_weights",
        "Line Weights",
        &[],
        "{model?: [{scale, widths}, …], annotation?: [16 widths], reset?: true} the project's line weights, as Revit's \
         Manage > Line Weights: what each pen draws on paper, mm. `model` gives, whole, the model's pens 1 to 16 at each of \
         its scales (1 to 32 of them, 1 : `scale`): a view draws its walls, doors, patterns and the rest of the model in the \
         column of the scale nearest its own (by how many times finer or coarser; the finer of two as near). `annotation` \
         gives the 16 pens that dimensions, tags, text, grids, levels and marks draw in, at any scale. Each width is 0.01 to \
         20 mm; pen 0 is always a hairline. `reset` goes back to what a project in its units starts with (the model's pens \
         wider at finer scales; at 1 : 100, pen 4 is 0.18 mm and pen 10 0.35 mm). Without any, it says what they are, each \
         scale named as the project's units name it, and `own` whether the project has set them",
        line_weights,
    ),
    edit(
        "object_styles",
        "Object Styles",
        &[],
        "{categories?: {category: {projection?: pen, cut?: pen}, …}, reset?: true} the project's object styles, as Revit's \
         Manage > Object Styles: the pen (0, a hairline, to 16) each model category draws its lines in where it is seen \
         (`projection`) and where a view cuts it (`cut`), in every plan and section; a view's Visibility/Graphics overrides \
         them. A category given keeps the pen it isn't given. The rest of an element's lines keep their own: a compound \
         wall's layers, a door's jambs, a stair's walk line. `reset` goes back to what a project starts with (cut pen 10 and \
         seen pen 4; a door's and a window's cut 6, a railing seen 6, a mass 4 both). Without either, it says each model \
         category's, `own` whether the project has set it apart",
        object_styles,
    ),
    edit(
        "units",
        "Project Units",
        &["UN"],
        "{length?: mm|cm|m|in|ft|ft-in} how drawings, schedules and the desktop show lengths, and read typed ones: \
         millimetres, centimetres, metres, fractional inches, decimal feet, or feet and fractional inches (to 1/16\"); areas \
         in m², or ft² with inches and feet. The model, and every command, keep millimetres",
        units,
    ),
    edit(
        "language",
        "Project Language",
        &[],
        "{code?: en|pt|es} the language the project's drawings speak (a stair's UP, a title block's SCALE, a schedule's \
         headings) and new names come in (levels, sections, elevations, sheets, rooms, schedules): English, Brazilian \
         Portuguese or Spanish. What's already named stays named",
        language,
    ),
    other("inspect", "Inspect", "{} levels, element counts, types, warnings and undo history", inspect),
    other(
        "render",
        "Render",
        "{view?: a level's, section's or elevation's name or id, a sheet's number, or {3D}, ceiling?, width?, height?, path?, \
         azimuth?, elevation?} a level's floor plan (with `ceiling`, its reflected ceiling plan, looking up from 2300 mm), a section, an elevation, a sheet (at 5 px a paper mm), or the 3D view seen from \
         `azimuth` degrees round from east (counter-clockwise) and `elevation` degrees up (by default from the south-east, \
         30° up), as a PNG (pngBase64), or written to a path: .png, .svg for a plan, .pdf for a sheet. A plan, section, \
         elevation or sheet that could not draw something as asked (a fill pattern too dense for the view, more ink behind \
         a region than is kept above it) says so in the reply's `notes`",
        render_view,
    ),
    edit(
        "schedule",
        "Schedule",
        &[],
        "{name?, category?, fields?, filters?, sort?, grouped?, path?} a schedule view, as Revit's View > Schedules makes one, \
         and its table: columns, rows, the `elements` each row lists, and totals (and a .csv file with a path). A new one needs `name` and `category`: rooms, \
         doors, windows, walls, floors, ceilings, roofs, columns, structural columns, footings, beams, strip footings, components, \
         stairs, ramps, railings or levels; `category` \
         alone reads its own (Room Schedule, Door Schedule, Window Schedule, Wall Schedule, Floor Schedule, Level Schedule, \
         each the project's once changed). `fields`: its columns, in order, each a field's name (`available` lists them: its \
         category's own, Count, project parameters, a component's family's parameters) or {name, formula?, kind?: \
         number|length|area|yesno, heading?, total?}; with a formula, a calculated field worked out from the row's other \
         fields as a family's formulas are (lengths in mm, areas in m²; `Area * 120`); `total` sums it in a totals row. \
         `filters`: [{field, op: equals|not-equals|greater|greater-or-equal|less|less-or-equal|contains|not-contains|\
         begins-with|not-begins-with|ends-with|not-ends-with|has-value|has-no-value, value}, …], the elements all of them \
         keep (text in any case, numbers as the schedule shows them, an unset yes or no as no). `sort`: [field or {field, descending?}, …], in turn (its first column unless it says; Phase Created \
         and Phase Demolished in the project's order of phases, a phase never demolished after the last); \
         `grouped`, a row for each set of the sort fields' values: Count and totals summed, and a field whose values differ \
         left empty. Each one given replaces the last; `fields: []` goes back to its category's",
        schedule,
    ),
    edit(
        "graphics",
        "Visibility/Graphics",
        &[],
        "{view?: a level's, section's or elevation's name or id, or {3D} (the lowest level's plan unless given), hide?: [category, \
         …], show?: […], hide_elements?: [id, …], show_elements?: […], categories?: {category: override | null, …}, elements?: \
         {id: override | null, …}, filters?: [{filter, hidden?, projection?, cut?, halftone?, transparency?}, …], phase?: name \
         | null, phase_filter?: name | null, options?: {set: option | null, …}, template?: name | null, reset?: true} what a view hides and how it draws the rest, as Revit's Visibility/Graphics does; with \
         none of those, the view's settings as they are. A category hidden (walls, floors, rooms, grids, dimensions, levels…, \
         in any case, singular or plural) leaves all its elements out of that view, and `hide_elements` the elements given. An \
         override is {hidden?, projection?: style, cut?: style, halftone?, transparency?}, a style {color?, weight? (a pen, \
         0 to 16: 0 a hairline, the thinnest line the screen or printer draws at any scale; 1 to 16 widths on paper from 0.10 to \
         4 mm, 4 being 0.18 mm and 10 0.35 mm, a cut wall's outline), dash? (solid, dashed, dotted), fill?}, a colour [r, g, b] or \"#rrggbb\". \
         Projection is what a plan sees below its cut plane, a section beyond its line and the annotation; cut is what the view \
         cuts; `fill` is the cut's poché, or a 3D view's surface; in a 3D view, and in what a section sees beyond its line, a \
         projection's `color` is the colour of the edges (weight and dash draw lines, and those are vectors). Halftone draws it \
         halfway to the paper; transparency (percent) sees through it in a 3D view. Halftone and transparency only add: a \
         filter or an element can lighten what its category doesn't, not take its category's away. Overrides merge into the last (null takes a field or the override away); an element's own \
         beats the filters applied (the first prevailing), which beat the category's. `filters` replaces the list applied; \
         `template` follows a view template; `reset` takes away all of the view's own settings. `phase` is the phase the view \
         shows (the project's last unless it says) and `phase_filter` how it shows each status of what's in it (see `phase`); \
         `options` chooses the design option it shows of a set (the set's primary unless it says; see `option`)",
        view_graphics,
    ),
    edit(
        "section_box",
        "Section Box",
        &[],
        "{view?: a 3D view's name or id ({3D} unless given), on?: true | false (true unless given), min?: [x, y, z], max?: \
         [x, y, z]} the box a 3D view is cut to, as Revit's Section Box: six planes square to the axes, keeping what lies \
         between `min` and `max` (mm; each at least 10 mm past the other on every axis), the model's surfaces and its point \
         clouds alike, so a scan's floors and ceilings can be cut away to show its walls. What it cuts away isn't drawn or \
         picked. A corner not given stays where it was; turned on with none, it's fitted round the model, 300 mm clear of \
         it. `on: false` turns it off and keeps its extents for when it's turned on again",
        section_box,
    ),
    edit(
        "filter",
        "View Filter",
        &[],
        "{name, categories?: [category, …], rules?: [{field, op, value}, …], delete?: true} a view filter, as Revit's parameter \
         filters are: the elements of its categories (any a schedule can be of: walls, doors, rooms, components…) that hold \
         every rule, a schedule filter's (`field` of the category, `op`: equals, not-equals, greater, greater-or-equal, less, \
         less-or-equal, contains, not-contains, begins-with, not-begins-with, ends-with, not-ends-with, has-value, \
         has-no-value; `value`), and every element of them without rules. A view applies it with `graphics {filters}`, \
         hiding what it picks out or drawing it another way. Gone, views stop applying it. Replies with the filter and the ids \
         it picks out; `name` alone reads one",
        view_filter,
    ),
    edit(
        "template",
        "View Template",
        &[],
        "{name, from?: a level's, section's, elevation's or 3D view's name or id, controls?: [categories, filters, range, \
         phase, options, scale, detail, style], view_range?: {top?, cut?, bottom?, depth?}, phase?, phase_filter?, options?, \
         scale?: 1 | … | 100000, detail?: coarse | medium | fine, visual_style?: shaded | hidden_line | wireframe, \
         hide?, show?, categories?, filters?, reset?, delete?: true} a view template, as Revit's: settings under a name for \
         views to follow with `graphics {template}`. `from` takes a view's categories (hidden and overridden), filters and, a \
         plan's, view range, its phase and phase filter, its design options, its scale, detail level and visual style; \
         `controls` is what views following it get from it (all eight unless it says) and cannot change for themselves: its \
         categories, its filters, a plan's view range, its phase with its phase filter, its design options, its scale (`scale` \
         command), its detail level (`detail`), its visual style (`visual_style`). `hide`, `show`, `categories`, `filters`, \
         `phase`, `phase_filter`, `options`, `scale`, `detail`, `visual_style` and `reset` change it as they change a view; \
         `view_range` merges into its range. Changed, every view following it follows. Gone, they keep their own settings. \
         `name` alone reads one",
        view_template,
    ),
    edit(
        "phase",
        "Phasing",
        &[],
        "{name?, before?, after?, rename?, delete?, into?, now?, overrides?} the project's phases, in order, as Revit's \
         Phasing does: a project starts with Existing and New Construction. `name` alone adds a phase at the end (or `before` \
         or `after` another); with `rename`, renames it (its elements, views and templates follow); with `delete`, takes it \
         out, its elements made and taken down in `into` instead (needed while anything holds it). An element is made in a \
         phase (its `Phase Created` parameter, the first unless it says) and may be taken down in a later one (`Phase \
         Demolished`), set with `modify {params}`; `now` (a phase, or null) is the phase what this session's commands make \
         is made in (the first phase leaves no value). A view shows a phase and a phase filter (`graphics {phase, \
         phase_filter}`): what's made after it isn't there, what's taken down before it neither, and the rest is new (made in \
         it), existing, demolished (taken down in it) or temporary (both), each shown as its phase filter says (see \
         `phase_filter`). `overrides` ({new|existing|demolished|temporary: override | null}, merged: an override as \
         `graphics` has it, without hidden) is how a phase filter draws what it says overridden; the first two statuses \
         start halftone and dashed red. Replies with the phases, `now` and the overrides",
        view_phases,
    ),
    edit(
        "phase_filter",
        "Phase Filter",
        &[],
        "{name, new?, existing?, demolished?, temporary?, delete?} a phase filter: how a view shows each status of what's in \
         its phase, each `by-category` (as its category and the view's overrides say), `overridden` (as the project's \
         overrides for that status say) or `not-displayed`. A project has Show All, Show Complete, Show New, Show Previous + \
         Demolished and Show Previous + New; any may change. A new one starts by category throughout. One in use by a view or a \
         template can't be deleted. Replies with the filter",
        view_phase_filter,
    ),
    edit(
        "option",
        "Design Options",
        &[],
        "{set?, options?, primary?, option?, rename?, delete?, accept?, now?} sets of design options, as Revit's Design \
         Options has them: `set` with `options` ([names]; no colons) makes a set or changes its options (one with elements in \
         it stays), `primary` is the option a view shows unless it chooses (the first by default). An element is in an option \
         by its `Design Option` parameter, `set: option`, set with `modify {params}` (a door or window with none is in its wall's); \
         it shows only in a view that shows that option: `graphics {options: {set: option | null}}`. `now` (`set: option`, or \
         null) is the option what this session's commands make is made in. `rename` renames the set, or with `option`, one of \
         its options (elements and views follow). `accept` keeps the primary option's elements in the model and deletes the \
         others' (and the set); `delete` deletes the set and every element in its options. Replies with the set (`deleted` \
         lists the elements that went)",
        design_options,
    ),
    Command {
        id: "measure",
        label: "Measure",
        aliases: &["ME"],
        params: "{from: [x, y], to: [x, y]} what a ruler laid between two points reads (mm): its `length`, its parts `dx` and `dy`, \
                 and its `angle` in degrees from east, counter-clockwise; it changes nothing",
        run: measure,
        undoable: false,
    },
    other(
        "library",
        "Library",
        "{kind?, name?} the project's library with its ids and GUIDs, as elements have them: each type (`kind` \
         `type:wall`, `type:door`, `type:beam`…), material, picture, family, parameter, view filter and template \
         (`view-filter`, `view-template`), phase, phase filter, design option set and design option (`option-set`, \
         `design-option`, named `set: option`), family type and family parameter (`family-type`, `family-parameter`, named \
         `family: name`); and `parts`, what entries hold that has its own id and GUID: each type's layers and each family's \
         forms and symbols (`kind` `layer`, `form`, `symbol`; `name` the type's or the family's). Wherever a command takes \
         an entry by name, its GUID does too",
        library,
    ),
    other(
        "select_by_id",
        "Select by ID",
        "{text} the elements a list names, each by its id, its GUID (dashes and braces or not) or its IFC GlobalId, \
         separated by spaces, commas, semicolons or lines: `ids` found, each once in the order named, `missing` (what named \
         none), and `plan` and `extent` ([[x, y], [x, y]] mm), the level's plan that shows them and where; it selects nothing \
         itself, a front end does",
        select_by_id,
    ),
    other(
        "ids_of",
        "IDs of Selection",
        "{ids: [id or GUID, …]} what names each element: its `id`, its `guid`, its `ifc` GlobalId, its `category` and its \
         `name` (or type, number, mark)",
        ids_of,
    ),
    other(
        "elements",
        "Elements",
        "{category?, level?, limit?, offset?} elements with their `id` (six digits, what commands take) and `guid` (made once, \
         never changed), fields and derived values (length, height, area_m2…); what an element points at (its level, host, \
         view…) it holds by GUID. Wherever a command takes an element, its id or its GUID will do",
        elements,
    ),
];

// ---------- parameters ----------

fn bad(cmd: &str, msg: impl std::fmt::Display) -> String {
    format!("{cmd}: {msg}")
}

fn absent(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

/// `[x, y]` in mm, finite and within the model's limits.
fn point(v: &Value) -> Option<P2> {
    let p = match v.as_array()?.as_slice() {
        [x, y] => P2(x.as_f64()?, y.as_f64()?),
        _ => return None,
    };
    (p.is_finite() && p.0.abs() <= MAX_COORD && p.1.abs() <= MAX_COORD).then_some(p)
}

fn point_opt(c: &str, p: &Value, key: &str) -> Result<Option<P2>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => point(v).map(Some).ok_or_else(|| bad(c, format!("`{key}` must be a point [x, y] in mm, got {v}"))),
        None => Ok(None),
    }
}

fn point_req(c: &str, p: &Value, key: &str) -> Result<P2, String> {
    point_opt(c, p, key)?.ok_or_else(|| bad(c, format!("`{key}` [x, y] is required")))
}

fn points(c: &str, p: &Value, key: &str) -> Result<Vec<P2>, String> {
    let a = p.get(key).and_then(Value::as_array).ok_or_else(|| bad(c, format!("`{key}` [[x, y], …] is required")))?;
    if a.len() > MAX_POINTS {
        return Err(bad(c, format!("`{key}` has more than {MAX_POINTS} points")));
    }
    a.iter().map(|v| point(v).ok_or_else(|| bad(c, format!("`{key}`: {v} is not a point [x, y] in mm")))).collect()
}

fn num(c: &str, p: &Value, key: &str) -> Result<Option<f64>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && x.abs() <= MAX_COORD)
            .map(Some)
            .ok_or_else(|| bad(c, format!("`{key}` must be a number in mm, got {v}"))),
        None => Ok(None),
    }
}

fn flag(c: &str, p: &Value, key: &str) -> Result<bool, String> {
    match p.get(key) {
        v if absent(v) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(v) => Err(bad(c, format!("`{key}` must be true or false, got {v}"))),
        None => Ok(false),
    }
}

fn string(c: &str, p: &Value, key: &str) -> Result<Option<String>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(Value::String(s)) if s.chars().count() <= MAX_TEXT => Ok(Some(s.clone())),
        Some(v) => Err(bad(c, format!("`{key}` must be text of at most {MAX_TEXT} characters, got {v}"))),
        None => Ok(None),
    }
}

/// An element of `m` by its id, or its GUID.
fn element_id(c: &str, m: &Model, v: &Value) -> Result<Id, String> {
    let g = serde_json::from_value::<Guid>(v.clone()).map_err(|_| bad(c, format!("{v} is not an element id or GUID")))?;
    m.elements.id(g).ok_or_else(|| bad(c, format!("no element {g}")))
}

/// `ids` [id, …], or one `id`: elements of the model, by id or GUID, each once, in the order given.
fn ids_param(c: &str, m: &Model, p: &Value) -> Result<Vec<Id>, String> {
    let as_id = |v: &Value| element_id(c, m, v);
    let ids: Vec<Id> = match (p.get("ids"), p.get("id")) {
        (Some(Value::Array(a)), _) if a.len() <= MAX_POINTS => a.iter().map(as_id).collect::<Result<_, _>>()?,
        (Some(Value::Array(_)), _) => return Err(bad(c, format!("`ids` lists more than {MAX_POINTS} elements"))),
        (v, Some(one)) if absent(v) => vec![as_id(one)?],
        _ => return Err(bad(c, "`ids` [id, …] is required")),
    };
    if let Some(missing) = ids.iter().find(|id| !m.elements.contains_key(id)) {
        return Err(bad(c, format!("no element {missing}")));
    }
    let mut seen = BTreeSet::new();
    Ok(ids.into_iter().filter(|id| seen.insert(*id)).collect())
}

/// `key`, an element by its id or its GUID, if given: as its GUID, or as the id given until the model resolves it.
fn id_opt(c: &str, p: &Value, key: &str) -> Result<Option<Guid>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => {
            serde_json::from_value(v.clone()).map(Some).map_err(|_| bad(c, format!("`{key}` must be an element id or GUID, got {v}")))
        }
        None => Ok(None),
    }
}

fn count_opt(c: &str, p: &Value, key: &str, default: usize) -> Result<usize, String> {
    match p.get(key) {
        v if absent(v) => Ok(default),
        Some(v) => {
            v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(c, format!("`{key}` must be a whole number, got {v}")))
        }
        None => Ok(default),
    }
}

/// `level` by name, id or GUID, as its GUID; the lowest level when not given. A plan view of a level names the level too.
fn level_param(c: &str, m: &Model, p: &Value, key: &str) -> Result<Guid, String> {
    plan_param(c, m, p, key).map(|(level, _)| level)
}

/// The plan `key` names, by name, id or GUID: a level's own plan (the lowest level's when not given), or a plan view of one. It
/// gives the level's GUID, and the plan view's when it is one: what an annotation says it is in.
fn plan_param(c: &str, m: &Model, p: &Value, key: &str) -> Result<(Guid, Option<Guid>), String> {
    let levels = m.levels();
    let id = match p.get(key) {
        v if absent(v) => levels.first().map(|(id, _)| *id).ok_or_else(|| bad(c, "the model has no levels"))?,
        Some(Value::String(n)) if Guid::parse(n).is_none() => levels
            .iter()
            .find(|(_, l)| l.name == *n)
            .map(|(id, _)| *id)
            .or_else(|| m.elements.iter().find_map(|(id, e)| matches!(e, Element::PlanView(v) if v.name == *n).then_some(*id)))
            .ok_or_else(|| {
                let names: Vec<&str> = levels.iter().map(|(_, l)| l.name.as_str()).collect();
                bad(c, format!("no level or plan view named `{n}` (the levels are: {})", names.join(", ")))
            })?,
        Some(v) => serde_json::from_value::<Guid>(v.clone())
            .ok()
            .and_then(|g| m.elements.id(g))
            .filter(|&id| m.plan_level(id).is_some())
            .ok_or_else(|| bad(c, format!("`{key}`: {v} is not a level or a plan view")))?,
        None => return Err(bad(c, "the model has no levels")),
    };
    let level = m.plan_level(id).map(|l| m.guid(l)).unwrap_or_default();
    Ok((level, m.plan_view(id).map(|_| m.guid(id))))
}

/// `view` by name, id or GUID: a level's plan, a section, an elevation, a schedule or a 3D view, or with `sheets` a sheet
/// by its number; when not given, the plan of `level` (the lowest by default).
fn view_param(c: &str, m: &Model, p: &Value, sheets: bool) -> Result<Id, String> {
    let plan = || plan_param(c, m, p, "level").map(|(level, view)| m.elements.id(view.unwrap_or(level)).unwrap_or_default());
    match p.get("view") {
        v if absent(v) => plan(),
        Some(Value::String(n)) if Guid::parse(n).is_none() => m
            .elements
            .iter()
            .find_map(|(id, e)| {
                (m.view_name(*id) == Some(n.as_str()) || sheets && matches!(e, Element::Sheet(sh) if sh.number == *n)).then_some(*id)
            })
            .ok_or_else(|| bad(c, format!("no view named `{n}`"))),
        Some(v) => serde_json::from_value::<Guid>(v.clone())
            .ok()
            .and_then(|g| m.elements.id(g))
            .ok_or_else(|| bad(c, format!("`view`: {v} is not a view"))),
        None => plan(),
    }
}

/// `type`, or the category's default ([`Model::default_type`]) when not given. [`Model::check`]
/// reports a name that doesn't exist, listing the ones that do.
fn type_param(c: &str, p: &Value, m: &Model, category: &str) -> Result<String, String> {
    match string(c, p, "type")? {
        Some(n) => Ok(n),
        None => m.default_type(category).map(String::from).ok_or_else(|| bad(c, "the model has no types for this")),
    }
}

/// `walls` → `wall`: categories in any case, singular or plural.
pub(crate) fn category(s: &str) -> String {
    // `structural column` and `structural_column` are `structuralcolumn`.
    let s: String = s.to_ascii_lowercase().chars().filter(char::is_ascii_alphanumeric).collect();
    // `mass` ends in the plural's letter.
    if s == "mass" || s == "masses" { "mass".into() } else { s.trim_end_matches('s').to_string() }
}

// ---------- derived values ----------

/// mm² as m², to the hundredth.
pub fn m2(mm2: f64) -> f64 {
    (mm2 / 1e4).round() / 100.0
}

/// A stair's or a ramp's runs, each from its foot to its top, and where its last one ends.
fn runs(extra: &mut Map<String, Value>, l: &stairs::Layout) {
    let runs: Vec<[P2; 2]> = l.runs.iter().map(|r| [micron(r.start), micron(r.end())]).collect();
    extra.insert("end".into(), json!(runs.last().map(|r| r[1])));
    extra.insert("runs".into(), json!(runs));
}

/// The area of the rings together, mm².
fn area(rings: &[Vec<P2>]) -> f64 {
    rings.iter().map(|r| geom::area(r).abs()).sum()
}

fn hundredth(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn level_name(m: &Model, id: impl Key) -> String {
    m.level(id).map_or_else(|| format!("level {}", m.shown(id)), |l| l.name.clone())
}

/// How far along a wall's location line the point nearest `p` lies, in mm.
fn along(w: &Wall, p: P2) -> f64 {
    Axis::of(w).project(p).0
}

/// How far `p` is from a wall's centre line.
fn off_centre(m: &Model, w: &Wall, p: P2) -> f64 {
    Axis::of(w).distance(p, walls::centre(m, w))
}

/// The wall a plan of `level` cuts nearest `at`, within its half thickness plus 100 mm of its centre
/// line, and how far along it `at` lies.
fn wall_at(m: &Model, level: impl Key, at: P2) -> Option<(Guid, f64)> {
    m.elements
        .with_guids()
        .filter_map(|(_, id, e)| match e {
            Element::Wall(w) if m.cut_by(w, level) => Some((id, w, off_centre(m, w, at))),
            _ => None,
        })
        .filter(|(_, w, d)| *d <= m.thickness(w) / 2.0 + 100.0)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(id, w, _)| (id, along(w, at)))
}

/// The outline `floor` gives a floor on `level` by default, along the centre lines of the walls
/// that bound its rooms: the outer outline of the largest piece they make, counter-clockwise; and
/// its courtyards, clockwise from their south-west corners: the loops of those walls standing free
/// inside it, joined to none of the outer walls, with no room in them and in no other loop. A loop
/// joined to the outer walls, or with a room in it, is rooms.
pub fn floor_outline(m: &Model, level: impl Key) -> Option<(Vec<P2>, Vec<Vec<P2>>)> {
    let level = m.guid(level);
    // Each piece's outline, clockwise, the largest first.
    let mut pieces: Vec<Vec<P2>> = geom::faces(&m.bounding_lines(level), TOL).into_iter().filter(|f| geom::area(f) < -TOL * TOL).collect();
    pieces.sort_by(|a, b| geom::area(a).total_cmp(&geom::area(b)));
    let (outer, inner) = pieces.split_first()?;
    // The loops standing free inside the outline: the other pieces, inside it. Whether loop `a`
    // stands inside another; the innermost loop round a point, and so the loops rooms are in.
    let loops: Vec<&Vec<P2>> = inner.iter().filter(|f| f.iter().all(|q| geom::contains(outer, *q))).collect();
    let nested = |a: usize| (0..loops.len()).any(|b| b != a && loops[a].iter().all(|q| geom::contains(loops[b], *q)));
    let around = |p: P2| {
        (0..loops.len()).filter(|&a| geom::contains(loops[a], p)).max_by(|&a, &b| geom::area(loops[a]).total_cmp(&geom::area(loops[b])))
    };
    let indoors: Vec<usize> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Room(r) if r.level == level => around(r.point),
            _ => None,
        })
        .collect();
    // Courtyards: the outermost loops, outdoors.
    let courtyards = (0..loops.len()).filter(|&a| !nested(a) && !indoors.contains(&a)).map(|a| tidy(loops[a])).collect();
    Some((outer.iter().rev().copied().collect(), courtyards))
}

/// A ring by its corners, to the micron, from its south-west corner.
pub(crate) fn tidy(ring: &[P2]) -> Vec<P2> {
    let mut ring: Vec<P2> = geom::corners(ring).into_iter().map(micron).collect();
    let south_west =
        ring.iter().enumerate().min_by(|(_, a), (_, b)| (a.1, a.0).partial_cmp(&(b.1, b.0)).unwrap_or(std::cmp::Ordering::Equal));
    let first = south_west.map_or(0, |(i, _)| i);
    ring.rotate_left(first);
    ring
}

/// One past the largest number among `taken` ("1" when there is none).
pub(crate) fn next_number<'a>(taken: impl Iterator<Item = &'a str>) -> String {
    taken.filter_map(|t| t.trim().parse::<u64>().ok()).max().unwrap_or(0).saturating_add(1).to_string()
}

/// Revit-style grid naming: the letter after the last grid's letter, else the next number.
fn next_grid_name(m: &Model) -> String {
    let names: Vec<&str> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Grid(g) => Some(g.name.as_str()),
            _ => None,
        })
        .collect();
    let mut last = names.last().map_or("", |n| n).chars();
    let letter = match (last.next(), last.next()) {
        (Some(ch), None) if ch.is_ascii_uppercase() && ch < 'Z' => Some(char::from(ch as u8 + 1).to_string()),
        _ => None,
    };
    match letter {
        Some(l) if !names.contains(&l.as_str()) => l,
        _ => next_number(names.iter().copied()),
    }
}

/// An element as agents read it: its fields (what it points at by GUID) plus its `id`, its `guid` and derived values.
pub fn element_json(m: &Model, id: Id) -> Value {
    let Some(e) = m.elements.get(id) else { return Value::Null };
    let mut extra = Map::new();
    extra.insert("id".into(), json!(id));
    extra.insert("guid".into(), json!(m.guid(id)));
    // Its type's and its family's GUIDs, beside their names.
    e.clone().lib_refs_mut(&mut |kind, name| {
        let key = match kind {
            LibKind::Type(_) => "type_guid",
            LibKind::Family => "family_guid",
            _ => return,
        };
        if let Some(g) = m.library.guid(kind, name) {
            extra.insert(key.into(), json!(g));
        }
    });
    match e {
        Element::PointCloud(pc) => {
            if let Some(cloud) = m.clouds.get(&pc.cloud) {
                extra.insert("points".into(), json!(cloud.len()));
                extra.insert("source".into(), json!(cloud.source()));
                if !cloud.stations().is_empty() {
                    extra.insert("stations".into(), json!(cloud.stations()));
                }
            }
        }
        Element::Wall(w) => {
            let axis = Axis::of(w);
            extra.insert("length".into(), json!(hundredth(axis.length() * 1e4) / 1e4));
            extra.insert("height".into(), json!(hundredth(m.top_z(w) - m.base_z(w))));
            extra.insert("thickness".into(), json!(m.thickness(w)));
            if let Some(r) = axis.radius() {
                extra.insert("radius".into(), json!(hundredth(r)));
            }
        }
        Element::Door(o) | Element::Window(o) => {
            if let (Some(t), Some(w)) = (m.opening_type(e), m.wall(o.host)) {
                extra.insert("width".into(), json!(t.width));
                extra.insert("height".into(), json!(t.height));
                extra.insert("level".into(), json!(w.level));
                extra.insert("at".into(), json!(m.anchor(id)));
                // Of a family: every parameter's value, worked out.
                if let Some(v) = o
                    .family
                    .as_ref()
                    .and_then(|n| m.families.get(n))
                    .and_then(|f| f.values_in(&o.type_name, &o.params, m.thickness(w)).ok())
                {
                    extra.insert("values".into(), values_json(v));
                }
            }
        }
        Element::Dimension(d) => {
            extra.insert("values".into(), json!(m.dimension_values(d).into_iter().map(hundredth).collect::<Vec<_>>()));
        }
        // Every parameter's value, worked out, and how high it reaches over its base.
        Element::Component(cp) => {
            if let Some(v) = m.families.get(&cp.family).and_then(|f| f.values(&cp.type_name, &cp.params).ok()) {
                extra.insert("values".into(), values_json(v));
            }
            let base = m.elevation(cp.level) + cp.offset;
            let top = m.component_pieces(cp).iter().map(|p| p.top).fold(base, f64::max);
            extra.insert("height".into(), json!(hundredth(top - base)));
        }
        Element::Column(col) | Element::StructuralColumn(col) | Element::Footing(col) => {
            let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
            extra.insert("height".into(), json!(top - bottom));
            if let Some(t) = m.column_type(e) {
                extra.insert("width".into(), json!(t.width));
                extra.insert("depth".into(), json!(t.depth));
                extra.insert("round".into(), json!(t.round));
            }
        }
        // Its section, how long it runs along its axis, and the heights of its centre line's ends.
        crate::model::run_arm!(_) => {
            if let (Some(t), Some(g)) = (m.run_type(e), m.run_geom(e)) {
                extra.insert("width".into(), json!(t.width));
                extra.insert("depth".into(), json!(t.depth));
                extra.insert("round".into(), json!(t.round));
                extra.insert("length".into(), json!(hundredth(g.length())));
                extra.insert("axis".into(), json!([g.a, g.b].map(|p| p.map(hundredth))));
            }
        }
        Element::Stair(st) => {
            let (n, each, length) = m.flight(st);
            let (bottom, top) = m.span(st.level, 0.0, st.top_level, st.height);
            extra.insert("height".into(), json!(top - bottom));
            extra.insert("risers".into(), json!(n));
            extra.insert("riser_height".into(), json!(hundredth(each)));
            extra.insert("length".into(), json!(length));
            runs(&mut extra, &stairs::stair(m, st));
        }
        Element::Ramp(r) => {
            let (bottom, top) = m.span(r.level, 0.0, r.top_level, r.height);
            let l = stairs::ramp(m, r);
            extra.insert("height".into(), json!(top - bottom));
            extra.insert("length".into(), json!(hundredth(l.runs.iter().map(|r| r.length).sum())));
            runs(&mut extra, &l);
        }
        Element::Railing(rl) => {
            let path: Vec<P2> = stairs::railing(m, rl).into_iter().map(|(p, _)| micron(p)).collect();
            extra.insert("path".into(), json!(path));
            extra.insert("level".into(), json!(m.level_of(e)));
            extra.insert("height".into(), json!(m.types.railing.get(&rl.type_name).map(|t| t.height)));
        }
        Element::Roof(r) => {
            let faces = m.roof_faces(r).unwrap_or_default();
            let ridge = faces
                .iter()
                .flat_map(|(loops, p)| loops.iter().flatten().map(|q| p.at(*q)))
                .fold(m.elevation(r.level) + r.offset, f64::max);
            let outline = m.roof_outline(r);
            let courtyards: f64 = r.courtyards.iter().map(|c| geom::area(c).abs()).sum();
            extra.insert("ridge".into(), json!(hundredth(ridge)));
            extra.insert("area_m2".into(), json!(m2(geom::area(&outline).abs() - courtyards)));
            extra.insert("outline".into(), json!(outline.into_iter().map(micron).collect::<Vec<_>>()));
        }
        Element::Sheet(sh) => {
            extra.insert("paper".into(), json!(paper(&sh.size)));
            let on: Vec<Id> =
                m.elements.iter().filter(|(_, e)| matches!(e, Element::Viewport(v) if m.same(v.sheet, id))).map(|(id, _)| *id).collect();
            extra.insert("viewports".into(), json!(on));
        }
        Element::Viewport(v) => {
            extra.insert("title".into(), json!(m.view_name(v.view)));
            extra.insert("size".into(), json!(sheet::size(m, v.view, v.scale)));
        }
        Element::Section(v) | Element::Elevation(v) => {
            let l = v.looking();
            extra.insert("length".into(), json!(v.start.dist(v.end)));
            // `+ 0.0`: no `-0`.
            extra.insert("looking".into(), json!([l.0 + 0.0, l.1 + 0.0]));
        }
        Element::Tag(t) => {
            extra.insert("text".into(), json!(m.tag_text(t)));
            extra.insert("level".into(), json!(m.level_of(e)));
            extra.insert("at".into(), json!(m.anchor(t.host).map(|a| a + t.offset)));
        }
        // Areas less the openings and shafts through them.
        Element::Floor(f) => {
            let left: f64 = m.floor_slabs(f).iter().map(|(face, rise)| area(&m.less_shafts(face, *rise).0)).sum();
            extra.insert("area_m2".into(), json!(m2(left)));
            extra.insert("thickness".into(), json!(m.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness)));
            // How high its top is, at its lowest and its highest.
            let top = m.floor_top(f);
            let (lo, hi) =
                f.boundary.iter().map(|q| top.at(*q)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)));
            extra.insert("top".into(), json!([hundredth(lo), hundredth(hi)]));
            // Where its top's model pattern starts and how far it's turned: shown at their defaults too, to be set by modify.
            extra.insert("pattern_origin".into(), json!(f.pattern_origin));
            extra.insert("pattern_angle".into(), json!(f.pattern_angle));
        }
        Element::Ceiling(cl) => {
            let left: f64 = m.ceiling_slabs(cl).iter().map(|(face, rise)| area(&m.less_shafts(face, *rise).0)).sum();
            extra.insert("area_m2".into(), json!(m2(left)));
            if let Some(t) = m.types.ceiling.get(&cl.type_name) {
                extra.insert("thickness".into(), json!(t.thickness));
                extra.insert("grid".into(), json!(t.grid));
            }
        }
        Element::Shaft(sh) => {
            let (bottom, top) = m.span(sh.level, sh.base_offset, sh.top_level, sh.height);
            extra.insert("height".into(), json!(top - bottom));
            extra.insert("area_m2".into(), json!(m2(geom::area(&sh.boundary).abs())));
        }
        Element::Mass(ms) => {
            let (area, height, volume) = mass_size(m, ms);
            extra.insert("area_m2".into(), json!(area));
            extra.insert("height".into(), json!(height));
            extra.insert("volume_m3".into(), json!(volume));
            let gross: f64 = m.mass_slices(ms).iter().map(|(_, z, _)| geom::area(&m.mass_outline(ms, *z)).abs()).sum();
            extra.insert("gross_floor_area_m2".into(), json!(m2(gross)));
            extra.insert("levels".into(), json!(m.mass_slices(ms).iter().map(|(l, _, _)| *l).collect::<Vec<Id>>()));
        }
        Element::Room(r) => {
            let region = m.room_region(r);
            extra.insert("enclosed".into(), json!(region.is_some()));
            extra.insert("area_m2".into(), json!(region.as_ref().map_or(0.0, |g| m2(g.area()))));
            extra.insert("perimeter_m".into(), json!(region.as_ref().map_or(0.0, |g| (g.perimeter() / 10.0).round() / 100.0)));
        }
        Element::Text(_) => {
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::DetailGroup(g) => {
            extra.insert("members".into(), json!(m.detail_groups.get(&g.type_name).map_or(0, |t| t.members.len())));
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::RepeatingDetail(r) => {
            extra.insert("count".into(), json!(r.places().map_or(0, |v| v.len())));
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::FilledRegion(r) => {
            let area = sketch::solve(&r.sketch).map_or(0.0, |p| p.area());
            extra.insert("area_m2".into(), json!(m2(area)));
            extra.insert("level".into(), json!(m.level_of(e)));
            extra.insert("pattern_origin".into(), json!(r.pattern_origin));
            extra.insert("pattern_angle".into(), json!(r.pattern_angle));
        }
        Element::DetailLine(l) => {
            let length = l.curve.length();
            extra.insert("shape".into(), json!(l.curve.kind()));
            extra.insert("length".into(), json!(hundredth(length)));
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::Spot(sp) => {
            extra.insert("elevation".into(), json!(m.spot_value(sp).map(hundredth)));
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::Link(l) => {
            extra.insert("loaded".into(), json!(m.loaded.contains_key(&l.path)));
            extra.insert("elements".into(), json!(m.loaded.get(&l.path).map(|o| o.elements.len())));
        }
        Element::Level(_) | Element::Grid(_) | Element::Schedule(_) | Element::View3D(_) | Element::PlanView(_) => {}
    }
    let mut v = serde_json::to_value(e).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.extend(extra);
    }
    v
}

// ---------- files and history ----------

/// Project files this large or larger are refused.
const MAX_FILE: u64 = 256 << 20;

fn ext(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn new(s: &mut Session, p: &Value) -> R {
    let c = "new";
    let language = match string(c, p, "language")? {
        Some(code) => language_param(c, &code)?,
        None => Language::English,
    };
    let name = string(c, p, "name")?.unwrap_or_else(|| language.word("Project").into());
    s.reset(Model::new_in(&name, language), None);
    Ok(json!({"name": name, "language": language.code()}))
}

fn language_param(c: &str, code: &str) -> Result<Language, String> {
    Language::from_code(code)
        .ok_or_else(|| bad(c, format!("`{code}` is no language here; there are en (English), pt (Português) and es (Español)")))
}

fn language(s: &mut Session, p: &Value) -> R {
    let c = "language";
    if let Some(code) = string(c, p, "code")? {
        let l = language_param(c, &code)?;
        if l != s.model.language {
            s.model_mut().language = l;
        }
    }
    Ok(json!({"language": s.model.language.code()}))
}

/// The most fields `open` names as left out of a file; it says how many there were in all.
const MAX_LEFT_OUT: usize = 100;

fn open(s: &mut Session, p: &Value) -> R {
    let reply = open_file(s, p)?;
    Ok(with_links(s, reply))
}

/// `reply` of an open, with the links that couldn't be read once the project is read.
fn with_links(s: &mut Session, mut reply: Value) -> Value {
    let failed = relink(s);
    if !failed.is_empty() {
        reply["links_not_loaded"] = json!(failed);
    }
    reply
}

/// `open`, but for what its links read.
fn open_file(s: &mut Session, p: &Value) -> R {
    let c = "open";
    let path = PathBuf::from(string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is required"))?);
    let size = crate::disk::len(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
    if size >= MAX_FILE {
        return Err(bad(c, format!("{} is too large ({size} bytes)", path.display())));
    }
    // IFC comes in as a new project: saving it asks where.
    if ext(&path) == "ifc" {
        let bytes = crate::disk::read(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        let stem = path.file_stem().map_or_else(|| "Project".into(), |s| s.to_string_lossy().into_owned());
        let i = crate::import::ifc(&bytes, &stem).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for e in i.model.elements.values() {
            *counts.entry(e.category()).or_default() += 1;
        }
        let reply =
            json!({"path": path, "name": i.model.name, "format": i.schema, "elements": counts, "skipped": i.skipped, "moved": i.moved});
        s.reset(i.model, None);
        return Ok(reply);
    }
    let text = crate::disk::read_to_string(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
    let (mut m, left_out) = Model::read(&text).map_err(|e| bad(c, format!("{} is not a BIMCraft project: {e}", path.display())))?;
    m.adopt_materials();
    // A project parameter named as a phase or a design option is held under is from before they were: it keeps its values, renamed.
    let mut renamed = BTreeMap::new();
    if m.parameters.keys().any(|n| phasing::reserved(n) || paint::reserved(n) || massing::reserved(n)) {
        let mut t = Session::new();
        t.model = Arc::new(m);
        let olds: Vec<String> =
            t.model.parameters.keys().filter(|n| phasing::reserved(n) || paint::reserved(n) || massing::reserved(n)).cloned().collect();
        for old in olds {
            let new = (1..)
                .map(|i| if i == 1 { format!("{old} Parameter") } else { format!("{old} Parameter {i}") })
                .find(|n| !t.model.parameters.contains_key(n))
                .unwrap_or_default();
            parameter(&mut t, &json!({"name": old, "rename": new})).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
            renamed.insert(old, new);
        }
        m = Arc::unwrap_or_clone(t.model);
    }
    m.sync_library();
    m.check().map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
    let name = m.name.clone();
    s.reset(m, Some(path.clone()));
    let mut reply = json!({"path": path, "name": name});
    if !renamed.is_empty() {
        reply["renamed"] = json!(renamed);
    }
    if !left_out.is_empty() {
        // A hostile file may name a million keys: the reply lists the first few and counts them all.
        reply["left_out"] = json!(left_out.iter().take(MAX_LEFT_OUT).collect::<Vec<_>>());
        reply["left_out_count"] = json!(left_out.len());
    }
    Ok(reply)
}

fn select_by_id(s: &mut Session, p: &Value) -> R {
    let c = "select_by_id";
    let text = match p.get("text") {
        Some(Value::String(t)) => t.clone(),
        Some(Value::Array(a)) if a.len() <= crate::inquiry::MAX_NAMES => {
            a.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), String::from)).collect::<Vec<_>>().join(" ")
        }
        _ => return Err(bad(c, "`text`, the ids, GUIDs or IFC GlobalIds to find, is required")),
    };
    let found = crate::inquiry::find(&s.model, &text);
    let shown = crate::inquiry::shown(&s.model, &found.ids);
    Ok(json!({
        "ids": found.ids,
        "library": found.library,
        "missing": found.missing,
        "plan": shown.map(|(level, _)| level),
        "extent": shown.map(|(_, [lo, hi])| [micron(lo), micron(hi)]),
    }))
}

fn library(s: &mut Session, p: &Value) -> R {
    let c = "library";
    let kind = string(c, p, "kind")?;
    let name = string(c, p, "name")?;
    let rows: Vec<Value> = s
        .model
        .library
        .entries()
        .filter(|(_, e)| kind.as_ref().is_none_or(|k| e.kind.to_string() == *k))
        .filter(|(_, e)| name.as_ref().is_none_or(|n| e.name == *n))
        .map(|(id, e)| json!({"id": id, "guid": e.guid, "kind": e.kind, "name": e.name}))
        .collect();
    // The parts: layers, a family's forms and symbols, each with its id and GUID.
    let parts: Vec<Value> = s
        .model
        .library
        .parts()
        .filter(|(_, x)| kind.as_ref().is_none_or(|k| x.what == k.as_str()))
        .filter(|(_, x)| name.as_ref().is_none_or(|n| x.owner == *n))
        .map(|(id, x)| json!({"id": id, "guid": x.guid, "kind": x.what, "of": x.of, "index": x.index}))
        .collect();
    Ok(json!({"count": rows.len(), "library": rows, "parts": parts}))
}

fn ids_of(s: &mut Session, p: &Value) -> R {
    let c = "ids_of";
    let m = &s.model;
    let list = match (p.get("ids"), p.get("id")) {
        (Some(Value::Array(a)), _) if a.len() <= MAX_POINTS => a.clone(),
        (v, Some(one)) if absent(v) => vec![one.clone()],
        _ => return Err(bad(c, "`ids` [id or GUID, …] is required")),
    };
    // An element or a library entry, by id or GUID; a library entry's GUID comes as its name (`Session::execute`).
    let one = |v: &Value| {
        let word = v.as_str().map_or_else(|| v.to_string(), String::from);
        crate::inquiry::one(m, &word)
            .or_else(|| crate::inquiry::library(m, &word))
            .or_else(|| m.library.entries().find(|(_, e)| e.name == word).map(|(id, _)| id))
    };
    let ids =
        list.iter().map(|v| one(v).ok_or_else(|| bad(c, format!("no element or library entry {v}")))).collect::<Result<Vec<_>, _>>()?;
    let rows: Vec<Value> = ids
        .iter()
        .filter_map(|id| crate::inquiry::names(&s.model, *id))
        .map(|n| json!({"id": n.id, "guid": n.guid, "ifc": n.ifc, "category": n.category, "name": n.name}))
        .collect();
    Ok(json!({"elements": rows}))
}

fn insert_from(s: &mut Session, p: &Value) -> R {
    let c = "insert";
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is required"))?;
    // Read as `open` reads it, in a session of its own.
    let mut other = Session::new();
    open_file(&mut other, &json!({"path": path})).map_err(|e| bad(c, e.trim_start_matches("open: ")))?;
    let ids: Vec<Id> = match p.get("ids") {
        v if absent(v) => Vec::new(),
        Some(_) => ids_param(c, &other.model, p)?,
        None => Vec::new(),
    };
    let made = crate::insert::insert(s.model_mut(), &other.model, &ids).map_err(|e| bad(c, e))?;
    let there: BTreeMap<String, Id> = made.ids.iter().map(|(a, b)| (a.to_string(), *b)).collect();
    Ok(json!({"ids": there, "copies": made.copies, "library": made.library, "left_out": made.left_out}))
}

/// The project a link's `path` names, read as `open` reads it: its own links are not followed.
fn read_link(s: &Session, path: &str) -> Result<Arc<Model>, String> {
    let at = PathBuf::from(path);
    let full = match (&s.path, at.is_relative()) {
        (Some(project), true) => project.parent().map_or(at.clone(), |dir| dir.join(&at)),
        _ => at,
    };
    let mut other = Session::new();
    open_file(&mut other, &json!({"path": full})).map_err(|e| e.trim_start_matches("open: ").to_string())?;
    Ok(other.model)
}

/// What every link of the project reads, again; the links that couldn't be read, as `{name, error}`.
fn relink(s: &mut Session) -> Vec<Value> {
    let links: Vec<(Id, String, String)> = s
        .model
        .elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Link(l) => Some((*id, l.name.clone(), l.path.clone())),
            _ => None,
        })
        .collect();
    let mut failed = Vec::new();
    for (_, name, path) in links {
        match read_link(s, &path) {
            Ok(m) => {
                Arc::make_mut(&mut s.model).loaded.insert(path, m);
            }
            Err(error) => {
                Arc::make_mut(&mut s.model).loaded.remove(&path);
                failed.push(json!({"name": name, "error": error}));
            }
        }
    }
    failed
}

fn link_model(s: &mut Session, p: &Value) -> R {
    let c = "link";
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is required"))?;
    let other = read_link(s, &path).map_err(|e| bad(c, e))?;
    let name = match string(c, p, "name")? {
        Some(n) => n,
        None => Path::new(&path).file_stem().map_or_else(|| "Link".into(), |f| f.to_string_lossy().into_owned()),
    };
    let at = point_opt(c, p, "at")?.unwrap_or(P2(0.0, 0.0));
    let angle = num(c, p, "angle")?.unwrap_or(0.0);
    let elements = other.elements.len();
    let m = s.model_mut();
    let id = m.insert(Element::Link(crate::model::Link { name: name.clone(), path: path.clone(), at, angle }));
    m.loaded.insert(path, other);
    Ok(json!({"id": id, "name": name, "elements": elements}))
}

fn reload(s: &mut Session, p: &Value) -> R {
    let c = "reload";
    let only = id_opt(c, p, "id")?;
    if let Some(id) = only
        && !matches!(s.model.elements.get(id), Some(Element::Link(_)))
    {
        return Err(bad(c, format!("element {id} is not a link")));
    }
    let want: Vec<(String, String)> = s
        .model
        .elements
        .iter()
        .filter(|(id, _)| only.is_none_or(|o| s.model.same(**id, o)))
        .filter_map(|(_, e)| match e {
            Element::Link(l) => Some((l.name.clone(), l.path.clone())),
            _ => None,
        })
        .collect();
    let (mut reloaded, mut failed) = (Vec::new(), Vec::new());
    for (name, path) in want {
        match read_link(s, &path) {
            Ok(m) => {
                Arc::make_mut(&mut s.model).loaded.insert(path, m);
                reloaded.push(name);
            }
            Err(error) => failed.push(json!({"name": name, "error": error})),
        }
    }
    Ok(json!({"reloaded": reloaded, "failed": failed}))
}

fn save(s: &mut Session, p: &Value) -> R {
    let c = "save";
    let path = match string(c, p, "path")? {
        Some(p) => PathBuf::from(p),
        None => s.path.clone().ok_or_else(|| bad(c, "`path` is required: this project hasn't been saved yet"))?,
    };
    let format = ext(&path);
    if format == "ifc" {
        crate::disk::write(&path, crate::ifc::export(&s.model)).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        return Ok(json!({"path": path, "format": "IFC4"}));
    }
    if format == "pdf" {
        let seen = crate::link::seen(&s.model);
        let m = &*seen;
        let mut sheets: Vec<(&str, Drawing, P2)> = m
            .elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::Sheet(sh) => Some((sh.number.as_str(), sheet::draw(m, *id)?, paper(&sh.size)?)),
                _ => None,
            })
            .collect();
        if sheets.is_empty() {
            return Err(bad(c, "there are no sheets to print; make one with `sheet` and `place` views on it"));
        }
        sheets.sort_by(|a, b| a.0.cmp(b.0));
        let pages: Vec<(&Drawing, P2)> = sheets.iter().map(|(_, d, p)| (d, *p)).collect();
        crate::disk::write(&path, pdf::pdf(&pages)).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        return Ok(json!({"path": path, "format": "PDF", "pages": pages.len()}));
    }
    if matches!(format.as_str(), "svg" | "png") {
        let level = level_param(c, &s.model, &json!({}), "view")?;
        write_plan(&render::plan(&crate::link::seen(&s.model), level, 1600, 1000), &path)?;
        return Ok(json!({"path": path, "format": format}));
    }
    let text = s.model.to_json().map_err(|e| bad(c, e))?;
    crate::disk::replace(&path, &text).map_err(|e| bad(c, e))?;
    s.path = Some(path.clone());
    Ok(json!({"path": path, "format": "bimcraft"}))
}

fn autosave(s: &mut Session, p: &Value) -> R {
    let path = PathBuf::from(string("autosave", p, "path")?.ok_or_else(|| bad("autosave", "`path` is required"))?);
    if s.path.as_ref() == Some(&path) {
        return Err(bad("autosave", "the recovery file must differ from the saved project"));
    }
    let text = s.model.to_json().map_err(|e| bad("autosave", e))?;
    if text.len() as u64 >= MAX_FILE {
        return Err(bad("autosave", "the recovery snapshot exceeds the project file size limit"));
    }
    crate::autosave::atomic_write(&path, &text).map_err(|e| bad("autosave", e))?;
    Ok(json!({"path": path, "format": "bimcraft"}))
}

fn recover(s: &mut Session, p: &Value) -> R {
    // Parse all params before open resets the session, so invalid input cannot partially recover.
    let project = string("recover", p, "project")?.map(PathBuf::from);
    let result = open_file(s, p)?;
    // Its links are read from where the project lives, not from where the backup does.
    s.path = project;
    Ok(with_links(s, result))
}

/// Standard base64, for images in JSON replies.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]) } else { '=' });
        }
    }
    out
}

fn write_plan(d: &render::Drawing, path: &Path) -> Result<(), String> {
    let bytes = match ext(path).as_str() {
        "svg" => render::svg(d).into_bytes(),
        "png" => render::png(d)?,
        other => return Err(format!("render: write a .png or .svg, not `.{other}`")),
    };
    crate::disk::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// A picture that only comes as PNG (the 3D view, a section): written to `path`, a .png, when
/// there is one, else returned in base64; `info` says what it is.
fn png_reply(c: &str, p: &Value, png: Vec<u8>, mut info: Value) -> R {
    match string(c, p, "path")? {
        Some(path) if ext(Path::new(&path)) == "png" => {
            crate::disk::write(&path, png).map_err(|e| bad(c, format!("{path}: {e}")))?;
            info["path"] = json!(path);
        }
        Some(path) => return Err(bad(c, format!("only plans make SVGs; save this view as .png, not as {path}"))),
        None => info["pngBase64"] = json!(base64(&png)),
    }
    Ok(info)
}

fn render_view(s: &mut Session, p: &Value) -> R {
    let c = "render";
    // What its links build is in the picture.
    let seen = crate::link::seen(&s.model);
    let m = &*seen;
    let side = |key: &str, default: usize| count_opt(c, p, key, default).map(|n| n.clamp(16, 8192) as u32);
    let (w, h) = (side("width", 1600)?, side("height", 1000)?);
    let (w3, h3) = (w.min(view3d::MAX_SIDE), h.min(view3d::MAX_SIDE));
    let view = p.get("view");
    if matches!(view, Some(Value::String(v)) if v.eq_ignore_ascii_case("{3D}") || v.eq_ignore_ascii_case("3D")) {
        let (azimuth, elevation) = (num(c, p, "azimuth")?.unwrap_or(view3d::AZIMUTH), num(c, p, "elevation")?.unwrap_or(view3d::ELEVATION));
        let scene = view3d::Scene::new(m);
        let look = Look::of_3d(m);
        let png = scene.render_in(&scene.fit_in(azimuth, elevation, w3, h3, &look), &[], &look).png().map_err(|e| bad(c, e))?;
        return png_reply(
            c,
            p,
            png,
            json!({"width": w3, "height": h3, "view": "{3D}", "kind": "3D view", "azimuth": azimuth, "elevation": elevation}),
        );
    }
    // A section or elevation, by name or id.
    let named = |id: &Id, v: &Section| view.and_then(Value::as_str) == Some(v.name.as_str()) || view.and_then(Value::as_u64) == Some(*id);
    let cut = m.elements.iter().find_map(|(id, e)| match e {
        Element::Section(v) | Element::Elevation(v) if named(id, v) => Some((*id, v.name.clone(), e.category())),
        _ => None,
    });
    if let Some((id, name, kind)) = cut {
        let (png, notes) = render::section_png_noted(m, id, render::section_fit(m, id, w3, h3)).map_err(|e| bad(c, e))?;
        return png_reply(c, p, png, with_notes(json!({"width": w3, "height": h3, "view": name, "kind": kind}), &notes));
    }
    // A sheet, by number, name or id, unless a level is called that.
    let level = match view {
        None => true,
        Some(Value::String(n)) => m.levels().iter().any(|(_, l)| l.name == *n),
        Some(v) => v.as_u64().is_some_and(|id| m.level(id).is_some()),
    };
    let named = |id: &Id, sh: &Sheet| {
        view.and_then(Value::as_str).is_some_and(|n| n == sh.number || n == sh.name) || view.and_then(Value::as_u64) == Some(*id)
    };
    let paper_of = m.elements.iter().find_map(|(id, e)| match e {
        Element::Sheet(sh) if !level && named(id, sh) => Some((*id, sh.number.clone(), paper(&sh.size)?)),
        _ => None,
    });
    if let Some((id, number, size)) = paper_of {
        let d = sheet::draw(m, id).ok_or_else(|| bad(c, format!("sheet {number} can't be drawn")))?;
        let info = with_notes(json!({"width": d.view.width, "height": d.view.height, "view": number, "kind": "sheet"}), &d.notes);
        if let Some(path) = string(c, p, "path")?.filter(|path| ext(Path::new(path)) == "pdf") {
            crate::disk::write(&path, pdf::pdf(&[(&d, size)])).map_err(|e| bad(c, format!("{path}: {e}")))?;
            return Ok(with_notes(json!({"path": path, "view": info["view"], "kind": "sheet"}), &d.notes));
        }
        return png_reply(c, p, render::png(&d)?, info);
    }
    let level = level_param(c, m, p, "view")?;
    let (d, kind) = match flag(c, p, "ceiling")? {
        true => (render::ceiling_plan(m, level, w, h), "ceiling plan"),
        false => (render::plan(m, level, w, h), "plan"),
    };
    match string(c, p, "path")? {
        Some(path) => {
            let path = PathBuf::from(path);
            write_plan(&d, &path)?;
            Ok(with_notes(json!({"path": path, "width": w, "height": h, "view": level_name(m, level), "kind": kind}), &d.notes))
        }
        None => Ok(with_notes(
            json!({"pngBase64": base64(&render::png(&d)?), "width": w, "height": h, "view": level_name(m, level), "kind": kind}),
            &d.notes,
        )),
    }
}

/// `reply` with what the drawing could not draw as asked (`notes`), when it could not.
fn with_notes(mut reply: Value, notes: &[String]) -> Value {
    if !notes.is_empty() {
        reply["notes"] = json!(notes);
    }
    reply
}

fn schedule(s: &mut Session, p: &Value) -> R {
    let c = "schedule";
    let m = &s.model;
    let name = string(c, p, "name")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let cat = match string(c, p, "category")? {
        Some(k) if !schedule::categories().contains(&category(&k).as_str()) => {
            return Err(bad(c, format!("`{k}` isn't a category schedules are of; there are: {}", schedule::categories().join(", "))));
        }
        k => k.map(|k| category(&k)),
    };
    let builtin = schedule::builtin(name.as_deref(), cat.as_deref());
    let found = name.as_deref().and_then(|n| schedule::named(m, n)).or_else(|| builtin.and_then(|(_, n)| schedule::named(m, n)));
    let mut sc = match (found, builtin, &name, &cat) {
        (Some(id), ..) => match m.elements.get(id) {
            Some(Element::Schedule(sc)) => sc.clone(),
            _ => return Err(bad(c, format!("element {id} isn't a schedule"))),
        },
        (None, Some((of, n)), _, _) => Schedule { name: m.language.word(n).to_string(), of: of.to_string(), ..Schedule::default() },
        (None, None, Some(n), Some(k)) => Schedule { name: n.clone(), of: k.clone(), ..Schedule::default() },
        (None, None, Some(n), None) => return Err(bad(c, format!("there's no schedule `{n}`; a new one needs `category` too"))),
        (None, None, None, Some(k)) => Schedule { of: k.clone(), ..Schedule::default() },
        (None, None, None, None) => return Err(bad(c, "`name` or `category` is required")),
    };
    if let Some(k) = cat.as_deref().filter(|k| *k != category(&sc.of)) {
        return Err(bad(c, format!("`{}` schedules {}s, not {k}s; make another for those", sc.name, sc.of)));
    }
    // Each given, replaced whole: a field or a sort field by its name alone, or in full.
    let named = |key: &str, by: &str| -> Result<Option<Value>, String> {
        match p.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Array(list)) => {
                Ok(Some(Value::Array(list.iter().map(|v| if v.is_string() { json!({by: v}) } else { v.clone() }).collect())))
            }
            Some(v) => Err(bad(c, format!("`{key}` is a list, not {v}"))),
        }
    };
    fn read<T: serde::de::DeserializeOwned>(key: &str, v: Value) -> Result<T, String> {
        serde_json::from_value(v).map_err(|e| bad("schedule", format!("`{key}`: {e}")))
    }
    let mut changed = false;
    if let Some(v) = named("fields", "name")? {
        (sc.fields, changed) = (read("fields", v)?, true);
    }
    if let Some(v) = named("filters", "field")? {
        (sc.filters, changed) = (read("filters", v)?, true);
    }
    if let Some(v) = named("sort", "field")? {
        (sc.sort, changed) = (read("sort", v)?, true);
    }
    if let Some(v) = p.get("grouped").filter(|v| !v.is_null()) {
        (sc.grouped, changed) = (v.as_bool().ok_or_else(|| bad(c, format!("`grouped` is true or false, not {v}")))?, true);
    }
    // A new one is made whatever it's given.
    let changed = changed || (found.is_none() && builtin.is_none());
    let id = match (found, changed) {
        (Some(id), true) => {
            s.model_mut().elements.insert(id, Element::Schedule(sc.clone()));
            Some(id)
        }
        (None, true) if !sc.name.is_empty() => Some(s.model_mut().insert(Element::Schedule(sc.clone()))),
        (None, true) => None,
        (id, false) => id,
    };
    let m = &s.model;
    // Sound before its table is made; and a file only for one the model keeps.
    crate::schedule::check(m, &sc).map_err(|e| bad(c, e))?;
    let t = crate::schedule::table(m, &sc).map_err(|e| bad(c, e))?;
    if let Some(path) = string(c, p, "path")? {
        if changed {
            m.check().map_err(|e| bad(c, e))?;
        }
        crate::disk::write(&path, crate::schedule::to_csv(&t)).map_err(|e| bad(c, format!("{path}: {e}")))?;
    }
    let columns: Vec<String> = t.columns.iter().map(|col| crate::schedule::heading(Units::Millimetres, col)).collect();
    let available: Vec<String> = crate::schedule::fields(m, &category(&sc.of)).into_iter().map(|(n, _)| n).collect();
    let mut out = json!({"id": id, "name": sc.name, "category": category(&sc.of), "columns": columns, "rows": t.rows, "elements": t.ids, "available": available});
    if let Some(totals) = t.totals {
        out["totals"] = json!(totals);
    }
    Ok(out)
}

/// A list of category names, as `key` gives them: each as views draw it.
fn drawn_categories(c: &str, p: &Value, key: &str) -> Result<Vec<String>, String> {
    let list = match p.get(key) {
        v if absent(v) => return Ok(Vec::new()),
        Some(Value::Array(a)) if a.len() <= graphics::MAX_ENTRIES => a,
        Some(v) => return Err(bad(c, format!("`{key}` is a list of categories, [\"walls\", \"rooms\"…], not {v}"))),
        None => return Ok(Vec::new()),
    };
    list.iter()
        .map(|v| v.as_str().ok_or_else(|| bad(c, format!("`{key}`: {v} is not a category"))).and_then(|n| drawn_category(c, n)))
        .collect()
}

/// The category `given` names, if views draw it.
fn drawn_category(c: &str, given: &str) -> Result<String, String> {
    let name = category(given);
    if graphics::drawn(&name) {
        Ok(name)
    } else {
        Err(bad(
            c,
            format!(
                "`{given}` isn't a category views draw; there are: {}",
                graphics::MODEL.iter().chain(&graphics::ANNOTATION).chain(&graphics::SCANS).copied().collect::<Vec<_>>().join(", ")
            ),
        ))
    }
}

/// A list of element ids, as `key` gives them: elements of the model that views draw.
fn drawn_elements(c: &str, m: &Model, p: &Value, key: &str) -> Result<Vec<Guid>, String> {
    let list = match p.get(key) {
        v if absent(v) => return Ok(Vec::new()),
        Some(Value::Array(a)) if a.len() <= graphics::MAX_ENTRIES => a,
        Some(v) => return Err(bad(c, format!("`{key}` is a list of element ids, not {v}"))),
        None => return Ok(Vec::new()),
    };
    list.iter()
        .map(|v| {
            serde_json::from_value::<Guid>(v.clone())
                .map_err(|_| bad(c, format!("`{key}`: {v} is not an element id or GUID")))
                .and_then(|id| drawn_element(c, m, id))
        })
        .collect()
}

/// `id`'s GUID if it's an element views draw.
fn drawn_element(c: &str, m: &Model, id: impl Key) -> Result<Guid, String> {
    match m.elements.get(id) {
        None => Err(bad(c, format!("no element {id}"))),
        Some(e) if !graphics::drawn(e.category()) => {
            Err(bad(c, format!("element {} is a {}, and views don't draw those", m.shown(id), e.category())))
        }
        Some(_) => Ok(m.guid(id)),
    }
}

/// `over` merged into what `key` of `from` holds, as JSON Merge Patch does: the override that comes
/// of it.
fn merged_override(c: &str, what: &str, from: Option<&Override>, patch: &Value) -> Result<Override, String> {
    let mut now = serde_json::to_value(from.cloned().unwrap_or_default()).unwrap_or_default();
    merge(&mut now, patch);
    serde_json::from_value(now).map_err(|e| bad(c, format!("{what}: {e}")))
}

/// The changes `p` asks of graphics `g`, made: hiding and showing categories and elements,
/// overrides, filters and the template to follow. Whether it changed any. A template's are its
/// categories' and its filters' alone.
fn edit_graphics(c: &str, m: &Model, p: &Value, g: &mut Graphics, of_template: bool) -> Result<(), String> {
    let given = |k: &str| !absent(p.get(k));
    let views_own = ["hide_elements", "show_elements", "elements"];
    if of_template && let Some(k) = views_own.iter().find(|k| given(k)).or(p.get("template").map(|_| &"template")) {
        return Err(bad(c, format!("`{k}` is a view's own; a template holds categories, filters, a phase and design options")));
    }
    // The template the view will follow once this call is done: one named, or none, in the same call, rules.
    let template_then = match p.get("template") {
        Some(Value::Null) => None,
        Some(Value::String(n)) => Some(n.clone()),
        _ => g.template.clone(),
    };
    let controlled = template_then.as_ref().and_then(|n| m.view_templates.get(n));
    let follows = |group: Group| controlled.filter(|t| t.controls.contains(&group)).map(|_| template_then.clone().unwrap_or_default());
    let touches_categories = ["hide", "show", "categories"].iter().any(|k| given(k));
    let touches_filters = given("filters");
    let touches_phase = p.get("phase").is_some() || p.get("phase_filter").is_some();
    let touches_options = given("options");
    if !flag(c, p, "reset")? {
        for (touches, group, what) in [
            (touches_categories, Group::Categories, "categories"),
            (touches_filters, Group::Filters, "filters"),
            (touches_phase, Group::Phase, "phase and phase filter"),
            (touches_options, Group::Options, "design options"),
        ] {
            if touches && let Some(t) = follows(group) {
                return Err(bad(
                    c,
                    format!("its {what} follow view template `{t}`; change the template, or `template: null` to stop following it"),
                ));
            }
        }
    }
    if flag(c, p, "reset")? {
        *g = Graphics::default();
    }
    for name in drawn_categories(c, p, "hide")? {
        g.categories.entry(name).or_default().hidden = true;
    }
    for name in drawn_categories(c, p, "show")? {
        if let Some(o) = g.categories.get_mut(&name) {
            o.hidden = false;
        }
        g.categories.retain(|_, o| !o.is_empty());
    }
    for id in drawn_elements(c, m, p, "hide_elements")? {
        g.elements.entry(id).or_default().hidden = true;
    }
    for id in drawn_elements(c, m, p, "show_elements")? {
        if let Some(o) = g.elements.get_mut(&id) {
            o.hidden = false;
        }
        g.elements.retain(|_, o| !o.is_empty());
    }
    match p.get("categories") {
        v if absent(v) => {}
        Some(Value::Object(o)) if o.len() <= graphics::MAX_ENTRIES => {
            for (name, patch) in o {
                let name = drawn_category(c, name)?;
                let over = if patch.is_null() {
                    Override::default()
                } else {
                    merged_override(c, &format!("category `{name}`"), g.categories.get(&name), patch)?
                };
                if over.is_empty() {
                    g.categories.remove(&name)
                } else {
                    g.categories.insert(name, over)
                };
            }
        }
        Some(v) => return Err(bad(c, format!("`categories` is {{category: override}}, not {v}"))),
        None => {}
    }
    match p.get("elements") {
        v if absent(v) => {}
        Some(Value::Object(o)) if o.len() <= graphics::MAX_ENTRIES => {
            for (key, patch) in o {
                let id = Guid::parse(key).ok_or_else(|| bad(c, format!("`elements`: `{key}` is not an element id or GUID")))?;
                let id = drawn_element(c, m, id)?;
                let over = if patch.is_null() {
                    Override::default()
                } else {
                    merged_override(c, &format!("element {}", m.shown(id)), g.elements.get(&id), patch)?
                };
                if over.is_empty() {
                    g.elements.remove(&id)
                } else {
                    g.elements.insert(id, over)
                };
            }
        }
        Some(v) => return Err(bad(c, format!("`elements` is {{id: override}}, not {v}"))),
        None => {}
    }
    match p.get("filters") {
        v if absent(v) => {}
        Some(Value::Array(a)) if a.len() <= graphics::MAX_APPLIED => {
            g.filters = a
                .iter()
                .map(|f| serde_json::from_value(f.clone()).map_err(|e| bad(c, format!("filter {f}: {e}"))))
                .collect::<Result<_, _>>()?;
        }
        Some(v) => {
            return Err(bad(
                c,
                format!("`filters` is [{{filter, hidden?, projection?, cut?, …}}, …] (up to {}), not {v}", graphics::MAX_APPLIED),
            ));
        }
        None => {}
    }
    match p.get("template") {
        None => {}
        Some(Value::Null) => g.template = None,
        Some(Value::String(n)) => g.template = Some(n.clone()),
        Some(v) => return Err(bad(c, format!("`template` is a view template's name, or null, not {v}"))),
    }
    // The phase it shows, and its phase filter: a name, or null for the last, and for every status as its category.
    for (key, slot) in [("phase", &mut g.phase), ("phase_filter", &mut g.phase_filter)] {
        match p.get(key) {
            None => {}
            Some(Value::Null) => *slot = None,
            Some(Value::String(n)) => *slot = Some(n.clone()),
            Some(v) => return Err(bad(c, format!("`{key}` is a name, or null, not {v}"))),
        }
    }
    // The design option it shows of each set, null taking a choice back.
    match p.get("options") {
        v if absent(v) => {}
        Some(Value::Object(o)) if o.len() <= phasing::MAX_OPTION_SETS => {
            for (set, option) in o {
                match option {
                    Value::Null => {
                        g.options.remove(set);
                    }
                    Value::String(n) => {
                        g.options.insert(set.clone(), n.clone());
                    }
                    v => return Err(bad(c, format!("`options`: set `{set}`'s option is a name, or null, not {v}"))),
                }
            }
        }
        Some(v) => return Err(bad(c, format!("`options` is {{set: option}}, not {v}"))),
        None => {}
    }
    Ok(())
}

/// A view's settings as the `graphics` command and the `template` command tell them: the
/// categories hidden and overridden, and the filters applied.
fn graphics_reply(g: &Graphics) -> Value {
    let hidden: Vec<&String> = g.categories.iter().filter(|(_, o)| o.hidden).map(|(k, _)| k).collect();
    json!({
        "hidden": hidden, "categories": g.categories, "filters": g.filters,
        "phase": g.phase, "phase_filter": g.phase_filter, "options": g.options,
        "scale": g.scale, "detail": g.detail, "visual_style": g.style,
    })
}

fn view_graphics(s: &mut Session, p: &Value) -> R {
    let c = "graphics";
    let is_3d = matches!(p.get("view"), Some(Value::String(n)) if n.eq_ignore_ascii_case("{3D}") || n.eq_ignore_ascii_case("3D"));
    // The default 3D view is made when something first changes in it; asking after it, or changing nothing, makes none.
    let mut view = if is_3d { s.model.default_3d() } else { Some(view_param(c, &s.model, p, false)?) };
    let m = &s.model;
    let was = match view {
        Some(v) => graphics::graphics_of(m, v)
            .cloned()
            .ok_or_else(|| bad(c, format!("{} isn't a plan, a section, an elevation or a 3D view", view_title(m, v))))?,
        None => Graphics::default(),
    };
    let mut g = was.clone();
    edit_graphics(c, m, p, &mut g, false)?;
    if g != was
        && let Some(v) = view
    {
        not_dependent(c, m, v, "graphics")?;
    }
    if g != was {
        // ponytail: checked here too so a refusal names the command, and `Model::check` the rest.
        g.check(&s.model).map_err(|e| bad(c, e))?;
        let v = match view {
            Some(v) => v,
            None => s.model_mut().insert(Element::View3D(View3D::default_view())),
        };
        set_graphics(s.model_mut(), v, g.clone());
        view = Some(v);
    }
    let m = &s.model;
    let in_force = g.resolved(m);
    let mut reply = graphics_reply(&in_force);
    let controlled: Vec<Group> =
        g.template.as_ref().and_then(|n| m.view_templates.get(n)).map(|t| t.controls.iter().copied().collect()).unwrap_or_default();
    reply["view"] = json!(view);
    reply["name"] = json!(view.and_then(|v| m.view_name(v)).unwrap_or("{3D}"));
    // Elements by their ids, as the command takes them.
    reply["hidden_elements"] = json!(g.elements.iter().filter(|(_, o)| o.hidden).filter_map(|(g, _)| m.elements.id(g)).collect::<Vec<_>>());
    reply["elements"] = json!(g.elements.iter().filter_map(|(g, o)| Some((m.elements.id(g)?.to_string(), o))).collect::<BTreeMap<_, _>>());
    reply["template"] = json!(g.template);
    reply["controlled"] = json!(controlled);
    Ok(reply)
}

/// ` (there are: a, b)`, or nothing when there are none.
fn known_names(names: &[&str]) -> String {
    if names.is_empty() { String::new() } else { format!(" (there are: {})", names.join(", ")) }
}

/// View `view`'s graphics, set.
/// How far a section box fitted round the model stands off it, mm.
const SECTION_BOX_ROOM: f64 = 300.0;

fn section_box(s: &mut Session, p: &Value) -> R {
    let c = "section_box";
    let m = &s.model;
    let default = absent(p.get("view"))
        || matches!(p.get("view"), Some(Value::String(n)) if n.eq_ignore_ascii_case("{3D}") || n.eq_ignore_ascii_case("3D"));
    // The default 3D view is made when its box is first set, as when its graphics first change.
    let view = if default {
        m.default_3d()
    } else {
        let v = view_param(c, m, p, false)?;
        if !matches!(m.elements.get(v), Some(Element::View3D(_))) {
            return Err(bad(c, format!("{} isn't a 3D view; only a 3D view has a section box", view_title(m, v))));
        }
        Some(v)
    };
    let was = view.and_then(|v| match m.elements.get(v) {
        Some(Element::View3D(v3)) => v3.section_box,
        _ => None,
    });
    let corner = |key: &str| -> Result<Option<[f64; 3]>, String> {
        match p.get(key) {
            v if absent(v) => Ok(None),
            Some(v) => serde_json::from_value::<[f64; 3]>(v.clone())
                .map(Some)
                .map_err(|_| bad(c, format!("`{key}`, a corner of the section box, is [x, y, z] in mm, not {v}"))),
            None => Ok(None),
        }
    };
    let (min, max) = (corner("min")?, corner("max")?);
    let on = match p.get("on") {
        v if absent(v) => true,
        Some(Value::Bool(b)) => *b,
        Some(v) => return Err(bad(c, format!("`on` must be true or false, got {v}"))),
        None => true,
    };
    let (min, max) = match (was, min, max) {
        (_, Some(lo), Some(hi)) => (lo, hi),
        (Some(b), lo, hi) => (lo.unwrap_or(b.min), hi.unwrap_or(b.max)),
        (None, lo, hi) => {
            let (flo, fhi) = m
                .model_box()
                .ok_or_else(|| bad(c, "there's nothing in the project to fit a section box round: give its `min` and `max`"))?;
            let room = |v: [f64; 3], k: f64| v.map(|x| x + k * SECTION_BOX_ROOM);
            (lo.unwrap_or(room(flo, -1.0)), hi.unwrap_or(room(fhi, 1.0)))
        }
    };
    let b = SectionBox { min, max, on };
    let v = match view {
        Some(v) => v,
        None => s.model_mut().insert(Element::View3D(View3D::default_view())),
    };
    if let Some(Element::View3D(v3)) = s.model_mut().elements.get_mut(v) {
        v3.section_box = Some(b);
    }
    Ok(json!({"view": v, "on": b.on, "min": b.min, "max": b.max}))
}

fn set_graphics(m: &mut Model, view: impl Key, g: Graphics) {
    if let Some(slot) = m.elements.get_mut(view).and_then(graphics::graphics_mut) {
        *slot = g;
    }
}

fn view_template(s: &mut Session, p: &Value) -> R {
    let c = "template";
    let name = string(c, p, "name")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).ok_or_else(|| bad(c, "`name` is required"))?;
    if flag(c, p, "delete")? {
        let m = s.model_mut();
        if m.view_templates.remove(&name).is_none() {
            return Err(bad(c, format!("there's no view template `{name}`")));
        }
        // Its views keep their own settings; they follow nothing now.
        for g in m.elements.values_mut().filter_map(graphics::graphics_mut) {
            if g.template.as_ref() == Some(&name) {
                g.template = None;
            }
        }
        return Ok(json!({"name": name, "deleted": true}));
    }
    let m = &s.model;
    let was = m.view_templates.get(&name).cloned();
    let keys = [
        "from",
        "controls",
        "view_range",
        "hide",
        "show",
        "categories",
        "filters",
        "reset",
        "hide_elements",
        "show_elements",
        "elements",
        "phase",
        "phase_filter",
        "options",
        "scale",
        "detail",
        "visual_style",
    ];
    let makes = keys.iter().any(|k| !absent(p.get(k))) || p.get("template").is_some();
    if was.is_none() && !makes {
        let there: Vec<&str> = m.view_templates.keys().map(String::as_str).collect();
        return Err(bad(
            c,
            format!("there's no view template `{name}`; make one with `from` (a view), or `controls`{}", known_names(&there)),
        ));
    }
    let mut t = was.clone().unwrap_or_else(|| ViewTemplate { controls: Group::ALL.into_iter().collect(), ..ViewTemplate::default() });
    // Made from a view: what its graphics and its plan's view range are now.
    if !absent(p.get("from")) {
        let view = view_param(c, m, &json!({"view": p["from"]}), false)?;
        let own = graphics::graphics_of(m, view).ok_or_else(|| {
            bad(c, format!("{} isn't a plan, a section, an elevation or a 3D view; templates are made from those", view_title(m, view)))
        })?;
        let resolved = own.resolved(m);
        t.graphics = Graphics {
            categories: resolved.categories.clone(),
            filters: resolved.filters.clone(),
            phase: resolved.phase.clone(),
            phase_filter: resolved.phase_filter.clone(),
            options: resolved.options.clone(),
            scale: resolved.scale,
            detail: resolved.detail,
            style: resolved.style,
            ..Graphics::default()
        };
        t.view_range = match m.elements.get(view) {
            Some(Element::Level(_) | Element::PlanView(_)) => Some(m.view_range_of(view)),
            _ => None,
        };
    }
    match p.get("controls") {
        v if absent(v) => {}
        Some(v) => {
            t.controls = serde_json::from_value(v.clone()).map_err(|e| {
                bad(
                    c,
                    format!(
                        "`controls` is a list of what it controls: {}: {e}",
                        Group::ALL.map(|g| format!("{g:?}").to_lowercase()).join(", ")
                    ),
                )
            })?;
        }
        None => {}
    }
    match p.get("view_range") {
        v if absent(v) => {}
        Some(v @ Value::Object(_)) => {
            let mut now = serde_json::to_value(t.view_range.unwrap_or_default()).unwrap_or_default();
            merge(&mut now, v);
            t.view_range = Some(serde_json::from_value(now).map_err(|e| bad(c, format!("`view_range`: {e}")))?);
        }
        Some(v) => return Err(bad(c, format!("`view_range` is {{top?, cut?, bottom?, depth?}} in mm, not {v}"))),
        None => {}
    }
    edit_graphics(c, m, p, &mut t.graphics, true)?;
    if let Some(scale) = scale_param(c, p)? {
        t.graphics.scale = Some(scale);
    }
    t.graphics.detail = detail_param(c, p)?.unwrap_or(t.graphics.detail);
    t.graphics.style = style_param(c, p, "visual_style")?.unwrap_or(t.graphics.style);
    if was.as_ref() != Some(&t) {
        s.model_mut().view_templates.insert(name.clone(), t.clone());
        t.check(&s.model).map_err(|e| bad(c, e))?;
    }
    let m = &s.model;
    let mut reply = graphics_reply(&t.graphics);
    reply["name"] = json!(name);
    reply["controls"] = json!(t.controls);
    reply["view_range"] = json!(t.view_range);
    reply["views"] = json!(
        m.elements
            .iter()
            .filter(|(_, e)| graphics::graphics_of_element(e).is_some_and(|g| g.template.as_ref() == Some(&name)))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
    );
    Ok(reply)
}

fn view_filter(s: &mut Session, p: &Value) -> R {
    let c = "filter";
    let name = string(c, p, "name")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).ok_or_else(|| bad(c, "`name` is required"))?;
    if flag(c, p, "delete")? {
        let m = s.model_mut();
        if m.view_filters.remove(&name).is_none() {
            return Err(bad(c, format!("there's no view filter `{name}`")));
        }
        for e in m.elements.values_mut() {
            if let Some(g) = graphics::graphics_mut(e) {
                g.filters.retain(|a| a.filter != name);
            }
        }
        for t in m.view_templates.values_mut() {
            t.graphics.filters.retain(|a| a.filter != name);
        }
        return Ok(json!({"name": name, "deleted": true}));
    }
    let was = s.model.view_filters.get(&name).cloned();
    let mut f = was.clone().unwrap_or_default();
    match p.get("categories") {
        v if absent(v) => {
            if was.is_none() {
                return Err(bad(c, "a new filter needs `categories`, [\"walls\", \"doors\"…]"));
            }
        }
        Some(Value::Array(a)) if a.len() <= 32 => {
            f.categories = a
                .iter()
                .map(|v| {
                    let given = v.as_str().ok_or_else(|| bad(c, format!("`categories`: {v} is not a category")))?;
                    let name = category(given);
                    if schedule::categories().contains(&name.as_str()) {
                        Ok(name)
                    } else {
                        Err(bad(
                            c,
                            format!("`{given}` isn't a category filters pick from; there are: {}", schedule::categories().join(", ")),
                        ))
                    }
                })
                .collect::<Result<_, _>>()?;
            f.categories.sort();
            f.categories.dedup();
        }
        Some(v) => return Err(bad(c, format!("`categories` is a list of categories, [\"walls\", \"doors\"…], not {v}"))),
        None => {}
    }
    match p.get("rules") {
        v if absent(v) => {}
        Some(Value::Array(a)) if a.len() <= schedule::MAX_FIELDS => {
            f.rules = a
                .iter()
                .map(|r| serde_json::from_value(r.clone()).map_err(|e| bad(c, format!("rule {r}: {e}"))))
                .collect::<Result<_, _>>()?;
        }
        Some(v) => return Err(bad(c, format!("`rules` is [{{field, op, value}}, …] (up to {}), not {v}", schedule::MAX_FIELDS))),
        None => {}
    }
    if was.as_ref() != Some(&f) {
        s.model_mut().view_filters.insert(name.clone(), f.clone());
        // Refused here, not only by the check after, so it names the command.
        f.check(&s.model).map_err(|e| bad(c, e))?;
    }
    Ok(json!({"name": name, "categories": f.categories, "rules": f.rules, "matches": f.matching(&s.model)}))
}

/// A name as a command takes it: text, trimmed, 1 to `MAX_TEXT` characters.
fn name_of(c: &str, v: &Value, what: &str) -> Result<String, String> {
    match v.as_str().map(str::trim) {
        Some(n) if !n.is_empty() && n.chars().count() <= MAX_TEXT => Ok(n.to_string()),
        _ => Err(bad(c, format!("{what} is a name of 1 to {MAX_TEXT} characters, not {v}"))),
    }
}

/// `key` of `p` as an optional name: none when absent or null.
fn name_opt(c: &str, p: &Value, key: &str) -> Result<Option<String>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => name_of(c, v, &format!("`{key}`")).map(Some),
        None => Ok(None),
    }
}

fn view_phases(s: &mut Session, p: &Value) -> R {
    let c = "phase";
    let name = name_opt(c, p, "name")?;
    let (before, after) = (name_opt(c, p, "before")?, name_opt(c, p, "after")?);
    let rename = match p.get("rename") {
        None => None,
        Some(v) => Some(name_of(c, v, "`rename`")?),
    };
    let into = name_opt(c, p, "into")?;
    let delete = flag(c, p, "delete")?;
    let now = match p.get("now") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(n)) => Some(Some(n.clone())),
        Some(v) => return Err(bad(c, format!("`now` is a phase's name, or null, not {v}"))),
    };
    let known = |m: &Model, n: &str| -> Result<usize, String> {
        phasing::index(m, n).ok_or_else(|| bad(c, format!("no phase `{n}` (there are: {})", m.phases.join(", "))))
    };
    if let Some(name) = &name {
        let m = &s.model;
        let held = phasing::holds_phase(m, s.phase.as_deref(), name);
        if delete {
            known(m, name)?;
            if m.phases.len() == 1 {
                return Err(bad(c, format!("`{name}` is the project's only phase; a project has one at least")));
            }
            if let Some(to) = &into {
                if to == name {
                    return Err(bad(c, "`into` is another phase than the one that goes"));
                }
                known(m, to)?;
            } else if held {
                return Err(bad(c, format!("`{name}` is in use: say `into` which phase takes its elements and views")));
            }
            let m = s.model_mut();
            // What was made in the first phase holds no value: name it, so it goes where the others go.
            if m.phases.first() == Some(name) {
                phasing::pin_first(m);
            }
            m.phases.retain(|x| x != name);
            if let Some(to) = &into {
                phasing::move_phase(m, name, Some(to));
            }
            if s.phase.as_ref() == Some(name) {
                s.phase = into;
            }
        } else if let Some(to) = &rename {
            known(m, name)?;
            if to != name && m.phases.contains(to) {
                return Err(bad(c, format!("there's a phase `{to}` already")));
            }
            let m = s.model_mut();
            if let Some(slot) = m.phases.iter_mut().find(|x| *x == name) {
                *slot = to.clone();
            }
            m.library.renamed(&LibKind::Phase, name, to);
            phasing::move_phase(m, name, Some(to));
            if s.phase.as_ref() == Some(name) {
                s.phase = Some(to.clone());
            }
        } else {
            if m.phases.contains(name) {
                return Err(bad(c, format!("there's a phase `{name}` already; rename or delete it")));
            }
            if before.is_some() && after.is_some() {
                return Err(bad(c, "`before` and `after` are two ways to say where; give one"));
            }
            let at = match (&before, &after) {
                (Some(b), _) => known(m, b)?,
                (_, Some(a)) => known(m, a)? + 1,
                _ => m.phases.len(),
            };
            let m = s.model_mut();
            // What holds no value was made in the first phase: a new first must not take it.
            if at == 0 {
                phasing::pin_first(m);
            }
            m.phases.insert(at, name.clone());
        }
    } else if rename.is_some() || delete || into.is_some() || before.is_some() || after.is_some() {
        return Err(bad(c, "`name` is required: the phase to rename, delete, or add"));
    }
    match p.get("overrides") {
        v if absent(v) => {}
        Some(Value::Object(o)) => {
            for (key, patch) in o {
                let status: phasing::Status = serde_json::from_value(json!(key))
                    .map_err(|_| bad(c, format!("`overrides`: `{key}` isn't a status: new, existing, demolished or temporary")))?;
                let now = s.model.phase_overrides.get(&status).cloned();
                let over = if patch.is_null() {
                    Override::default()
                } else {
                    merged_override(c, &format!("the {key} phase"), now.as_ref(), patch)?
                };
                let m = s.model_mut();
                if over.is_empty() {
                    m.phase_overrides.remove(&status);
                } else {
                    m.phase_overrides.insert(status, over);
                }
            }
        }
        Some(v) => return Err(bad(c, format!("`overrides` is {{status: override}}, not {v}"))),
        None => {}
    }
    if let Some(now) = now {
        if let Some(n) = &now {
            known(&s.model, n)?;
        }
        s.phase = now;
    }
    let m = &s.model;
    Ok(json!({"phases": m.phases, "now": s.phase, "overrides": m.phase_overrides}))
}

fn view_phase_filter(s: &mut Session, p: &Value) -> R {
    let c = "phase_filter";
    let name = name_opt(c, p, "name")?.ok_or_else(|| bad(c, "`name` is required"))?;
    let m = &s.model;
    if flag(c, p, "delete")? {
        if !m.phase_filters.contains_key(&name) {
            return Err(bad(c, format!("there's no phase filter `{name}`")));
        }
        if let Some((id, _)) =
            m.elements.iter().find(|(_, e)| graphics::graphics_of_element(e).is_some_and(|g| g.phase_filter.as_ref() == Some(&name)))
        {
            return Err(bad(c, format!("`{name}` is the phase filter of {}; give it another first", view_title(m, *id))));
        }
        if let Some(t) = m.view_templates.iter().find(|(_, t)| t.graphics.phase_filter.as_ref() == Some(&name)) {
            return Err(bad(c, format!("`{name}` is the phase filter of view template `{}`; give it another first", t.0)));
        }
        s.model_mut().phase_filters.remove(&name);
        return Ok(json!({"name": name, "deleted": true}));
    }
    let mut f = m.phase_filters.get(&name).cloned().unwrap_or_default();
    for status in phasing::Status::ALL {
        match p.get(status.name()) {
            v if absent(v) => {}
            Some(v) => {
                let how = serde_json::from_value(v.clone())
                    .map_err(|_| bad(c, format!("`{}` is by-category, overridden or not-displayed, not {v}", status.name())))?;
                f.set(status, how);
            }
            None => {}
        }
    }
    if m.phase_filters.get(&name) != Some(&f) {
        s.model_mut().phase_filters.insert(name.clone(), f.clone());
    }
    Ok(json!({"name": name, "filter": f}))
}

fn design_options(s: &mut Session, p: &Value) -> R {
    let c = "option";
    let set = name_opt(c, p, "set")?;
    let now = match p.get("now") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(n)) => Some(Some(n.clone())),
        Some(v) => return Err(bad(c, format!("`now` is `set: option`, or null, not {v}"))),
    };
    let Some(set) = set else {
        let Some(now) = now else { return Err(bad(c, "`set` is required: the design option set to make, change, rename or delete")) };
        if let Some(n) = &now
            && !phasing::option_exists(&s.model, n)
        {
            return Err(bad(c, format!("`now` is `set: option`, one the project has; `{n}` isn't")));
        }
        s.option = now;
        return Ok(json!({"now": s.option}));
    };
    let m = &s.model;
    let mine = |key: &str| name_opt(c, p, key);
    let (option, rename) = (mine("option")?, mine("rename")?);
    let (primary, delete, accept) = (mine("primary")?, flag(c, p, "delete")?, flag(c, p, "accept")?);
    let options: Option<Vec<String>> = match p.get("options") {
        v if absent(v) => None,
        Some(Value::Array(a)) if a.len() <= phasing::MAX_OPTIONS => {
            Some(a.iter().map(|v| name_of(c, v, "an option")).collect::<Result<_, _>>()?)
        }
        Some(v) => return Err(bad(c, format!("`options` is a list of up to {} names, not {v}", phasing::MAX_OPTIONS))),
        None => None,
    };
    let exists = m.option_sets.get(&set).cloned();
    if set.contains(':') {
        return Err(bad(c, format!("set `{set}`: a name has no colon in it")));
    }
    if let Some(list) = &options {
        if list.is_empty() {
            return Err(bad(c, format!("set `{set}`: `options` names at least one option, not none")));
        }
        for (i, o) in list.iter().enumerate() {
            if o.contains(':') {
                return Err(bad(c, format!("set `{set}`: option `{o}`: a name has no colon in it")));
            }
            if list[..i].contains(o) {
                return Err(bad(c, format!("set `{set}` has option `{o}` twice")));
            }
        }
    }
    // Delete and accept: what is in the set goes, or the primary's stays.
    if delete || accept {
        let existing = exists.ok_or_else(|| {
            bad(c, format!("no design option set `{set}`{}", known_names(&m.option_sets.keys().map(String::as_str).collect::<Vec<_>>())))
        })?;
        let keep = accept.then(|| existing.primary.clone());
        let mut gone = Vec::new();
        let mut kept = Vec::new();
        for o in &existing.options {
            let held = phasing::holders(m, phasing::OPTION, &format!("{set}: {o}"));
            if keep.as_ref() == Some(o) { kept.extend(held) } else { gone.extend(held) }
        }
        let deleted = if gone.is_empty() { json!([]) } else { delete_elements(s, &gone)? };
        let m = s.model_mut();
        for id in kept {
            if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
                params.remove(phasing::OPTION);
            }
        }
        m.option_sets.remove(&set);
        phasing::forget_set(m, &set);
        if s.option.as_deref().is_some_and(|o| o.starts_with(&format!("{set}: "))) {
            s.option = None;
        }
        return Ok(json!({"set": set, "deleted": deleted}));
    }
    // Rename the set, or one of its options.
    if let Some(to) = rename {
        let existing = exists.ok_or_else(|| bad(c, format!("no design option set `{set}`")))?;
        if to.contains(':') {
            return Err(bad(c, format!("`{to}`: a name has no colon in it")));
        }
        match option {
            Some(o) => {
                if !existing.options.contains(&o) {
                    return Err(bad(c, format!("set `{set}` has no option `{o}` (there are: {})", existing.options.join(", "))));
                }
                if to != o && existing.options.contains(&to) {
                    return Err(bad(c, format!("set `{set}` has an option `{to}` already")));
                }
                let m = s.model_mut();
                phasing::rename_option(m, &set, &o, &to);
                if s.option.as_deref() == Some(&format!("{set}: {o}")) {
                    s.option = Some(format!("{set}: {to}"));
                }
            }
            None => {
                if to != set && s.model.option_sets.contains_key(&to) {
                    return Err(bad(c, format!("there's a design option set `{to}` already")));
                }
                phasing::rename_set(s.model_mut(), &set, &to);
                if let Some(rest) = s.option.as_deref().and_then(|o| o.strip_prefix(&format!("{set}: "))) {
                    s.option = Some(format!("{to}: {rest}"));
                }
                let m = &s.model;
                let set = m.option_sets.get(&to).cloned();
                if let Some(now) = now {
                    if let Some(n) = &now
                        && !phasing::option_exists(m, n)
                    {
                        return Err(bad(c, format!("`now` is `set: option`, one the project has; `{n}` isn't")));
                    }
                    s.option = now;
                }
                return Ok(json!({"set": to, "options": set.as_ref().map(|x| &x.options), "primary": set.map(|x| x.primary)}));
            }
        }
    } else if options.is_some() || primary.is_some() || exists.is_none() {
        // Make the set, or change its options and its primary.
        let new_options = match (&options, &exists) {
            (Some(list), _) => list.clone(),
            (None, Some(e)) => e.options.clone(),
            (None, None) => {
                return Err(bad(
                    c,
                    format!("design option set `{set}` doesn't exist: make it with `options`, [\"Option 1\", \"Option 2\"]"),
                ));
            }
        };
        let new_primary = primary
            .or_else(|| exists.as_ref().map(|e| e.primary.clone()).filter(|x| new_options.contains(x)))
            .unwrap_or_else(|| new_options.first().cloned().unwrap_or_default());
        let made = phasing::OptionSet { options: new_options, primary: new_primary };
        // An option with elements in it can't go.
        if let Some(old) = &exists
            && let Some(o) = old
                .options
                .iter()
                .find(|o| !made.options.contains(o) && !phasing::holders(&s.model, phasing::OPTION, &format!("{set}: {o}")).is_empty())
        {
            return Err(bad(
                c,
                format!("option `{o}` has elements in it; move them (modify {{params}}) or `accept`/`delete` the set first"),
            ));
        }
        let m = s.model_mut();
        if let Some(old) = &exists {
            for o in old.options.iter().filter(|o| !made.options.contains(o)) {
                phasing::forget_option(m, &set, o);
            }
        }
        m.option_sets.insert(set.clone(), made);
    }
    if let Some(now) = now {
        if let Some(n) = &now
            && !phasing::option_exists(&s.model, n)
        {
            return Err(bad(c, format!("`now` is `set: option`, one the project has; `{n}` isn't")));
        }
        s.option = now;
    }
    let made = s.model.option_sets.get(&set).cloned();
    Ok(json!({"set": set, "options": made.as_ref().map(|x| &x.options), "primary": made.map(|x| x.primary), "now": s.option}))
}

/// Delete elements `ids` as the `delete` command does: what hangs on them goes too. The reply's ids.
fn delete_elements(s: &mut Session, ids: &[Id]) -> Result<Value, String> {
    delete(s, &json!({"ids": ids})).map(|r| r["deleted"].clone())
}

fn measure(_: &mut Session, p: &Value) -> R {
    let c = "measure";
    let r = snap::ruler(point_req(c, p, "from")?, point_req(c, p, "to")?);
    let to = |x: f64, k: f64| (x * k).round() / k + 0.0;
    Ok(json!({"length": to(r.length, 100.0), "dx": to(r.dx, 100.0), "dy": to(r.dy, 100.0), "angle": to(r.angle, 1e4)}))
}

fn undo(s: &mut Session, p: &Value) -> R {
    s.step_back(false, steps("undo", p)?)
}

fn redo(s: &mut Session, p: &Value) -> R {
    s.step_back(true, steps("redo", p)?)
}

/// How many steps undo or redo takes: `count`, 1 by default.
fn steps(c: &str, p: &Value) -> Result<usize, String> {
    if let Some(key) = p.as_object().and_then(|o| o.keys().find(|k| k.as_str() != "count")) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    count_opt(c, p, "count", 1)
}

// ---------- modelling ----------

fn highrise(s: &mut Session, p: &Value) -> R {
    let params = p.as_object().ok_or_else(|| bad("highrise", "params must be an object"))?;
    if let Some(key) = params.keys().find(|k| k.as_str() != "storeys") {
        return Err(bad("highrise", format!("unknown parameter `{key}`")));
    }
    let storeys = count_opt("highrise", p, "storeys", 20)?;
    if !(1..=80).contains(&storeys) {
        return Err(bad("highrise", "`storeys` must be between 1 and 80"));
    }
    let model = crate::sample::highrise(storeys as u32)?;
    let elements = model.elements.len();
    s.reset(model, None);
    Ok(json!({"storeys": storeys, "elements": elements}))
}

fn level(s: &mut Session, p: &Value) -> R {
    let c = "level";
    let elevation = num(c, p, "elevation")?.ok_or_else(|| bad(c, "`elevation` in mm is required"))?;
    let m = &s.model;
    let taken: BTreeSet<&str> = m.levels().into_iter().map(|(_, l)| l.name.as_str()).collect();
    let name = match string(c, p, "name")? {
        Some(n) => n,
        None => {
            let level = m.language.word("Level");
            (taken.len() + 1..).map(|n| format!("{level} {n}")).find(|n| !taken.contains(n.as_str())).unwrap_or_default()
        }
    };
    let extent = match p.get("extent") {
        v if absent(v) => None,
        _ => match points(c, p, "extent")?.as_slice() {
            [a, b] => Some([*a, *b]),
            _ => return Err(bad(c, "`extent` is two corners, [[x, y], [x, y]]: south-west, then north-east")),
        },
    };
    let id = s.model_mut().insert(Element::Level(Level { extent, ..Level::new(name.clone(), elevation) }));
    Ok(json!({"id": id, "name": name}))
}

/// A scan brought in from a file, or one the project holds, placed: its middle on `at` and its foot on the level, or where the
/// file's coordinates put it.
fn point_cloud(s: &mut Session, p: &Value) -> R {
    let c = "point_cloud";
    let m = &s.model;
    let level = level_param(c, m, p, "level")?;
    let paths: Option<Vec<PathBuf>> = match p.get("path") {
        None | Some(Value::Null) => None,
        Some(Value::String(one)) => Some(vec![PathBuf::from(one)]),
        Some(Value::Array(all)) if all.is_empty() => {
            return Err(bad(c, "`path` is a scan file, or a list of the files of one scan's stations, not an empty list"));
        }
        Some(Value::Array(all)) if all.len() > cloud::MAX_STATIONS => {
            return Err(bad(c, format!("a scan comes from up to {} files, not {}", cloud::MAX_STATIONS, all.len())));
        }
        Some(Value::Array(all)) => Some(
            all.iter()
                .map(|v| v.as_str().map(PathBuf::from).ok_or_else(|| bad(c, format!("`path` lists files by their paths, not {v}"))))
                .collect::<Result<_, _>>()?,
        ),
        Some(v) => return Err(bad(c, format!("`path` is a scan file, or a list of the files of one scan's stations, not {v}"))),
    };
    let (name, scan, new) = match (paths, string(c, p, "cloud")?) {
        (Some(_), Some(_)) => return Err(bad(c, "give `path`, a scan file to bring in, or `cloud`, a scan the project holds; not both")),
        (None, None) => {
            return Err(bad(
                c,
                "give `path`, a scan file (PTS, XYZ, PLY, LAS or E57) to bring in, or `cloud`, the name of a scan the project holds",
            ));
        }
        (None, Some(n)) => {
            let scan = m.clouds.get(&n).cloned().ok_or_else(|| {
                let names: Vec<&str> = m.clouds.keys().map(String::as_str).collect();
                bad(
                    c,
                    format!(
                        "the project holds no scan named `{n}` (it holds: {})",
                        if names.is_empty() { "none".into() } else { names.join(", ") }
                    ),
                )
            })?;
            (n, scan, false)
        }
        (Some(paths), None) => {
            let mut formats = Vec::with_capacity(paths.len());
            for path in &paths {
                let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
                formats.push(cloud::Format::from_extension(&ext).map_err(|e| bad(c, format!("{}: {e}", path.display())))?);
            }
            let stem = paths
                .first()
                .and_then(|p| p.file_stem())
                .map_or_else(|| "scan".into(), |s| s.to_string_lossy().chars().take(MAX_TEXT).collect());
            let name = string(c, p, "name")?.unwrap_or(stem);
            if name.trim().is_empty() {
                return Err(bad(c, "a scan needs a `name`"));
            }
            if m.clouds.contains_key(&name) {
                return Err(bad(
                    c,
                    format!(
                        "the project already holds a scan named `{name}`: place it again with `cloud`, or bring this file in under another `name`"
                    ),
                ));
            }
            if m.clouds.len() >= cloud::MAX_CLOUDS {
                return Err(bad(c, format!("the project holds {} scans already; delete the point clouds of one first", cloud::MAX_CLOUDS)));
            }
            let scale = match string(c, p, "units")? {
                None => 1000.0,
                Some(u) => cloud::unit(&u).ok_or_else(|| bad(c, format!("`units` is one of mm, cm, m, in, ft, not {u}")))?,
            };
            let spacing = num(c, p, "spacing")?.unwrap_or(0.0);
            // What the project's other scans leave of what it may hold, shared among the files.
            let held: usize = m.clouds.values().map(Cloud::len).sum();
            let cap = cloud::MAX_CLOUD_POINTS.saturating_sub(held);
            if cap == 0 {
                return Err(bad(c, format!("the project's scans hold {held} points, all it may; delete the point clouds of one first")));
            }
            let options = cloud::Options { scale, spacing, cap: (cap / paths.len()).max(1) };
            let mut scans = Vec::with_capacity(paths.len());
            for (path, format) in paths.iter().zip(formats) {
                let len = crate::disk::len(path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
                if len > cloud::MAX_BYTES as u64 {
                    return Err(bad(c, format!("{} is too large ({len} bytes; up to {})", path.display(), cloud::MAX_BYTES)));
                }
                let bytes = crate::disk::read(path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
                let read = cloud::read(&bytes, format, &options).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
                let source = path.file_name().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                scans.push((source, read));
            }
            let scan = match <[_; 1]>::try_from(scans) {
                Ok([(source, read)]) => Cloud::new(&source, read),
                Err(scans) => cloud::merge(scans, spacing).and_then(|(read, stations)| Cloud::of_stations(read, stations)),
            };
            (name, scan.map_err(|e| bad(c, e))?, true)
        }
    };
    let (lo, _) = scan.bounds();
    let origin = scan.origin();
    let (mut at, mut offset) = match string(c, p, "origin")?.as_deref() {
        None | Some("centre" | "center") => (P2(0.0, 0.0), -lo[2]),
        Some("file") => {
            let z = origin[2] - m.elevation(level);
            if !(origin[0].abs() <= MAX_COORD && origin[1].abs() <= MAX_COORD && z.abs() <= MAX_COORD) {
                return Err(bad(
                    c,
                    format!(
                        "the scan's middle lies at {origin:?} mm in its file, past the {MAX_COORD} mm a model reaches: place it by its `centre`"
                    ),
                ));
            }
            (P2(origin[0], origin[1]), z)
        }
        Some(o) => {
            return Err(bad(
                c,
                format!("`origin` is centre (its middle on `at`, its foot on the level) or file (where its file puts it), not {o}"),
            ));
        }
    };
    if let Some(a) = point_opt(c, p, "at")? {
        at = a;
    }
    if let Some(o) = num(c, p, "offset")? {
        offset = o;
    }
    let angle = num(c, p, "angle")?.unwrap_or(0.0);
    let (points, read) = (scan.len(), scan.read());
    let m = s.model_mut();
    if new {
        m.clouds.insert(name.clone(), scan);
    }
    let id = m.insert(Element::PointCloud(PointCloud { cloud: name.clone(), level, at, offset, angle }));
    Ok(json!({"id": id, "cloud": name, "points": points, "read": read, "origin": origin}))
}

fn grid(s: &mut Session, p: &Value) -> R {
    let c = "grid";
    let (start, end) = (point_req(c, p, "start")?, point_req(c, p, "end")?);
    let name = match string(c, p, "name")? {
        Some(n) => n,
        None => next_grid_name(&s.model),
    };
    let id = s.model_mut().insert(Element::Grid(Grid { name: name.clone(), start, end }));
    Ok(json!({"id": id, "name": name}))
}

fn wall(s: &mut Session, p: &Value) -> R {
    let c = "wall";
    let mut pts = points(c, p, "points")?;
    if pts.len() < 2 {
        return Err(bad(c, "`points` needs at least 2 points"));
    }
    if flag(c, p, "closed")?
        && let Some(&first) = pts.first()
    {
        pts.push(first);
    }
    let m = &s.model;
    let level = level_param(c, m, p, "level")?;
    let type_name = type_param(c, p, m, "wall")?;
    let height = num(c, p, "height")?;
    let top_level = match p.get("top_level") {
        v if absent(v) => height.is_none().then(|| m.level_above(level)).flatten(),
        _ => Some(level_param(c, m, p, "top_level")?),
    };
    let base_offset = num(c, p, "base_offset")?.unwrap_or(0.0);
    let height = height.unwrap_or(DEFAULT_HEIGHT);
    let arc = num(c, p, "arc")?.unwrap_or(0.0);
    let location = match string(c, p, "location")? {
        Some(n) => Location::from_name(&n).ok_or_else(|| bad(c, location_names(&n)))?,
        None => Location::Centre,
    };
    let (attach_top, attach_base) = (id_opt(c, p, "attach_top")?, id_opt(c, p, "attach_base")?);
    let m = s.model_mut();
    let ids: Vec<Id> = pts
        .windows(2)
        .filter_map(|seg| match seg {
            [a, b] => Some(m.insert(Element::Wall(Wall {
                params: Default::default(),
                type_name: type_name.clone(),
                level,
                start: *a,
                end: *b,
                top_level,
                height,
                base_offset,
                arc,
                location,
                attach_top,
                attach_base,
            }))),
            _ => None,
        })
        .collect();
    Ok(json!({"ids": ids}))
}

fn location_names(n: &str) -> String {
    let names: Vec<&str> = Location::ALL.iter().map(|l| l.name()).collect();
    format!("`location` is one of {}, not `{n}`", names.join(", "))
}

/// Walls turned round about their location lines: they run the other way, so their exterior sides
/// go to the other side; their doors and windows stay where they are, swinging and hinged as they were.
fn flip(s: &mut Session, p: &Value) -> R {
    let c = "flip";
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    if let Some(id) = ids.iter().find(|id| m.wall(**id).is_none()) {
        return Err(bad(c, format!("{} {id} isn't a wall", m.elements.get(id).map_or("element", Element::category))));
    }
    let m = s.model_mut();
    let lengths: BTreeMap<Guid, f64> = ids.iter().filter_map(|id| Some((m.guid(*id), Axis::of(m.wall(*id)?).length()))).collect();
    for (id, e) in m.elements.iter_mut() {
        match e {
            Element::Wall(w) if ids.contains(id) => {
                std::mem::swap(&mut w.start, &mut w.end);
                w.arc = -w.arc + 0.0;
            }
            Element::Door(o) | Element::Window(o) if lengths.contains_key(&o.host) => {
                o.offset = lengths.get(&o.host).copied().unwrap_or_default() - o.offset;
                (o.flip_facing, o.flip_hand) = (!o.flip_facing, !o.flip_hand);
            }
            _ => {}
        }
    }
    let hosts: BTreeMap<Guid, Guid> = m
        .elements
        .with_guids()
        .filter_map(|(_, id, e)| match e {
            Element::Door(o) | Element::Window(o) => Some((id, o.host)),
            _ => None,
        })
        .collect();
    // Dimensions keep to the same lines: a flipped wall's left and right swap, and its start and end, and its doors' jambs'.
    for e in m.elements.values_mut() {
        if let Element::Dimension(d) = e {
            for r in &mut d.refs {
                if lengths.contains_key(&r.id) {
                    r.face = r.face.flipped();
                } else if hosts.get(&r.id).is_some_and(|h| lengths.contains_key(h)) {
                    r.face = r.face.mirrored();
                }
            }
        }
    }
    Ok(json!({"flipped": ids}))
}

fn door(s: &mut Session, p: &Value) -> R {
    opening(s, p, true)
}

fn window(s: &mut Session, p: &Value) -> R {
    opening(s, p, false)
}

fn opening(s: &mut Session, p: &Value, is_door: bool) -> R {
    let o = build_opening(&s.model, p, is_door)?;
    let (host, offset, mark) = (s.model.elements.id(o.host), o.offset, o.mark.clone());
    let id = s.model_mut().insert(if is_door { Element::Door(o) } else { Element::Window(o) });
    Ok(json!({"id": id, "host": host, "offset": offset, "mark": mark}))
}

/// The door or window `p` asks for, before it is in the model: what the `door` and `window` commands insert, and what
/// the desktop sketches under the cursor.
pub fn opening_of(m: &Model, p: &Value, is_door: bool) -> Result<Element, String> {
    build_opening(m, p, is_door).map(|o| if is_door { Element::Door(o) } else { Element::Window(o) })
}

fn build_opening(m: &Model, p: &Value, is_door: bool) -> Result<Opening, String> {
    let c = if is_door { "door" } else { "window" };
    let types = if is_door { &m.types.door } else { &m.types.window };
    // Of a family's type, or one of the project's own.
    let family = string(c, p, "family")?;
    let (type_name, family_sill) = match &family {
        Some(name) => {
            let kind = if is_door { crate::family::Category::Door } else { crate::family::Category::Window };
            let f = m.families.get(name).filter(|f| f.category == kind).ok_or_else(|| {
                let names: Vec<&str> = m.families.iter().filter(|(_, f)| f.category == kind).map(|(n, _)| n.as_str()).collect();
                bad(c, format!("there's no {c} family `{name}` (there are: {})", names.join(", ")))
            })?;
            let type_name = match string(c, p, "type")? {
                Some(t) => t,
                None => f.types.keys().next().cloned().ok_or_else(|| bad(c, format!("family `{name}` has no type")))?,
            };
            let values = f.values(&type_name, &BTreeMap::new()).map_err(|e| bad(c, e))?;
            let sill = match values.get(crate::family::SILL) {
                Some(crate::family::Val::Number(x)) => Some(*x),
                _ => None,
            };
            (type_name, sill)
        }
        None => (type_param(c, p, m, c)?, None),
    };
    let at = point_opt(c, p, "at")?;
    let (host, along_at) = match id_opt(c, p, "wall")? {
        Some(h) => {
            let w = m.wall(h).ok_or_else(|| bad(c, format!("{h} is not a wall")))?;
            (m.guid(h), at.map_or(w.start.dist(w.end) / 2.0, |a| along(w, a)))
        }
        None => {
            let at = at.ok_or_else(|| bad(c, "give `at` [x, y] on a wall, or `wall` with `offset`"))?;
            let level = level_param(c, m, p, "level")?;
            wall_at(m, level, at).ok_or_else(|| bad(c, format!("no wall at [{}, {}] on {}", at.0, at.1, level_name(m, level))))?
        }
    };
    let offset = num(c, p, "offset")?.unwrap_or(along_at);
    let default_sill = family_sill.unwrap_or_else(|| match &family {
        Some(_) if is_door => 0.0,
        Some(_) => 900.0,
        None => types.get(&type_name).map_or(0.0, |t| t.sill),
    });
    let sill = if is_door { 0.0 } else { num(c, p, "sill")?.unwrap_or(default_sill) };
    let mark = match string(c, p, "mark")? {
        Some(mk) => mk,
        None => next_number(m.elements.values().filter_map(|e| match e {
            Element::Door(o) if is_door => Some(o.mark.as_str()),
            Element::Window(o) if !is_door => Some(o.mark.as_str()),
            _ => None,
        })),
    };
    let o = Opening {
        params: Default::default(),
        family,
        type_name,
        host,
        offset,
        sill,
        mark,
        flip_facing: flag(c, p, "flip_facing")?,
        flip_hand: flag(c, p, "flip_hand")?,
    };
    Ok(o)
}

fn floor(s: &mut Session, p: &Value) -> R {
    let c = "floor";
    let m = &s.model;
    let level = level_param(c, m, p, "level")?;
    let type_name = type_param(c, p, m, "floor")?;
    let (mut boundary, openings, sketch) = match sketched(c, p, "openings", &[])? {
        Some((r, lines)) => (r.outer, r.holes, lines),
        None => {
            let (boundary, found) = match absent(p.get("boundary")) {
                true => floor_outline(m, level)
                    .ok_or_else(|| bad(c, format!("no walls enclose an area on {}; give `boundary`", level_name(m, level))))?,
                false => (ring(points(c, p, "boundary")?), Vec::new()),
            };
            (boundary, rings(c, p, "openings")?.unwrap_or(found), Vec::new())
        }
    };
    if geom::area(&boundary) < 0.0 {
        boundary.reverse();
    }
    let arrow = match absent(p.get("arrow")) {
        true => None,
        false => match points(c, p, "arrow")?.as_slice() {
            &[tail, head] => Some([tail, head]),
            _ => return Err(bad(c, "`arrow` is [[x, y], [x, y]]: its tail, then its head")),
        },
    };
    let (offset, slope) = (num(c, p, "offset")?.unwrap_or(0.0), num(c, p, "slope")?.unwrap_or(0.0));
    let structural = flag(c, p, "structural")?;
    // Its openings lie inside it and apart, or the model refuses it.
    let area = m2(geom::area(&boundary) - openings.iter().map(|o| geom::area(o).abs()).sum::<f64>());
    let id = s.model_mut().insert(Element::Floor(Floor {
        params: Default::default(),
        type_name,
        level,
        boundary,
        openings,
        offset,
        slope,
        arrow,
        structural,
        sketch,
        pattern_origin: P2(0.0, 0.0),
        pattern_angle: 0.0,
    }));
    Ok(json!({"id": id, "area_m2": area}))
}

fn ceiling(s: &mut Session, p: &Value) -> R {
    let c = "ceiling";
    let m = &s.model;
    let level = level_param(c, m, p, "level")?;
    let type_name = type_param(c, p, m, "ceiling")?;
    let drawn = sketched(c, p, "openings", &[])?;
    let sketch = drawn.as_ref().map(|(_, lines)| lines.clone()).unwrap_or_default();
    let (mut boundary, found) = match (drawn, absent(p.get("boundary")), point_opt(c, p, "point")?) {
        (Some((r, _)), ..) => (r.outer, r.holes),
        (None, false, _) => (ring(points(c, p, "boundary")?), Vec::new()),
        // The room's free floor: what the walls' faces close around the point, and the walls standing
        // inside it, clockwise as a floor's openings are.
        (None, true, Some(at)) => geom::region_at(&m.face_lines(level), at, TOL)
            .map(|r| {
                let holes = r.holes.iter().map(|h| tidy(&h.iter().rev().copied().collect::<Vec<P2>>())).collect();
                (geom::corners(&r.outer).into_iter().map(micron).collect(), holes)
            })
            .ok_or_else(|| bad(c, format!("no walls on {} close around [{}, {}]; give `boundary`", level_name(m, level), at.0, at.1)))?,
        (None, true, None) => return Err(bad(c, "`point` [x, y] in a room, `boundary` [[x, y], …] or `sketch` is required")),
    };
    if geom::area(&boundary) < 0.0 {
        boundary.reverse();
    }
    let openings: Vec<Vec<P2>> = rings(c, p, "openings")?.unwrap_or(found);
    let offset = num(c, p, "offset")?.unwrap_or(CEILING_HEIGHT);
    // Its openings lie inside it and apart, or the model refuses it.
    let area = m2(geom::area(&boundary) - openings.iter().map(|o| geom::area(o).abs()).sum::<f64>());
    let id = s.model_mut().insert(Element::Ceiling(Ceiling {
        params: Default::default(),
        type_name,
        level,
        boundary,
        openings,
        offset,
        sketch,
    }));
    Ok(json!({"id": id, "area_m2": area}))
}

fn shaft(s: &mut Session, p: &Value) -> R {
    let c = "shaft";
    let m = &s.model;
    let level = level_param(c, m, p, "level")?;
    let (mut boundary, sketch) = match sketched(c, p, "", &[])? {
        Some((r, lines)) if r.holes.is_empty() => (r.outer, lines),
        Some(_) => return Err(bad(c, "a shaft is one outline with no holes in it: sketch each hole as a shaft of its own")),
        None => (ring(points(c, p, "boundary")?), Vec::new()),
    };
    if geom::area(&boundary) < 0.0 {
        boundary.reverse();
    }
    let (top_level, height) = top_param(c, m, p, level)?;
    let base_offset = num(c, p, "base_offset")?.unwrap_or(0.0);
    let id = s.model_mut().insert(Element::Shaft(Shaft { level, boundary, top_level, height, base_offset, sketch }));
    Ok(json!({"id": id}))
}

/// A mass's footprint area (m²), height (mm) and volume (m³).
fn mass_size(m: &Model, ms: &Mass) -> (f64, f64, f64) {
    let (bottom, top) = m.span(ms.level, ms.base_offset, ms.top_level, ms.height);
    (m2(geom::area(&ms.boundary).abs()), top - bottom, (m.mass_volume(ms, bottom, top) / 1e7).round() / 100.0)
}

fn mass(s: &mut Session, p: &Value) -> R {
    let c = "mass";
    let m = &s.model;
    let level = level_param(c, m, p, "level")?;
    let mut boundary = points(c, p, "boundary")?;
    let mut top = match (absent(p.get("top")), absent(p.get("lean"))) {
        (false, false) => return Err(bad(c, "give `top`, an outline, or `lean`, how far the top moves sideways, not both")),
        (false, true) => Some(points(c, p, "top")?),
        (true, false) => {
            let by = point_req(c, p, "lean")?;
            Some(boundary.iter().map(|q| *q + by).collect::<Vec<P2>>())
        }
        (true, true) => None,
    };
    // Counter-clockwise, and its top outline the same way round, twin for twin.
    if geom::area(&boundary) < 0.0 {
        boundary.reverse();
        if let Some(t) = top.as_mut() {
            t.reverse();
        }
    }
    let (top_level, height) = top_param(c, m, p, level)?;
    let name = string(c, p, "name")?.unwrap_or_else(|| m.language.word("Mass").into());
    let ms = Mass {
        name,
        level,
        top: top.filter(|t| *t != boundary),
        boundary,
        top_level,
        height,
        base_offset: num(c, p, "base_offset")?.unwrap_or(0.0),
        params: Default::default(),
    };
    let (area, height, volume) = mass_size(m, &ms);
    let id = s.model_mut().insert(Element::Mass(ms));
    Ok(json!({"id": id, "area_m2": area, "height": height, "volume_m3": volume}))
}

/// The mass `mass` names.
fn mass_of(c: &str, m: &Model, p: &Value) -> Result<(Id, Mass), String> {
    let mass = id_opt(c, p, "mass")?.ok_or_else(|| bad(c, "`mass`, the id of a mass, is required"))?;
    match (m.elements.id(mass), m.elements.get(mass)) {
        (Some(id), Some(Element::Mass(ms))) => Ok((id, ms.clone())),
        _ => Err(bad(c, format!("{mass} isn't a mass"))),
    }
}

/// The elements `made` (an id, or a list of them) holding `value` under `key`: their ids.
fn mark(s: &mut Session, made: &Value, key: &str, value: Value) -> Vec<Id> {
    let ids: Vec<Id> =
        made.as_array().map_or_else(|| made.as_u64().into_iter().collect(), |a| a.iter().filter_map(Value::as_u64).collect());
    for id in &ids {
        if let Some(params) = s.model_mut().elements.get_mut(id).and_then(Element::params_mut) {
            params.insert(key.into(), value.clone());
        }
    }
    ids
}

/// `p` with the parameters of `from` that `keys` name, for the command that makes the element.
fn passing(p: &Value, keys: &[&str], mut to: Value) -> Value {
    for k in keys.iter().filter(|k| !absent(p.get(**k))) {
        to[*k] = p[*k].clone();
    }
    to
}

fn mass_floors(s: &mut Session, p: &Value) -> R {
    let c = "mass_floors";
    let m = &s.model;
    let masses: Vec<(Id, Mass)> = match id_opt(c, p, "mass")? {
        Some(_) => vec![mass_of(c, m, p)?],
        None => m.elements.iter().filter_map(|(id, e)| if let Element::Mass(ms) = e { Some((*id, ms.clone())) } else { None }).collect(),
    };
    if masses.is_empty() {
        return Err(bad(c, "no mass; make one with `mass`"));
    }
    let prune = flag(c, p, "prune")?;
    let kind = string(c, p, "type")?;
    let (mut ids, mut made, mut updated, mut stale, mut removed) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (mass, ms) in &masses {
        let held = massing::floors_of(&s.model, s.model.guid(*mass));
        let slices = s.model.mass_slices(ms);
        let levels: Vec<Id> = slices.iter().map(|(level, _, _)| *level).collect();
        for &(level, z, _) in &slices {
            // The outline of the mass at the level: its footprint, or between that and its top outline if it's lofted.
            let ring = s.model.mass_outline(ms, z);
            let Some(&(f, _)) = held.iter().find(|(_, l)| s.model.same(*l, level)) else {
                let made_one = floor(s, &passing(p, &["type"], json!({"boundary": ring, "level": level})))?;
                for id in mark(s, &made_one["id"], massing::FLOOR, json!({"mass": mass, "level": level})) {
                    made.push(id);
                    ids.push(id);
                }
                continue;
            };
            ids.push(f);
            // A floor that already has the outline (and the type) is left alone, so a second run is no undo step.
            if let Some(Element::Floor(fl)) = s.model.elements.get(f)
                && (fl.boundary != ring || kind.as_ref().is_some_and(|k| *k != fl.type_name))
                && let Some(Element::Floor(fl)) = s.model_mut().elements.get_mut(f)
            {
                fl.boundary = ring;
                fl.type_name = kind.clone().unwrap_or_else(|| fl.type_name.clone());
                updated.push(f);
            }
        }
        let levels: Vec<Guid> = levels.into_iter().map(|l| s.model.guid(l)).collect();
        for (f, _) in held.into_iter().filter(|(_, l)| !levels.contains(l)) {
            if prune { removed.push(f) } else { stale.push(f) }
        }
    }
    if !removed.is_empty() {
        delete_elements(s, &removed)?;
    }
    Ok(json!({"ids": ids, "made": made, "updated": updated, "stale": stale, "removed": removed}))
}

fn show_mass(s: &mut Session, p: &Value) -> R {
    let c = "show_mass";
    let want = match p.get("show") {
        v if absent(v) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(v) => return Err(bad(c, format!("`show` must be true or false, got {v}"))),
        None => None,
    };
    let view = passing(p, &["view"], json!({}));
    let hidden = |r: &Value| r["hidden"].as_array().is_some_and(|h| h.iter().any(|n| n == "mass"));
    let show = want.unwrap_or_else(|| view_graphics(s, &view).is_ok_and(|r| hidden(&r)));
    let mut q = view;
    q[if show { "show" } else { "hide" }] = json!(["mass"]);
    let r = view_graphics(s, &q).map_err(|e| e.replacen("graphics", c, 1))?;
    Ok(json!({"shown": !hidden(&r), "hidden": r["hidden"], "view": r["view"], "name": r["name"]}))
}

fn wall_by_face(s: &mut Session, p: &Value) -> R {
    let c = "wall_by_face";
    let (mass, ms) = mass_of(c, &s.model, p)?;
    let (ring, crown) = (ms.footprint(), ms.top_ring());
    let n = ring.len();
    // A wall stands plumb, so only on a face whose top edge is right over its bottom edge.
    let faces: BTreeSet<usize> = match p.get("faces") {
        v if absent(v) => {
            let faces: BTreeSet<usize> = (0..n).filter(|f| massing::plumb(&ring, &crown, *f)).collect();
            if faces.is_empty() {
                return Err(bad(c, "every face of the mass leans, and a wall stands plumb; none can stand on one"));
            }
            faces
        }
        Some(Value::Array(a)) if a.len() <= MAX_POINTS => a
            .iter()
            .map(|v| {
                let face = v.as_u64().and_then(|f| usize::try_from(f).ok()).filter(|f| *f < n).ok_or_else(|| {
                    bad(c, format!("face {v} isn't an edge of the footprint: 0 to {}, from its first point", n.saturating_sub(1)))
                })?;
                if massing::plumb(&ring, &crown, face) {
                    Ok(face)
                } else {
                    Err(bad(c, format!("face {face} leans, and a wall stands plumb")))
                }
            })
            .collect::<Result<_, _>>()?,
        Some(v) => return Err(bad(c, format!("`faces` must be a list of edge numbers, got {v}"))),
        None => (0..n).collect(),
    };
    let top = ms.top_level.map_or_else(|| json!({"height": ms.height}), |t| json!({"top_level": t}));
    let mut ids = Vec::new();
    for f in faces {
        // Counter-clockwise, the mass is to the left of its edge; a wall faces out to its left, so it runs the other way.
        let (a, b) = (ring[f], ring[(f + 1) % n]);
        let mut q = json!({"points": [b, a], "level": ms.level, "base_offset": ms.base_offset, "location": "exterior-finish"});
        q.as_object_mut().into_iter().for_each(|o| o.extend(top.as_object().cloned().unwrap_or_default()));
        let made = wall(s, &passing(p, &["type", "location"], q))?;
        ids.extend(mark(s, &made["ids"], massing::FACE, json!({"mass": mass, "face": f})).into_iter().map(Value::from));
    }
    Ok(json!({"ids": ids}))
}

fn floor_by_face(s: &mut Session, p: &Value) -> R {
    let (mass, ms) = mass_of("floor_by_face", &s.model, p)?;
    let (level, offset) = ms.top_face();
    let made = floor(s, &passing(p, &["type"], json!({"boundary": ms.top_ring(), "level": level, "offset": offset})))?;
    mark(s, &made["id"], massing::FACE, json!({"mass": mass, "face": "top"}));
    Ok(made)
}

fn roof_by_face(s: &mut Session, p: &Value) -> R {
    let (mass, ms) = mass_of("roof_by_face", &s.model, p)?;
    let (level, offset) = ms.top_face();
    let q = json!({"boundary": ms.top_ring(), "level": level, "offset": offset, "slope": 0, "overhang": 0});
    let made = roof(s, &passing(p, &["type", "slope", "overhang"], q))?;
    mark(s, &made["id"], massing::FACE, json!({"mass": mass, "face": "top"}));
    Ok(made)
}

fn update_by_face(s: &mut Session, p: &Value) -> R {
    let c = "update_by_face";
    let m = &s.model;
    let made_by_face = || m.elements.iter().filter_map(|(id, e)| Some((*id, massing::face_of(e)?)));
    let ids: Vec<Id> = match (absent(p.get("ids")) && absent(p.get("id")), id_opt(c, p, "mass")?) {
        (false, Some(_)) => return Err(bad(c, "give `ids` or `mass`, not both")),
        (true, Some(mass)) => {
            mass_of(c, m, p)?;
            made_by_face().filter(|(_, (of, _))| m.same(*of, mass)).map(|(id, _)| id).collect()
        }
        (false, None) => {
            let ids = ids_param(c, m, p)?;
            if let Some(id) = ids.iter().find(|id| m.elements.get(id).and_then(massing::face_of).is_none()) {
                return Err(bad(c, format!("{id} isn't made by face; `wall_by_face`, `floor_by_face` and `roof_by_face` make them")));
            }
            ids
        }
        (true, None) => made_by_face().map(|(id, _)| id).collect(),
    };
    if ids.is_empty() {
        return Err(bad(
            c,
            "nothing is made by face: `wall_by_face`, `floor_by_face` and `roof_by_face` make walls, floors and roofs that follow a mass",
        ));
    }
    let (mut changed, mut same, mut gone) = (Vec::new(), Vec::new(), Vec::new());
    for id in ids {
        let Some((e, (mass, face))) = m.elements.get(id).and_then(|e| Some((e, massing::face_of(e)?))) else { continue };
        let mut now = e.clone();
        match m.elements.get(mass) {
            Some(Element::Mass(ms)) => match massing::follow(&mut now, ms, m.elements.id(mass).unwrap_or_default(), face) {
                Ok(()) if now == *e => same.push(id),
                Ok(()) => changed.push((id, now)),
                Err(why) => gone.push(format!("{} {id} {why}", e.category())),
            },
            _ => gone.push(format!("{} {id} was made from mass {mass}, which isn't there", e.category())),
        }
    }
    if !gone.is_empty() {
        return Err(bad(c, gone.join("; ")));
    }
    let updated: Vec<Id> = changed.iter().map(|(id, _)| *id).collect();
    for (id, e) in changed {
        s.model_mut().elements.insert(id, e);
    }
    Ok(json!({"updated": updated, "unchanged": same}))
}

fn room(s: &mut Session, p: &Value) -> R {
    let c = "room";
    let m = &s.model;
    let point = point_req(c, p, "point")?;
    let level = level_param(c, m, p, "level")?;
    let name = string(c, p, "name")?.unwrap_or_else(|| m.language.word("Room").into());
    let number = match string(c, p, "number")? {
        Some(n) => n,
        None => next_number(m.elements.values().filter_map(|e| match e {
            Element::Room(r) => Some(r.number.as_str()),
            _ => None,
        })),
    };
    let r = Room { params: Default::default(), level, point, name, number: number.clone() };
    let region = m.room_region(&r).ok_or_else(|| {
        bad(
            c,
            format!(
                "[{}, {}] isn't enclosed by walls on {}; place rooms inside a closed loop of walls",
                point.0,
                point.1,
                level_name(m, level)
            ),
        )
    })?;
    let area = m2(region.area());
    let id = s.model_mut().insert(Element::Room(r));
    Ok(json!({"id": id, "number": number, "area_m2": area}))
}

/// Of the lines a plan of `level` shows that `keep` takes (walls' faces, centre lines and ends,
/// doors' and windows' centres and jambs, grids), the nearest `p`, within 100 mm.
fn reference_near(m: &Model, level: impl Key, p: P2, keep: impl Fn(Line) -> bool) -> Option<Reference> {
    let cut = |w: Option<&Wall>| w.is_some_and(|w| m.cut_by(w, level));
    m.elements
        .iter()
        .filter(|(_, e)| match e {
            Element::Wall(w) => cut(Some(w)),
            Element::Door(o) | Element::Window(o) => cut(m.wall(o.host)),
            Element::Grid(_) => true,
            _ => false,
        })
        .filter_map(|(&id, _)| m.reference_at(id, p, &keep))
        .filter(|(_, d)| *d <= 100.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

/// The round column on `level` whose edge passes nearest `p`, within 100 mm.
fn arc_near(m: &Model, level: impl Key, p: P2) -> Option<Reference> {
    let level = m.guid(level);
    m.elements
        .with_guids()
        .filter(|(_, _, e)| e.column().is_some_and(|col| col.level == level))
        .filter_map(|(_, id, _)| {
            let r = Reference { id, face: Face::Centre };
            let (c, radius) = m.reference_arc(&r).ok()?;
            Some((r, (p.dist(c) - radius).abs()))
        })
        .filter(|(_, d)| *d <= 100.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

fn dimension(s: &mut Session, p: &Value) -> R {
    let c = "dimension";
    let m = &s.model;
    let (level, view) = plan_param(c, m, p, "level")?;
    let at = point_req(c, p, "at")?;
    let kind = match string(c, p, "kind")? {
        None => DimKind::Aligned,
        Some(k) => serde_json::from_value(json!(k)).map_err(|_| bad(c, format!("`kind` is aligned, angular or radial, not `{k}`")))?,
    };
    let given = p.get("refs").and_then(Value::as_array).ok_or_else(|| bad(c, "`refs` [ref, …] is required"))?;
    if given.len() > MAX_POINTS {
        return Err(bad(c, format!("`refs` has more than {MAX_POINTS} references")));
    }
    let mut refs: Vec<Reference> = Vec::new();
    for v in given {
        let near = |q: P2, what: &str| bad(c, format!("no {what} at [{}, {}] on {}", q.0, q.1, level_name(m, level)));
        let r = match point(v) {
            Some(q) if kind == DimKind::Radial => arc_near(m, level, q).ok_or_else(|| near(q, "round column's edge"))?,
            Some(q) => {
                // After the first: one parallel to it, or for an angle, one across it.
                let first = refs.first().and_then(|r| m.reference_line(r).ok());
                let keep = |l: Line| first.is_none_or(|(a, b)| geom::parallel(a, b, l.0, l.1) == (kind == DimKind::Aligned));
                reference_near(m, level, q, keep)
                    .ok_or_else(|| near(q, "wall face, wall centre line, wall end, door, window or grid line"))?
            }
            None => serde_json::from_value::<Reference>(v.clone())
                .map_err(|e| bad(c, format!("{v} is not a reference {{id, face?}} or a point [x, y]: {e}")))?,
        };
        refs.push(r);
    }
    let d = Dimension { level, view, kind, refs, at };
    let values: Vec<f64> = m.dimension_values(&d).into_iter().map(hundredth).collect();
    let id = s.model_mut().insert(Element::Dimension(d));
    Ok(json!({"id": id, "values": values}))
}

fn tag(s: &mut Session, p: &Value) -> R {
    let c = "tag";
    let m = &s.model;
    let offset = point_opt(c, p, "offset")?;
    let leader = flag(c, p, "leader")?;
    let label = string(c, p, "label")?.filter(|l| !l.trim().is_empty());
    let (level, view) = plan_param(c, m, p, "level")?;
    let hosts = if absent(p.get("ids")) && absent(p.get("id")) {
        // Tag All: the plan's doors and windows, or its `category`, that have no tag in it.
        let of = string(c, p, "category")?.map(|s| category(&s));
        let kind = of.clone().unwrap_or_else(|| "door and window".into());
        let plan = view.unwrap_or(level);
        let tagged: BTreeSet<Guid> = m
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Tag(t) if m.annotation_in(e, plan) => Some(t.host),
                _ => None,
            })
            .collect();
        // What the plan hides isn't tagged: its tag would hang there on nothing.
        let look = Look::of(m, level);
        let open: Vec<Id> = m
            .elements
            .iter()
            .filter(|(id, e)| {
                of.as_deref().map_or(matches!(e, Element::Door(_) | Element::Window(_)), |cat| e.category() == cat)
                    && !tagged.contains(&m.guid(*id))
                    && m.level_of(e) == Some(level)
                    && look.shows(**id)
            })
            .map(|(id, _)| *id)
            .collect();
        if open.is_empty() {
            return Err(bad(c, format!("nothing to tag: {} has no {kind} without a tag", level_name(m, level))));
        }
        open
    } else {
        ids_param(c, m, p)?
    };
    let tags: Vec<Tag> = hosts
        .iter()
        .map(|&host| Tag { host: m.guid(host), view, offset: offset.unwrap_or_else(|| m.tag_offset(host)), leader, label: label.clone() })
        .collect();
    let m = s.model_mut();
    let ids: Vec<Id> = tags.into_iter().map(|t| m.insert(Element::Tag(t))).collect();
    Ok(json!({"ids": ids}))
}

fn text_note(s: &mut Session, p: &Value) -> R {
    let c = "text";
    let m = &s.model;
    let text = string(c, p, "text")?.ok_or_else(|| bad(c, "`text` is required"))?;
    let at = point_req(c, p, "at")?;
    let view = view_param(c, m, p, true)?;
    let n = Note {
        view: m.guid(view),
        at,
        text,
        size: num(c, p, "size")?.unwrap_or(NOTE_SIZE),
        angle: num(c, p, "angle")?.unwrap_or(0.0),
        leader: point_opt(c, p, "leader")?,
    };
    let id = s.model_mut().insert(Element::Text(n));
    Ok(json!({"id": id}))
}

fn spot(s: &mut Session, p: &Value) -> R {
    let c = "spot";
    let m = &s.model;
    let at = point_req(c, p, "at")?;
    let view = view_param(c, m, p, false)?;
    let host = match (id_opt(c, p, "host")?, m.plan_level(view)) {
        (Some(h), _) => Some(h),
        (None, Some(level)) => Some(m.guid(m.spot_host(level, at).ok_or_else(|| {
            let place = format!("[{}, {}] on {}", at.0, at.1, level_name(m, level));
            bad(c, format!("nothing to read at {place}; place it on a floor, a roof, a stair, a wall or a column"))
        })?)),
        (None, None) => None,
    };
    let sp = Spot { view: m.guid(view), at, host };
    let elevation = m.spot_value(&sp).map(hundredth);
    let host = host.and_then(|h| m.elements.id(h));
    let id = s.model_mut().insert(Element::Spot(sp));
    Ok(json!({"id": id, "elevation": elevation, "host": host}))
}

/// The top of something standing on `level`: up to the next level, or `height` mm tall, or up to
/// `top_level`; and that height.
fn top_param(c: &str, m: &Model, p: &Value, level: impl Key) -> Result<(Option<Guid>, f64), String> {
    let height = num(c, p, "height")?;
    let top_level = match p.get("top_level") {
        v if absent(v) => height.is_none().then(|| m.level_above(level)).flatten(),
        _ => Some(level_param(c, m, p, "top_level")?),
    };
    Ok((top_level, height.unwrap_or(DEFAULT_HEIGHT)))
}

/// Degrees to a billionth, so a quarter turn reads 90.
fn degrees(a: f64) -> f64 {
    (a * 1e9).round() / 1e9
}

fn column(s: &mut Session, p: &Value) -> R {
    let e = stood_of(&s.model, p, "column")?;
    Ok(json!({"id": s.model_mut().insert(e)}))
}

fn structural_column(s: &mut Session, p: &Value) -> R {
    let e = stood_of(&s.model, p, "structuralcolumn")?;
    Ok(json!({"id": s.model_mut().insert(e)}))
}

fn footing(s: &mut Session, p: &Value) -> R {
    let e = stood_of(&s.model, p, "footing")?;
    Ok(json!({"id": s.model_mut().insert(e)}))
}

/// The column, structural column or footing (`category`) that `p` asks for, as its command makes it, not yet in the model:
/// a tool previews what a click would make.
pub fn stood_of(m: &Model, p: &Value, category: &str) -> Result<Element, String> {
    let c = command_of(category);
    let at = point_req(c, p, "at")?;
    let level = level_param(c, m, p, "level")?;
    let type_name = type_param(c, p, m, category)?;
    let angle = num(c, p, "angle")?.unwrap_or(0.0);
    // An isolated footing is a column laid flat: as thick as `thickness`, its top `offset` over the level.
    if category == "footing" {
        let thickness = num(c, p, "thickness")?.unwrap_or(400.0);
        if thickness <= 0.0 {
            return Err(bad(c, format!("`thickness` must be above zero, got {thickness}")));
        }
        let top = num(c, p, "offset")?.unwrap_or(0.0);
        let col = Column {
            params: Default::default(),
            type_name,
            level,
            at,
            angle,
            top_level: None,
            height: thickness,
            base_offset: top - thickness,
        };
        return Ok(Element::Footing(col));
    }
    let (top_level, height) = top_param(c, m, p, level)?;
    let col = Column {
        params: Default::default(),
        type_name,
        level,
        at,
        angle,
        top_level,
        height,
        base_offset: num(c, p, "base_offset")?.unwrap_or(0.0),
    };
    Ok(if category == "column" { Element::Column(col) } else { Element::StructuralColumn(col) })
}

fn beam(s: &mut Session, p: &Value) -> R {
    chain(s, p, "beam")
}

fn strip_footing(s: &mut Session, p: &Value) -> R {
    chain(s, p, "stripfooting")
}

fn pipe(s: &mut Session, p: &Value) -> R {
    chain(s, p, "pipe")
}

fn conduit(s: &mut Session, p: &Value) -> R {
    chain(s, p, "conduit")
}

fn cable_tray(s: &mut Session, p: &Value) -> R {
    chain(s, p, "cabletray")
}

fn wire(s: &mut Session, p: &Value) -> R {
    chain(s, p, "wire")
}

fn down_conductor(s: &mut Session, p: &Value) -> R {
    chain(s, p, "downconductor")
}

fn ground_conductor(s: &mut Session, p: &Value) -> R {
    chain(s, p, "groundconductor")
}

fn cabling(s: &mut Session, p: &Value) -> R {
    chain(s, p, "cable")
}

fn chain(s: &mut Session, p: &Value, category: &str) -> R {
    let made = runs_of(&s.model, p, category)?;
    let m = s.model_mut();
    let ids: Vec<Id> = made.into_iter().map(|e| m.insert(e)).collect();
    Ok(json!({"ids": ids}))
}

/// The run category a command makes, if it makes runs (`pipe`, `cable_tray`): `command_of`'s other way.
pub fn run_category_of(command: &str) -> Option<&'static str> {
    crate::model::RUN_CATEGORIES.into_iter().find(|c| command_of(c) == command)
}

/// The command that makes elements of `category`.
pub fn command_of(category: &str) -> &'static str {
    match category {
        "structuralcolumn" => "structural_column",
        "stripfooting" => "strip_footing",
        "beam" => "beam",
        "pipe" => "pipe",
        "conduit" => "conduit",
        "cabletray" => "cable_tray",
        "wire" => "wire",
        "downconductor" => "down_conductor",
        "groundconductor" => "ground_conductor",
        "cable" => "cabling",
        "footing" => "footing",
        _ => "column",
    }
}

/// The runs of `category` (a beam, a strip footing) that `p` asks for, one for each segment of its `points`, as the command
/// makes them, not yet in the model: a tool previews what a click would make.
pub fn runs_of(m: &Model, p: &Value, category: &str) -> Result<Vec<Element>, String> {
    let c = command_of(category);
    let mut pts = points(c, p, "points")?;
    if pts.len() < 2 {
        return Err(bad(c, "`points` needs at least 2 points"));
    }
    if flag(c, p, "closed")?
        && let Some(&first) = pts.first()
    {
        pts.push(first);
    }
    let level = level_param(c, m, p, "level")?;
    let type_name = type_param(c, p, m, category)?;
    let offset = num(c, p, "offset")?.unwrap_or(0.0);
    let end_offset = num(c, p, "end_offset")?;
    if end_offset.is_some() && pts.len() != 2 {
        return Err(bad(c, "`end_offset` is the height of one run's end: give 2 points"));
    }
    let justify = match string(c, p, "justify")? {
        None => crate::runs::default_justify(category),
        Some(n) => serde_json::from_value(Value::String(n.to_lowercase()))
            .map_err(|_| bad(c, format!("`justify` is top, centre or bottom, not `{n}`")))?,
    };
    let system = string(c, p, "system")?.unwrap_or_default();
    pts.windows(2)
        .map(|seg| match seg {
            [a, b] => Element::of_run(
                category,
                crate::model::Run {
                    type_name: type_name.clone(),
                    level,
                    start: *a,
                    end: *b,
                    offset,
                    end_offset,
                    justify,
                    system: system.clone(),
                    params: Default::default(),
                },
            ),
            _ => None,
        })
        .collect::<Option<Vec<Element>>>()
        .ok_or_else(|| bad(c, format!("`{category}` has no runs")))
}

/// The way a stair or a ramp climbs from `start`: toward `end`, else `angle` degrees (east by default).
fn climb(c: &str, p: &Value, start: P2) -> Result<f64, String> {
    match (point_opt(c, p, "end")?, num(c, p, "angle")?) {
        (Some(end), _) if end.dist(start) > TOL => Ok(degrees((end.1 - start.1).atan2(end.0 - start.0).to_degrees())),
        (Some(_), _) => Err(bad(c, format!("`end` must be away from `start`: it says which way the {c} climbs"))),
        (None, a) => Ok(a.unwrap_or(0.0)),
    }
}

/// `landings` [{at, turn?}, …], or none.
fn landings_param(c: &str, p: &Value) -> Result<Vec<Landing>, String> {
    match p.get("landings") {
        v if absent(v) => Ok(Vec::new()),
        Some(v @ Value::Array(a)) if a.len() <= 100 => serde_json::from_value(v.clone()).map_err(|e| bad(c, format!("`landings`: {e}"))),
        Some(_) => Err(bad(c, "`landings` is a list of at most 100 landings [{at, turn?}, …]")),
        None => Ok(Vec::new()),
    }
}

fn stair(s: &mut Session, p: &Value) -> R {
    let c = "stair";
    let m = &s.model;
    let start = point_req(c, p, "start")?;
    let angle = climb(c, p, start)?;
    let level = level_param(c, m, p, "level")?;
    let (top_level, height) = top_param(c, m, p, level)?;
    let dim = |key: &str, default: f64| num(c, p, key).map(|v| v.unwrap_or(default));
    let st = Stair {
        params: Default::default(),
        level,
        top_level,
        height,
        start,
        angle,
        width: dim("width", STAIR_WIDTH)?,
        riser: dim("riser", RISER)?,
        tread: dim("tread", TREAD)?,
        landings: landings_param(c, p)?,
    };
    let (n, _, length) = m.flight(&st);
    let level = st.level;
    let id = s.model_mut().insert(Element::Stair(st));
    let railings = sides(c, s, p, id, level)?;
    Ok(json!({"id": id, "risers": n, "length": length, "railings": railings}))
}

/// With `railings`, a railing of the default type down each side of stair or ramp `id`: their ids.
fn sides(c: &str, s: &mut Session, p: &Value, id: Id, level: Guid) -> Result<Vec<Id>, String> {
    if !flag(c, p, "railings")? {
        return Ok(Vec::new());
    }
    let type_name = s.model.default_type("railing").map(String::from).ok_or_else(|| bad(c, "the model has no railing types"))?;
    let m = s.model_mut();
    let host = m.guid(id);
    let railing = |side| Railing {
        params: Default::default(),
        type_name: type_name.clone(),
        level,
        path: Vec::new(),
        base_offset: 0.0,
        host: Some(host),
        side: Some(side),
    };
    Ok([Side::Left, Side::Right].into_iter().map(|side| m.insert(Element::Railing(railing(side)))).collect())
}

fn ramp(s: &mut Session, p: &Value) -> R {
    let c = "ramp";
    let m = &s.model;
    let start = point_req(c, p, "start")?;
    let angle = climb(c, p, start)?;
    let level = level_param(c, m, p, "level")?;
    let (top_level, height) = top_param(c, m, p, level)?;
    let r = Ramp {
        params: Default::default(),
        level,
        top_level,
        height,
        start,
        angle,
        width: num(c, p, "width")?.unwrap_or(RAMP_WIDTH),
        slope: num(c, p, "slope")?.unwrap_or(RAMP_SLOPE),
        landings: landings_param(c, p)?,
    };
    let length: f64 = stairs::ramp(m, &r).runs.iter().map(|r| r.length).sum();
    let level = r.level;
    let id = s.model_mut().insert(Element::Ramp(r));
    let railings = sides(c, s, p, id, level)?;
    Ok(json!({"id": id, "length": hundredth(length), "railings": railings}))
}

fn railing(s: &mut Session, p: &Value) -> R {
    let c = "railing";
    let m = &s.model;
    let type_name = type_param(c, p, m, "railing")?;
    let base = |level, path, host, side| Railing {
        params: Default::default(),
        type_name: type_name.clone(),
        level,
        path,
        base_offset: 0.0,
        host,
        side,
    };
    let made: Vec<Railing> = match id_opt(c, p, "host")? {
        // Along a stair's or a ramp's sides.
        Some(host) => {
            let level = m.elements.get(host).and_then(|e| m.level_of(e)).ok_or_else(|| bad(c, format!("no stair or ramp {host}")))?;
            let sides = match string(c, p, "side")?.as_deref() {
                None => vec![Side::Left, Side::Right],
                Some("left") => vec![Side::Left],
                Some("right") => vec![Side::Right],
                Some(other) => return Err(bad(c, format!("`side` is left or right, going up, not `{other}`"))),
            };
            sides.into_iter().map(|side| base(level, Vec::new(), Some(host), Some(side))).collect()
        }
        None => {
            let mut r = base(level_param(c, m, p, "level")?, points(c, p, "path")?, None, None);
            r.base_offset = num(c, p, "base_offset")?.unwrap_or(0.0);
            vec![r]
        }
    };
    let m = s.model_mut();
    let ids: Vec<Id> = made.into_iter().map(|r| m.insert(Element::Railing(r))).collect();
    Ok(json!({"ids": ids}))
}

fn roof(s: &mut Session, p: &Value) -> R {
    let c = "roof";
    let m = &s.model;
    let levels = m.levels();
    let level = if absent(p.get("level")) {
        levels.get(1).or(levels.first()).map(|(id, _)| m.guid(*id)).ok_or_else(|| bad(c, "the model has no levels"))?
    } else {
        level_param(c, m, p, "level")?
    };
    let type_name = type_param(c, p, m, "roof")?;
    let extrusion: Option<Extrusion> = shaped(c, p, "extrusion", "{line: [[x,y],[x,y]], profile: [[along, up],…], from?, to}")?;
    let drawn = sketched(c, p, "courtyards", &[])?;
    let sketch = drawn.as_ref().map(|(_, lines)| lines.clone()).unwrap_or_default();
    let given = !absent(p.get("boundary")) || drawn.is_some();
    let overhang = num(c, p, "overhang")?.unwrap_or(if given || extrusion.is_some() { 0.0 } else { 500.0 });
    let (boundary, found) = match (drawn, given, &extrusion) {
        (Some((r, _)), ..) => (r.outer, r.holes),
        (None, true, _) => (ring(points(c, p, "boundary")?), Vec::new()),
        (None, false, Some(_)) => (Vec::new(), Vec::new()),
        (None, false, None) => roof_footprint(m, level, overhang).map_err(|e| bad(c, e))?,
    };
    let r = Roof {
        params: Default::default(),
        type_name,
        level,
        boundary,
        courtyards: rings(c, p, "courtyards")?.unwrap_or(found),
        slope: if extrusion.is_some() { 0.0 } else { num(c, p, "slope")?.unwrap_or(30.0) },
        overhang,
        gable: flag(c, p, "gable")?,
        gables: shaped(c, p, "gables", "a list of edge numbers")?.unwrap_or_default(),
        offset: num(c, p, "offset")?.unwrap_or(0.0),
        openings: rings(c, p, "openings")?.unwrap_or_default(),
        join: id_opt(c, p, "join")?,
        extrusion,
        sketch,
    };
    let id = s.model_mut().insert(Element::Roof(r));
    Ok(json!({"id": id}))
}

/// A dormer on a roof's slope: its front wall across the slope through `at`, its cheek walls back
/// up the slope until the roof rises to their tops, all standing on the roof; a gable roof over
/// them, running back into the host's and ending where it meets it; the host open inside its walls.
fn dormer(s: &mut Session, p: &Value) -> R {
    let c = "dormer";
    let m = &s.model;
    let host = id_opt(c, p, "host")?.ok_or_else(|| bad(c, "`host`, the roof it stands on, is required"))?;
    let hr = match m.elements.get(host) {
        Some(Element::Roof(r)) if r.join.is_none() => r.clone(),
        _ => return Err(bad(c, format!("{host} isn't a roof that joins none"))),
    };
    let at = point_req(c, p, "at")?;
    let (w, height, slope) =
        (num(c, p, "width")?.unwrap_or(1600.0), num(c, p, "height")?.unwrap_or(1200.0), num(c, p, "slope")?.unwrap_or(30.0));
    let type_name = type_param(c, p, m, "wall")?;
    let t = m.types.wall.get(&type_name).map_or(0.0, |wt| wt.thickness);
    if !(w > t + TOL && height > TOL && slope > 0.0 && slope <= MAX_SLOPE) {
        return Err(bad(
            c,
            format!("it needs `width` wider than its walls ({t} mm), some `height`, and `slope` from 0 to {MAX_SLOPE} degrees"),
        ));
    }
    let faces = m.roof_faces(&hr).map_err(|e| bad(c, e))?;
    // Over a slope: inside its outline, and not in a hole in it.
    let over = |loops: &[Vec<P2>], q: P2| loops.iter().filter(|l| geom::contains(l, q)).count() % 2 == 1;
    let (loops, under) = faces
        .into_iter()
        .find(|(loops, _)| over(loops, at))
        .ok_or_else(|| bad(c, format!("[{}, {}] isn't over roof {host}", at.0, at.1)))?;
    let tan_h = under.g.length();
    if tan_h < 1e-6 {
        return Err(bad(c, format!("roof {host} is level at [{}, {}]: a dormer stands on a slope", at.0, at.1)));
    }
    // Up the slope, and across it to the left; the host's top at the front, where the walls stand.
    let (u, n) = (under.g.unit(), under.g.unit().perp());
    let front = under.at(at) + m.roof_rise(&hr, &under);
    let (tan, rise) = (slope.to_radians().tan(), m.types.roof.get(&hr.type_name).map_or(0.0, |rt| rt.thickness) / slope.to_radians().cos());
    let (eave, ridge) = (front + height, front + height + w / 2.0 * tan);
    // How far up the slope the host rises to its walls' tops, and to its roof's ridge, top and underside.
    let (cheek, back) = (height / tan_h, (ridge + rise - front) / tan_h);
    if !over(&loops, at + u * back) {
        return Err(bad(c, format!("it runs back over roof {host}'s ridge; set it lower down the slope, or make it lower or narrower")));
    }
    let level = hr.level;
    let base_offset = front - m.elevation(level);
    let (h, f) = (w / 2.0 + t / 2.0, t / 2.0);
    let boundary = [at - u * f + n * h, at - u * f - n * h, at + u * back - n * h, at + u * back + n * h].map(micron).to_vec();
    let model = s.model_mut();
    let roof = model.insert(Element::Roof(Roof {
        params: Default::default(),
        type_name: hr.type_name.clone(),
        level,
        boundary,
        courtyards: Vec::new(),
        slope,
        overhang: f,
        gable: false,
        gables: vec![0, 2],
        offset: eave - model.elevation(level),
        openings: Vec::new(),
        join: Some(host),
        extrusion: None,
        sketch: Vec::new(),
    }));
    let roof_guid = model.guid(roof);
    let wall = |start: P2, end: P2| {
        Element::Wall(Wall {
            params: Default::default(),
            type_name: type_name.clone(),
            level,
            start,
            end,
            top_level: None,
            height: ridge - front,
            base_offset,
            arc: 0.0,
            location: Location::Centre,
            attach_top: Some(roof_guid),
            attach_base: Some(host),
        })
    };
    let (l, r) = (micron(at + n * (w / 2.0)), micron(at - n * (w / 2.0)));
    let walls = [wall(r, l), wall(l, micron(l + u * cheek)), wall(micron(r + u * cheek), r)].map(|e| model.insert(e));
    // The host open inside its walls, up to where its top meets the dormer's underside.
    let (a, inner) = (w / 2.0 - f, |x: f64| (height + (w / 2.0 - x.abs()) * tan) / tan_h);
    let hole = [(a, f), (a, inner(a)), (0.0, inner(0.0)), (-a, inner(a)), (-a, f)].map(|(x, y)| micron(at + n * x + u * y)).to_vec();
    if let Some(Element::Roof(r)) = model.elements.get_mut(host) {
        r.openings.push(hole);
    }
    let window = match string(c, p, "window")? {
        Some(ty) => Some(opening(s, &json!({"wall": walls[0], "type": ty, "sill": 100}), false)?["id"].clone()),
        None => None,
    };
    Ok(json!({"roof": roof, "walls": walls, "window": window}))
}

/// A sketch line as a command takes it: `[[x,y],[x,y]]` straight, `[[x,y],[x,y],[x,y]]` an arc from the first to the
/// second through the third, or `{start, end, through?, id?}`, `id` one of the sketch's lines it keeps.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Given {
    Line([P2; 2]),
    Arc([P2; 3]),
    Full(Box<sketch::RawSketchLine>),
}

const SKETCH_SHAPE: &str = "a list of lines, each [[x,y],[x,y]], an arc [[x,y],[x,y],[x,y]] (its ends, then a point it passes \
     through) or an object: `id?` and a curve's fields, `start` and `end` (with `arc` degrees or a `through` point), \
     `points` (with `arcs` and `closed`), `centre` and `radius`, `centre` and `radii` (with `angle` and `span`), `control` \
     (with `knots`), or `fit` (with `start_tangent` and `end_tangent`)";

/// The lines given under `key`, a line given an `id` keeping that of the line of `old` (the sketch being edited) it names.
fn sketch_param(c: &str, p: &Value, key: &str, old: &[SketchLine]) -> Result<Option<Vec<SketchLine>>, String> {
    let Some(given) = shaped::<Vec<Given>>(c, p, key, SKETCH_SHAPE)? else { return Ok(None) };
    given
        .into_iter()
        .enumerate()
        .map(|(i, g)| match g {
            Given::Line([a, b]) => Ok(SketchLine::new(a, b)),
            Given::Arc([a, b, t]) => Ok(SketchLine::arc(a, b, t)),
            Given::Full(k) => {
                let curve = k.fields().curve().map_err(|e| bad(c, format!("`{key}` line {i}: {e}")))?;
                let mut l = SketchLine { style: k.style.clone(), invisible: k.invisible, ..SketchLine::from(curve) };
                if k.id != 0 {
                    let id = k.id;
                    let was = old.iter().find(|o| o.id == id).ok_or_else(|| {
                        bad(c, format!("`{key}` line {i}: {id} isn't a line of this sketch; leave `id` out for a new one"))
                    })?;
                    (l.id, l.guid) = (was.id, was.guid);
                    // Sent back by its id, it keeps the style and the invisibility it had, unless it is given them again.
                    l.style = l.style.or_else(|| was.style.clone());
                    if p.get(key).and_then(|v| v.get(i)).is_none_or(|o| o.get("invisible").is_none()) {
                        l.invisible = was.invisible;
                    }
                }
                Ok(l)
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// `sketch`: lines drawn in any order, solved into one outline (counter-clockwise) and its holes (clockwise), as
/// `sketch.rs` solves them, and the lines, to keep; None when it isn't given. Given with `boundary` or `holes` (the key the
/// element's holes go by), it's refused. A line given an `id` keeps that of the line of `old` it names.
fn sketched(c: &str, p: &Value, holes: &str, old: &[SketchLine]) -> Result<Option<(geom::Region, Vec<SketchLine>)>, String> {
    let Some(lines) = sketch_param(c, p, "sketch", old)? else { return Ok(None) };
    if ["boundary", holes].iter().any(|k| !k.is_empty() && !absent(p.get(*k))) {
        return Err(bad(c, format!("give the outline as `sketch` or `boundary` (with `{holes}`), not both")));
    }
    match sketch::solve_one(&lines) {
        Ok(r) => Ok(Some((r, lines))),
        Err(e) => Err(bad(c, format!("the sketch isn't one outline: {}", sketch::say(&e, &sketch::namer(&lines))))),
    }
}

/// A ring given as points (a boundary, an opening), any point repeating the one before it (or at its end, the first) left
/// out, and the points a millimetre from the one before, so a click twice on a corner is one. Whether it is simple, no edge
/// across another or along it, [`Model::check_edits`] says after the command (`modify` is held to it too); fewer than
/// three points, or no area, [`Model::check`] refuses.
fn ring(given: Vec<P2>) -> Vec<P2> {
    let mut r: Vec<P2> = Vec::with_capacity(given.len());
    for q in given {
        if r.last().is_none_or(|l| l.dist(q) > TOL) {
            r.push(q);
        }
    }
    while r.len() > 1 && r.first().zip(r.last()).is_some_and(|(a, b)| a.dist(*b) <= TOL) {
        r.pop();
    }
    r
}

/// Rings given under `key` (a floor's openings, a roof's courtyards), each cleaned as [`ring`] cleans one.
fn rings(c: &str, p: &Value, key: &str) -> Result<Option<Vec<Vec<P2>>>, String> {
    let Some(given) = shaped::<Vec<Vec<P2>>>(c, p, key, "a list of outlines [[[x,y],…],…]")? else { return Ok(None) };
    Ok(Some(given.into_iter().map(ring).collect()))
}

fn sketch_lines(_: &mut Session, p: &Value) -> R {
    let c = "sketch";
    let lines = sketch_param(c, p, "lines", &[])?.ok_or_else(|| bad(c, "`lines` [[[x,y],[x,y]],…] is required"))?;
    let profile = sketch::solve(&lines)
        .map_err(|e| bad(c, format!("the lines don't make closed loops: {}", sketch::say(&e, &sketch::namer(&lines)))))?;
    let regions: Vec<Value> =
        profile.regions.iter().map(|r| json!({"outline": r.outer, "holes": r.holes, "area_m2": m2(r.area())})).collect();
    Ok(json!({"regions": regions, "area_m2": m2(profile.area())}))
}

/// An element's sketch: the one it keeps, while that still draws its outline and holes; else their edges, as new lines.
fn boundary_now(e: &Element) -> Option<Vec<SketchLine>> {
    let (outline, holes) = match e {
        Element::Floor(f) => (f.boundary.clone(), f.openings.clone()),
        Element::Ceiling(x) => (x.boundary.clone(), x.openings.clone()),
        Element::Roof(r) if r.extrusion.is_none() => (r.boundary.clone(), r.courtyards.clone()),
        Element::Shaft(sh) => (sh.boundary.clone(), Vec::new()),
        _ => return None,
    };
    let kept = e.sketch().filter(|ls| sketch::draws(ls, &outline, &holes)).cloned();
    Some(kept.unwrap_or_else(|| sketch::lines_of(&outline, &holes)))
}

/// Revit's Edit Boundary: an element's outline and its holes redrawn as one sketch.
fn edit_boundary(s: &mut Session, p: &Value) -> R {
    let c = "edit_boundary";
    let m = &s.model;
    let id = id_opt(c, p, "id")?.ok_or_else(|| bad(c, "`id`, the floor, ceiling, roof or shaft to edit, is required"))?;
    let e = m.elements.get(id).cloned().ok_or_else(|| bad(c, format!("no element {id}")))?;
    if let Element::Roof(Roof { extrusion: Some(_), .. }) = e {
        return Err(bad(c, format!("{id} is an extrusion roof: change its profile, `modify` its `extrusion`")));
    }
    // A filled region is its sketch, any number of outlines.
    if let Element::FilledRegion(r) = e {
        let Some(lines) = sketch_param(c, p, "sketch", &r.sketch)? else { return Ok(json!({"id": id, "sketch": r.sketch})) };
        let profile = sketch::solve(&lines)
            .map_err(|e| bad(c, format!("the sketch doesn't close into loops: {}", sketch::say(&e, &sketch::namer(&lines)))))?;
        let id = m.elements.id(id).ok_or_else(|| bad(c, "no such element"))?;
        s.model_mut().elements.insert(id, Element::FilledRegion(FilledRegion { sketch: lines, ..r }));
        return Ok(json!({"id": id, "area_m2": m2(profile.area())}));
    }
    let now =
        boundary_now(&e).ok_or_else(|| bad(c, format!("{id} isn't a floor, a ceiling, a roof's footprint, a shaft or a filled region")))?;
    let Some((r, lines)) = sketched(c, p, "", &now)? else {
        return Ok(json!({"id": id, "sketch": now}));
    };
    let area = m2(r.area());
    let e = match e {
        Element::Floor(f) => Element::Floor(Floor { boundary: r.outer, openings: r.holes, sketch: lines, ..f }),
        Element::Ceiling(x) => Element::Ceiling(Ceiling { boundary: r.outer, openings: r.holes, sketch: lines, ..x }),
        Element::Shaft(_) if !r.holes.is_empty() => return Err(bad(c, "a shaft is one outline with no holes in it")),
        Element::Shaft(sh) => Element::Shaft(Shaft { boundary: r.outer, sketch: lines, ..sh }),
        Element::Roof(old) => {
            let mut new = Roof { boundary: r.outer, courtyards: r.holes, gables: Vec::new(), sketch: lines, ..old.clone() };
            // A gable stays where its edge does, either way round.
            let same =
                |(a, b): (P2, P2), (c, d): (P2, P2)| (a.dist(c) <= TOL && b.dist(d) <= TOL) || (a.dist(d) <= TOL && b.dist(c) <= TOL);
            let kept: Vec<(P2, P2)> = old.edges().enumerate().filter(|(i, _)| old.gables.contains(i)).map(|(_, e)| e).collect();
            new.gables = new.edges().enumerate().filter(|(_, e)| kept.iter().any(|k| same(*k, *e))).map(|(i, _)| i).collect();
            Element::Roof(new)
        }
        _ => return Err(bad(c, format!("{id} isn't a floor, a ceiling, a roof's footprint or a shaft"))),
    };
    let id = m.elements.id(id).ok_or_else(|| bad(c, "no such element"))?;
    s.model_mut().elements.insert(id, e);
    Ok(json!({"id": id, "area_m2": area}))
}

/// `key` as JSON shaped like `T` (a list of at most `MAX_POINTS`), or none.
fn shaped<T: serde::de::DeserializeOwned>(c: &str, p: &Value, key: &str, shape: &str) -> Result<Option<T>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) if v.as_array().is_none_or(|a| a.len() <= MAX_POINTS) => {
            serde_json::from_value(v.clone()).map(Some).map_err(|e| bad(c, format!("`{key}` is {shape}: {e}")))
        }
        Some(_) => Err(bad(c, format!("`{key}` lists more than {MAX_POINTS}"))),
        None => Ok(None),
    }
}

/// The footprint `roof` gives a roof on `level` by default, over the walls of the level below: their
/// outline, `overhang` mm past their outer faces, counter-clockwise; and its courtyards, the ones
/// `floor_outline` finds in those walls, `overhang` mm past their faces into each, clockwise; each
/// ring from its south-west corner. Over a courtyard too small to stay open past the overhang, the
/// roof runs on, as it does over rooms.
pub fn roof_footprint(m: &Model, level: impl Key, overhang: f64) -> Result<(Vec<P2>, Vec<Vec<P2>>), String> {
    let below = m.levels().into_iter().rev().find(|(_, l)| l.elevation < m.elevation(level) - TOL).map(|(id, _)| id);
    let below = below.ok_or_else(|| format!("{} has no level below to roof over; give `boundary`", level_name(m, level)))?;
    let (outline, loops) =
        floor_outline(m, below).ok_or_else(|| format!("no walls enclose an area on {}; give `boundary`", level_name(m, below)))?;
    // ponytail: the thickest wall on the level sets where the faces are; per edge if outlines mix wall types.
    let face = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Wall(w) if m.cut_by(w, below) => Some(m.thickness(w) / 2.0),
            _ => None,
        })
        .fold(0.0, f64::max);
    // How far the eaves are from the walls' centre lines: out from a ring running counter-clockwise,
    // in from one running clockwise.
    let d = face + overhang;
    // The courtyards that stay open past the overhang: moved in, one too small for it turns inside
    // out, its edges running back.
    // ponytail: one pinched shut only in the middle keeps its edges' ways and crosses itself; check
    // for crossings too if courtyards like that turn up.
    let courtyards = loops
        .iter()
        .filter_map(|l| {
            let c = geom::grow(l, d);
            let open = geom::ring_edges(l).zip(geom::ring_edges(&c)).all(|((p, q), (r, s))| (q - p).dot(s - r) > 0.0);
            open.then(|| tidy(&c))
        })
        .collect();
    Ok((tidy(&geom::grow(&outline, d)), courtyards))
}

fn section(s: &mut Session, p: &Value) -> R {
    let c = "section";
    let (start, end, depth) = (point_req(c, p, "start")?, point_req(c, p, "end")?, num(c, p, "depth")?);
    let name = match string(c, p, "name")? {
        Some(n) => n,
        None => s.model.free_view_name(s.model.language.word("Section"), true),
    };
    let id = s.model_mut().insert(Element::Section(Section {
        name: name.clone(),
        start,
        end,
        depth,
        crop: None,
        graphics: Graphics::default(),
    }));
    Ok(json!({"id": id, "name": name}))
}

fn elevation(s: &mut Session, p: &Value) -> R {
    let c = "elevation";
    let m = &s.model;
    let (depth, name, at) = (num(c, p, "depth")?, string(c, p, "name")?, point_opt(c, p, "at")?);
    let facing = match string(c, p, "facing")? {
        Some(f) => Some(Facing::parse(&f).ok_or_else(|| bad(c, format!("`facing` is north, south, east or west, not `{f}`")))?),
        None => None,
    };
    let nothing = || bad(c, "there are no walls or floors to look at; draw some, or give `start` and `end`");
    let lines: Vec<(String, P2, P2)> = match (point_opt(c, p, "start")?, point_opt(c, p, "end")?) {
        (Some(a), Some(b)) => vec![(name.unwrap_or_else(|| m.free_view_name(m.language.word("Elevation"), true)), a, b)],
        (None, None) => {
            let sides = match (facing, at) {
                (Some(f), _) => vec![f],
                (None, Some(q)) => vec![m.facing_from(q).ok_or_else(nothing)?],
                (None, None) => {
                    let there =
                        |f: &Facing| m.elements.values().any(|e| matches!(e, Element::Elevation(v) if v.name == m.language.word(f.name())));
                    let missing: Vec<Facing> = Facing::ALL.into_iter().filter(|f| !there(f)).collect();
                    if missing.is_empty() {
                        return Err(bad(c, "the four building elevations are there already"));
                    }
                    missing
                }
            };
            let mut lines = Vec::new();
            for f in sides {
                let (a, b) = m.elevation_line(f, at).ok_or_else(nothing)?;
                lines.push((name.clone().unwrap_or_else(|| m.free_view_name(m.language.word(f.name()), false)), a, b));
            }
            lines
        }
        _ => return Err(bad(c, "give both `start` and `end`, or neither")),
    };
    let m = s.model_mut();
    let made: Vec<(Id, String)> = lines
        .into_iter()
        .map(|(name, start, end)| {
            (
                m.insert(Element::Elevation(Section { name: name.clone(), start, end, depth, crop: None, graphics: Graphics::default() })),
                name,
            )
        })
        .collect();
    match made.as_slice() {
        [(id, name)] => Ok(json!({"id": id, "name": name})),
        _ => Ok(json!({"ids": made.iter().map(|(id, _)| *id).collect::<Vec<_>>()})),
    }
}

fn crop(s: &mut Session, p: &Value) -> R {
    let c = "crop";
    let m = &s.model;
    let view = view_param(c, m, p, true)?;
    // Some(corners), or Some(None) to uncrop; none: to what the view shows now.
    let region = match p.get("region") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(_) => match points(c, p, "region")?.as_slice() {
            [a, b] => Some(Some([*a, *b])),
            _ => return Err(bad(c, "`region` is two corners, [[x, y], [x, y]]")),
        },
    };
    match m.elements.get(view) {
        Some(Element::Level(_) | Element::PlanView(_)) => {
            let name = m.view_name(view).unwrap_or_default();
            let crop = match region {
                Some(r) => r,
                None => {
                    let pts = render::plan_shows(m, view);
                    let first = *pts.first().ok_or_else(|| bad(c, format!("{name} shows nothing to crop to")))?;
                    let (lo, hi) =
                        pts.iter().fold((first, first), |(lo, hi), q| (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1))));
                    Some([micron(lo - P2(1000.0, 1000.0)), micron(hi + P2(1000.0, 1000.0))])
                }
            };
            if let Some(Element::Level(Level { crop: slot, .. }) | Element::PlanView(PlanView { crop: slot, .. })) =
                s.model_mut().elements.get_mut(view)
            {
                *slot = crop;
            }
            Ok(json!({"view": view, "crop": crop}))
        }
        Some(Element::Section(v) | Element::Elevation(v)) => {
            let mut v = v.clone();
            // How far along the line its start moves.
            let mut shift = 0.0;
            match region {
                Some(Some([a, b])) => {
                    if !(a.0 < b.0 - TOL && a.1 < b.1 - TOL) {
                        return Err(bad(
                            c,
                            "a crop region runs from its bottom left corner to its top right one (in a plan, south-west to north-east)",
                        ));
                    }
                    // Its line runs between the region's sides.
                    let u = (v.end - v.start).unit();
                    (v.start, v.end, v.crop, shift) = (micron(v.start + u * a.0), micron(v.start + u * b.0), Some([a.1, b.1]), a.0);
                }
                Some(None) => v.crop = None,
                None => v.crop = Some(render::section_heights(m, view)),
            }
            let crop = v.crop;
            let m = s.model_mut();
            let guid = m.guid(view);
            if let Some(Element::Section(old) | Element::Elevation(old)) = m.elements.get_mut(view) {
                *old = v;
            }
            // What's noted in the view keeps its spot over the model.
            for e in m.elements.values_mut() {
                match e {
                    Element::Text(n) if n.view == guid => {
                        n.at.0 -= shift;
                        if let Some(l) = &mut n.leader {
                            l.0 -= shift;
                        }
                    }
                    Element::Spot(sp) if sp.view == guid => sp.at.0 -= shift,
                    _ => {}
                }
            }
            Ok(json!({"view": view, "crop": crop}))
        }
        _ => Err(bad(c, format!("{} isn't a plan, a section or an elevation; only those crop", view_title(m, view)))),
    }
}

/// The plan, section or elevation `p` names, the views that have a scale and a detail level.
fn scaled_view(c: &str, m: &Model, p: &Value) -> Result<Id, String> {
    let view = view_param(c, m, p, false)?;
    match m.elements.get(view) {
        Some(Element::Level(_) | Element::PlanView(_) | Element::Section(_) | Element::Elevation(_)) => Ok(view),
        _ => Err(bad(c, format!("{} isn't a plan, a section or an elevation; only those have one", view_title(m, view)))),
    }
}

/// View `view`'s graphics changed by `f`, when it makes a difference: the model stays as it was otherwise, so no undo step.
fn change_graphics(s: &mut Session, view: impl Key, f: impl Fn(&mut Graphics)) {
    let mut g = graphics::graphics_of(&s.model, view).cloned().unwrap_or_default();
    let was = g.clone();
    f(&mut g);
    if g != was {
        set_graphics(s.model_mut(), view, g);
    }
}

/// The scale `p` asks for, 1 : n, if it does: a view always has one, so null is refused.
fn scale_param(c: &str, p: &Value) -> Result<Option<f64>, String> {
    match p.get("scale") {
        None => Ok(None),
        Some(v) => match v.as_f64() {
            Some(n) if graphics::SCALES.contains(&n) => Ok(Some(n)),
            _ => {
                let (lo, hi) = (graphics::SCALES.start(), graphics::SCALES.end());
                let none = if v.is_null() {
                    ": a view always has a scale (1 : 100, or 1/8\" = 1'-0\" in feet and inches, until it is given one)"
                } else {
                    ""
                };
                Err(bad(c, format!("`scale` is the n of 1 : n, from {lo} to {hi}, not {v}{none}")))
            }
        },
    }
}

/// The detail level `p` asks for, if it does.
fn detail_param(c: &str, p: &Value) -> Result<Option<graphics::Detail>, String> {
    match p.get("detail") {
        v if absent(v) => Ok(None),
        Some(v) => Ok(Some(
            v.as_str().and_then(graphics::Detail::parse).ok_or_else(|| bad(c, format!("`detail` is coarse, medium or fine, not {v}")))?,
        )),
        None => Ok(None),
    }
}

/// The visual style `p` asks for, as `key` gives it, if it does.
fn style_param(c: &str, p: &Value, key: &str) -> Result<Option<graphics::VisualStyle>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => Ok(Some(
            v.as_str()
                .and_then(graphics::VisualStyle::parse)
                .ok_or_else(|| bad(c, format!("`{key}` is shaded, hidden_line or wireframe, not {v}")))?,
        )),
        None => Ok(None),
    }
}

/// An error unless view `view` may set what `group` holds: its template, if it controls that, sets it.
fn own_to_set(c: &str, m: &Model, view: impl Key, group: Group, what: &str) -> Result<(), String> {
    not_dependent(c, m, view, what)?;
    match graphics::graphics_of(m, view).and_then(|g| g.controlled_by(m, group)) {
        Some(t) => {
            Err(bad(c, format!("its {what} follow view template `{t}`; change the template, or `template: null` to stop following it")))
        }
        None => Ok(()),
    }
}

/// Revit's Duplicate View, of a plan: a new plan view of the same level (see the catalog entry).
fn duplicate_view(s: &mut Session, p: &Value) -> R {
    let c = "duplicate_view";
    let m = &s.model;
    let view = view_param(c, m, p, false)?;
    let Some(level) = m.plan_level(view).map(|l| m.guid(l)) else {
        return Err(bad(
            c,
            format!(
                "{} isn't a plan: plans are duplicated, sections, elevations, schedules, sheets and the 3D view not yet",
                view_title(m, view)
            ),
        ));
    };
    let source = m.guid(view);
    let mode = string(c, p, "mode")?.unwrap_or_else(|| "duplicate".into());
    if !matches!(mode.as_str(), "duplicate" | "detailing" | "dependent") {
        return Err(bad(c, format!("`mode` is duplicate, detailing or dependent, not `{mode}`")));
    }
    let from = m.plan_view(view);
    if mode == "dependent" && from.is_some_and(|v| v.parent.is_some()) {
        return Err(bad(c, format!("{} is a dependent view, which has none of its own: make it of its parent", view_title(m, view))));
    }
    let name = match string(c, p, "name")? {
        Some(n) => n,
        None => {
            let word = m.language.word(if mode == "dependent" { "Dependent" } else { "Copy" });
            m.free_view_name(&format!("{} {word}", view_title(m, view)), true)
        }
    };
    // What it starts as: the settings the source is drawn with, a dependent's being its parent's.
    let held = m.elements.get(m.plan_source(view));
    let (view_range, graphics, section_box) = match held {
        Some(Element::Level(l)) => (l.view_range, l.graphics.clone(), l.section_box),
        Some(Element::PlanView(v)) => (v.view_range, v.graphics.clone(), v.section_box),
        _ => Default::default(),
    };
    let dependent = mode == "dependent";
    let new = PlanView {
        level,
        name: name.clone(),
        view_range: if dependent { Default::default() } else { view_range },
        crop: m.crop_of(view),
        graphics: if dependent { Graphics::default() } else { graphics },
        parent: dependent.then_some(source),
        section_box: section_box && !dependent,
    };
    // With detailing, what is annotated in the source itself (not in its parent) is copied into the new view.
    let notes: Vec<Element> =
        m.elements.values().filter(|e| mode == "detailing" && m.annotation_plan(e) == Some(source)).cloned().collect();
    let m = s.model_mut();
    let id = m.insert(Element::PlanView(new));
    let guid = m.guid(id);
    let copied = notes.len();
    for mut e in notes {
        match &mut e {
            Element::Dimension(d) => d.view = Some(guid),
            Element::Tag(t) => t.view = Some(guid),
            Element::Text(n) => n.view = guid,
            Element::Spot(sp) => sp.view = guid,
            Element::DetailLine(l) => l.view = guid,
            Element::FilledRegion(r) => r.view = guid,
            Element::DetailGroup(g) => g.view = guid,
            Element::RepeatingDetail(r) => r.view = guid,
            _ => {}
        }
        m.insert(e);
    }
    Ok(json!({"id": id, "name": name, "level": m.elements.id(level), "annotations": copied}))
}

/// An error unless view `view` is no dependent view: it follows its parent in its view range and graphics (`what` it was
/// asked to change among them).
fn not_dependent(c: &str, m: &Model, view: impl Key, what: &str) -> Result<(), String> {
    match m.plan_view(view).and_then(|v| v.parent) {
        Some(p) => Err(bad(
            c,
            format!(
                "{} is a dependent view: it follows its parent, `{}`, in its {what}; change them there",
                view_title(m, view),
                view_title(m, p)
            ),
        )),
        None => Ok(()),
    }
}

fn view_scale(s: &mut Session, p: &Value) -> R {
    let c = "scale";
    let view = scaled_view(c, &s.model, p)?;
    if let Some(scale) = scale_param(c, p)? {
        own_to_set(c, &s.model, view, Group::Scale, "scale")?;
        change_graphics(s, view, |g| g.scale = Some(scale));
    }
    let m = &s.model;
    let now = graphics::scale_of(m, view);
    Ok(json!({"view": view, "name": m.view_name(view), "scale": now}))
}

fn visual_style(s: &mut Session, p: &Value) -> R {
    let c = "visual_style";
    let m = &s.model;
    // The default 3D view, unless another is named; it is made when something first changes in it.
    let view = match p.get("view") {
        v if absent(v) => m.default_3d(),
        Some(Value::String(n)) if n.eq_ignore_ascii_case("{3D}") || n.eq_ignore_ascii_case("3D") => m.default_3d(),
        Some(_) => Some(view_param(c, m, p, false)?),
        None => m.default_3d(),
    };
    if let Some(v) = view.filter(|v| !matches!(m.elements.get(v), Some(Element::View3D(_)))) {
        return Err(bad(c, format!("{} isn't a 3D view; only those have a visual style", view_title(m, v))));
    }
    let style = style_param(c, p, "style")?;
    if let (Some(_), Some(v)) = (style, view) {
        own_to_set(c, m, v, Group::Style, "visual style")?;
    }
    let has =
        |view: Option<Id>, m: &Model| view.and_then(|v| graphics::graphics_of(m, v)).map_or_else(Default::default, |g| g.resolved(m).style);
    let view = match (style, view) {
        (Some(style), view) if style != has(view, m) => {
            let v = view.unwrap_or_else(|| s.model_mut().insert(Element::View3D(View3D::default_view())));
            change_graphics(s, v, |g| g.style = style);
            Some(v)
        }
        (_, view) => view,
    };
    Ok(json!({"view": view, "style": has(view, &s.model)}))
}

fn view_detail(s: &mut Session, p: &Value) -> R {
    let c = "detail";
    let view = scaled_view(c, &s.model, p)?;
    if let Some(level) = detail_param(c, p)? {
        own_to_set(c, &s.model, view, Group::Detail, "detail level")?;
        change_graphics(s, view, |g| g.detail = level);
    }
    let m = &s.model;
    let now = graphics::graphics_of(m, view).map(|g| g.resolved(m).detail).unwrap_or_default();
    Ok(json!({"view": view, "name": m.view_name(view), "detail": now}))
}

/// What to call element `id` in a message: a view's or a sheet's name, else its category and id.
fn view_title(m: &Model, id: impl Key) -> String {
    match (m.view_name(id), m.sheet(id)) {
        (Some(n), _) => n.to_string(),
        (_, Some(sh)) => format!("sheet {}", sh.number),
        _ => format!("{} {}", m.elements.get(id).map_or("element", Element::category), m.shown(id)),
    }
}

fn new_sheet(s: &mut Session, p: &Value) -> R {
    let c = "sheet";
    let m = &s.model;
    let number = match string(c, p, "number")? {
        Some(n) => n,
        None => {
            let taken: BTreeSet<&str> = m
                .elements
                .values()
                .filter_map(|e| match e {
                    Element::Sheet(sh) => Some(sh.number.as_str()),
                    _ => None,
                })
                .collect();
            (101..).map(|n| format!("A{n}")).find(|n| !taken.contains(n.as_str())).unwrap_or_default()
        }
    };
    let name = string(c, p, "name")?.unwrap_or_else(|| m.language.word("Unnamed").into());
    let size = string(c, p, "size")?.map_or_else(|| "A3".into(), |z| z.trim().to_ascii_uppercase());
    let id = s.model_mut().insert(Element::Sheet(Sheet { number: number.clone(), name, size }));
    Ok(json!({"id": id, "number": number}))
}

fn place(s: &mut Session, p: &Value) -> R {
    let c = "place";
    let m = &s.model;
    let sheet = match p.get("sheet") {
        Some(Value::String(n)) => m
            .elements
            .iter()
            .find_map(|(id, e)| matches!(e, Element::Sheet(sh) if sh.number == *n || sh.name == *n).then_some(*id))
            .ok_or_else(|| bad(c, format!("no sheet numbered `{n}`")))?,
        Some(v) => v.as_u64().filter(|id| m.sheet(*id).is_some()).ok_or_else(|| bad(c, format!("`sheet`: {v} is not a sheet")))?,
        None => return Err(bad(c, "`sheet`, a sheet's number or id, is required")),
    };
    let scale = num(c, p, "scale")?;
    let at = point_opt(c, p, "at")?;
    let view = match p.get("view") {
        Some(Value::String(n)) => match m.elements.keys().find(|id| m.view_name(**id) == Some(n.as_str())) {
            Some(id) => *id,
            // A schedule, or the 3D view, comes to be as it's first placed: a schedule by its name in
            // English or in the project's language, named in the latter.
            None => {
                let builtin = schedule::builtin(Some(n), None);
                match (builtin, builtin.and_then(|(_, name)| schedule::named(m, name))) {
                    (_, Some(id)) => id,
                    (Some((of, name)), None) => {
                        let name = m.language.word(name).to_string();
                        s.model_mut().insert(Element::Schedule(Schedule { name, of: of.to_string(), ..Schedule::default() }))
                    }
                    (None, _) if n == "{3D}" => s.model_mut().insert(Element::View3D(View3D {
                        name: n.clone(),
                        azimuth: num(c, p, "azimuth")?.unwrap_or(view3d::AZIMUTH),
                        elevation: num(c, p, "elevation")?.unwrap_or(view3d::ELEVATION),
                        ..View3D::default_view()
                    })),
                    (None, _) => return Err(bad(c, format!("no level, section, elevation, schedule or 3D view named `{n}`"))),
                }
            }
        },
        Some(v) => serde_json::from_value::<Guid>(v.clone())
            .ok()
            .and_then(|g| m.elements.id(g))
            .filter(|id| m.view_name(*id).is_some())
            .ok_or_else(|| bad(c, format!("`view`: {v} is not a plan, a section, an elevation, a schedule or a 3D view")))?,
        None => return Err(bad(c, "`view`, a level's, section's, elevation's, schedule's or 3D view's name or id, is required")),
    };
    // The view's own scale, unless told another; 1 : 100 for a view with none.
    let scale = scale.unwrap_or_else(|| graphics::scale_of(&s.model, view));
    let at = at.unwrap_or_else(|| sheet::spot(&s.model, sheet, view, scale));
    let (sheet, view) = (s.model.guid(sheet), s.model.guid(view));
    let id = s.model_mut().insert(Element::Viewport(Viewport { sheet, view, at, scale }));
    Ok(json!({"id": id, "at": at}))
}

fn material(s: &mut Session, p: &Value) -> R {
    let c = "material";
    let name = string(c, p, "name")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).ok_or_else(|| bad(c, "`name` is required"))?;
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if !m.materials.contains_key(&name) {
            return Err(bad(c, format!("the library has no `{name}`")));
        }
        if let Some((cat, n, ..)) = m.layered_types().find(|(.., l)| l.iter().any(|l| l.material == name)) {
            return Err(bad(c, format!("`{name}` is in {cat} type `{n}`'s layers; take it out of them first")));
        }
        if let Some(user) = m.material_user(&name) {
            return Err(bad(c, format!("`{name}` is in {user}; take it out of it first")));
        }
        m.materials.remove(&name);
        prune_images(m);
        return Ok(json!({"deleted": name}));
    }
    let mut mat = m.materials.get(&name).cloned().unwrap_or_default();
    match p.get("texture") {
        None => {}
        Some(Value::Null) => mat.texture = None,
        Some(Value::Object(o)) => mat.texture = Some(texture_param(c, m, o, mat.texture.take())?),
        Some(v) => return Err(bad(c, format!("`texture` is {{kind, size?, angle?, offset?, path?}} or null, not {v}"))),
    }
    if let Some(v) = p.get("color").filter(|v| !v.is_null()) {
        mat.color = colour(v).ok_or_else(|| bad(c, format!("`color` is \"#rrggbb\" or [r, g, b], each 0 to 255, not {v}")))?;
    }
    // A pattern drawn in code by its name, or one of the project's fill patterns.
    for (key, slot, fill) in [("cut", &mut mat.cut, &mut mat.cut_pattern), ("surface", &mut mat.surface, &mut mat.surface_pattern)] {
        if let Some(v) = p.get(key).filter(|v| !v.is_null()) {
            if let Ok(pattern) = serde_json::from_value::<Pattern>(v.clone()) {
                (*slot, *fill) = (pattern, None);
            } else if let Some(n) = v.as_str().map(str::trim).filter(|n| m.fill_patterns.contains_key(*n)) {
                (*slot, *fill) = (Pattern::None, Some(n.to_string()));
            } else {
                let names: Vec<String> = Pattern::ALL.iter().map(|p| json!(p).as_str().unwrap_or_default().to_string()).collect();
                return Err(bad(
                    c,
                    format!("`{key}`: {v} isn't a pattern: one of {}, or a fill pattern of the project's", names.join(", ")),
                ));
            }
        }
    }
    let name = match string(c, p, "rename")? {
        Some(to) => {
            let to = to.trim().to_string();
            if to.is_empty() {
                return Err(bad(c, "`rename` needs a name"));
            }
            if to != name && m.materials.contains_key(&to) {
                return Err(bad(c, format!("the library already has `{to}`")));
            }
            m.materials.remove(&name);
            m.library.renamed(&LibKind::Material, &name, &to);
            m.layers_mut().filter(|l| l.material == name).for_each(|l| l.material.clone_from(&to));
            m.rename_material_in_families(&name, &to);
            to
        }
        None => name,
    };
    m.materials.insert(name.clone(), mat.clone());
    prune_images(m);
    Ok(json!({"name": name, "material": mat}))
}

/// The pictures no material's texture is made of, taken out of the project.
fn prune_images(m: &mut Model) {
    let used: BTreeSet<String> =
        m.materials.values().filter_map(|x| x.texture.as_ref()).filter(|t| t.kind == Kind::Image).map(|t| t.image.clone()).collect();
    m.images.retain(|id, _| used.contains(id));
}

/// Two numbers, `[a, b]`, or one for both.
fn pair(c: &str, key: &str, v: &Value) -> Result<[f64; 2], String> {
    let n = |x: &Value| {
        x.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(c, format!("texture `{key}` is [a, b] numbers, or one, not {v}")))
    };
    match v {
        Value::Array(a) if a.len() == 2 => Ok([n(&a[0])?, n(&a[1])?]),
        v => n(v).map(|x| [x, x]),
    }
}

/// A material's texture as the JSON `p` says over its `old` one: kind, size, angle, offset, and for a picture the file to take
/// into the project (`path`) or the project's own picture (`image`).
fn texture_param(c: &str, m: &mut Model, p: &Map<String, Value>, old: Option<Texture>) -> Result<Texture, String> {
    if let Some(k) = p.keys().find(|k| !["kind", "size", "angle", "offset", "path", "image"].contains(&k.as_str())) {
        return Err(bad(c, format!("texture: `{k}` isn't one of kind, size, angle, offset, path, image")));
    }
    let kinds: Vec<&str> = Kind::ALL.iter().map(|k| k.name()).collect();
    let kind = match p.get("kind").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value::<Kind>(v.clone())
            .map_err(|_| bad(c, format!("texture `kind`: {v} isn't one of {}", kinds.join(", "))))?,
        None => old.as_ref().map(|t| t.kind).ok_or_else(|| bad(c, format!("a texture needs a `kind`: one of {}", kinds.join(", "))))?,
    };
    // Over the old one while it's of this kind; else a new one at its natural size.
    let mut t = old.filter(|t| t.kind == kind).unwrap_or_else(|| Texture::new(kind));
    if let Some(v) = p.get("size").filter(|v| !v.is_null()) {
        t.size = pair(c, "size", v)?;
    }
    if let Some(v) = p.get("offset").filter(|v| !v.is_null()) {
        t.offset = pair(c, "offset", v)?;
    }
    if let Some(v) = p.get("angle").filter(|v| !v.is_null()) {
        t.angle = v.as_f64().ok_or_else(|| bad(c, format!("texture `angle` is a number of degrees, not {v}")))?;
    }
    match (kind, p.get("path").filter(|v| !v.is_null()), p.get("image").filter(|v| !v.is_null())) {
        (Kind::Image, Some(path), _) => {
            let path = PathBuf::from(path.as_str().ok_or_else(|| bad(c, format!("texture `path` is a file's path, not {path}")))?);
            let len = crate::disk::len(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
            if len > texture::MAX_BYTES as u64 {
                return Err(bad(c, format!("{} is too large ({len} bytes)", path.display())));
            }
            let bytes = crate::disk::read(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
            let stem = path.file_stem().map_or_else(|| "picture".into(), |s| s.to_string_lossy().into_owned());
            let picture = Picture::import(&stem, &bytes).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
            let (id, (w, h)) = (picture.id(), picture.size());
            if !m.images.contains_key(&id) && m.images.len() >= texture::MAX_IMAGES {
                return Err(bad(c, format!("the project holds {} pictures already; take a texture of one away first", m.images.len())));
            }
            m.images.insert(id.clone(), picture);
            t.image = id;
            // A new picture covers a tile a metre across, as tall as its proportions make it.
            if p.get("size").is_none_or(Value::is_null) {
                t.size = [1000.0, 1000.0 * h as f64 / w.max(1) as f64];
            }
        }
        (Kind::Image, None, Some(image)) => {
            t.image = image.as_str().ok_or_else(|| bad(c, format!("texture `image` is a picture's id, not {image}")))?.to_string();
        }
        (Kind::Image, None, None) if t.image.is_empty() => {
            return Err(bad(c, "a texture of a picture needs its file, `path`, or one of the project's, `image`"));
        }
        (Kind::Image, None, None) => {}
        (_, None, None) => t.image.clear(),
        _ => return Err(bad(c, "only a texture of kind `image` takes a `path` or an `image`")),
    }
    t.check(&m.images).map_err(|e| bad(c, format!("texture: {e}")))?;
    Ok(t)
}

fn paint(s: &mut Session, p: &Value) -> R {
    let c = "paint";
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let face = string(c, p, "face")?;
    let material = match p.get("material") {
        None => return Err(bad(c, "`material` is required: a material's name, or null to take the paint off")),
        Some(Value::Null) => None,
        Some(Value::String(n)) => Some(n.trim().to_string()),
        Some(v) => return Err(bad(c, format!("`material` is a material's name, or null, not {v}"))),
    };
    if let Some(n) = material.as_ref().filter(|n| !m.materials.contains_key(*n)) {
        return Err(bad(c, format!("the library has no `{n}`; add it with `material`")));
    }
    let m = s.model_mut();
    for &id in &ids {
        let e = m.elements.get_mut(id).ok_or_else(|| bad(c, format!("no element {id}")))?;
        let (category, faces) = (e.category(), paint::faces(e.category()));
        let Some(params) = e.params_mut().filter(|_| !faces.is_empty()) else {
            return Err(bad(c, format!("{category} {id} has no face to paint")));
        };
        let touched: Vec<&str> = match face.as_deref() {
            Some(f) if faces.contains(&f) => vec![f],
            Some(f) => return Err(bad(c, format!("a {category} has no face `{f}`; its faces are {}", faces.join(", ")))),
            None => faces.to_vec(),
        };
        let mut now = params.get(paint::KEY).and_then(Value::as_object).cloned().unwrap_or_default();
        for f in touched {
            match &material {
                Some(n) => now.insert(f.to_string(), json!(n)),
                None => now.remove(f),
            };
        }
        if now.is_empty() {
            params.remove(paint::KEY);
        } else {
            params.insert(paint::KEY.into(), Value::Object(now));
        }
    }
    Ok(json!({"painted": ids}))
}

fn parameter(s: &mut Session, p: &Value) -> R {
    let c = "parameter";
    let name = string(c, p, "name")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).ok_or_else(|| bad(c, "`name` is required"))?;
    let m = s.model_mut();
    // Off the categories `gone`, the ones it was for: their elements' values of `name`, and their
    // schedules' column of it, and filtering and sorting by it. A component's value of its family's
    // parameter of that name is its family's, and stays.
    let clear = |m: &mut Model, name: &str, gone: &[String]| {
        let Model { families, elements, .. } = m;
        for e in elements.values_mut() {
            if let Element::Schedule(sc) = e {
                if gone.contains(&sc.category()) {
                    sc.fields.retain(|f| f.name != name || f.formula.is_some());
                    sc.filters.retain(|f| f.field != name);
                    sc.sort.retain(|s| s.field != name);
                }
                continue;
            }
            let (cat, family) = (e.category(), e.family().and_then(|(f, _)| families.get(f)));
            if !gone.iter().any(|g| g == cat) || family.is_some_and(|f| f.parameters.iter().any(|q| q.name == name)) {
                continue;
            }
            if let Some(params) = e.params_mut() {
                params.remove(name);
            }
        }
    };
    // A view filter with a rule on it keeps it: taking it away would change what the filter picks out.
    let in_use = |m: &Model, name: &str, gone: &[String]| {
        m.view_filters
            .iter()
            .find(|(_, f)| f.categories.iter().any(|k| gone.contains(k)) && f.rules.iter().any(|r| r.field == name))
            .map(|(n, _)| bad(c, format!("view filter `{n}` has a rule on `{name}`; take the rule off before the parameter goes")))
    };
    if flag(c, p, "delete")? {
        let Some(was) = m.parameters.get(&name).cloned() else {
            return Err(bad(c, format!("there's no parameter `{name}`")));
        };
        if let Some(e) = in_use(m, &name, &was.categories) {
            return Err(e);
        }
        m.parameters.remove(&name);
        clear(m, &name, &was.categories);
        return Ok(json!({"deleted": name}));
    }
    let mut param: ProjectParameter = m.parameters.get(&name).cloned().unwrap_or_default();
    if let Some(v) = p.get("kind").filter(|v| !v.is_null()) {
        let kind: ParameterKind =
            serde_json::from_value(v.clone()).map_err(|_| bad(c, format!("`kind`: {v} isn't one of text, number, length, yesno")))?;
        // Values of it: its categories' elements' own (a component's of its family's parameter of
        // that name aren't).
        let Model { families, elements, .. } = &*m;
        let valued = elements.values().any(|e| {
            let theirs = e.family().and_then(|(f, _)| families.get(f)).is_some_and(|f| f.parameters.iter().any(|q| q.name == name));
            param.categories.iter().any(|k| k == e.category()) && !theirs && e.params().is_some_and(|ps| ps.contains_key(&name))
        });
        if kind != param.kind && valued {
            return Err(bad(c, format!("`{name}` has values of its kind; take them away before it holds another")));
        }
        param.kind = kind;
    }
    if let Some(v) = p.get("categories").filter(|v| !v.is_null()) {
        let list: Vec<String> =
            serde_json::from_value(v.clone()).map_err(|_| bad(c, format!("`categories` is a list of categories, not {v}")))?;
        if let Some(k) = list.iter().find(|k| !PARAMETER_CATEGORIES.contains(&category(k).as_str())) {
            return Err(bad(c, format!("`{k}` isn't a category it can be for; there are {}", PARAMETER_CATEGORIES.join(", "))));
        }
        let mut list: Vec<String> = list.iter().map(|k| category(k)).collect();
        list.sort();
        list.dedup();
        let gone: Vec<String> = param.categories.iter().filter(|k| !list.contains(k)).cloned().collect();
        if let Some(e) = in_use(m, &name, &gone) {
            return Err(e);
        }
        clear(m, &name, &gone);
        param.categories = list;
    }
    let name = match string(c, p, "rename")? {
        Some(to) => {
            let to = to.trim().to_string();
            if to.is_empty() {
                return Err(bad(c, "`rename` needs a name"));
            }
            if to != name && m.parameters.contains_key(&to) {
                return Err(bad(c, format!("there's a parameter `{to}` already")));
            }
            m.parameters.remove(&name);
            m.library.renamed(&LibKind::Parameter, &name, &to);
            for f in m.view_filters.values_mut().filter(|f| f.categories.iter().any(|k| param.categories.contains(k))) {
                f.rules.iter_mut().filter(|r| r.field == name).for_each(|r| r.field = to.clone());
            }
            for e in m.elements.values_mut() {
                if let Element::Schedule(sc) = e {
                    if param.categories.contains(&category(&sc.of)) {
                        let swap = |n: &mut String| {
                            if *n == name {
                                *n = to.clone();
                            }
                        };
                        sc.fields.iter_mut().filter(|f| f.formula.is_none()).for_each(|f| swap(&mut f.name));
                        sc.filters.iter_mut().for_each(|f| swap(&mut f.field));
                        sc.sort.iter_mut().for_each(|s| swap(&mut s.field));
                    }
                    continue;
                }
                let cat = e.category();
                if param.categories.iter().any(|k| k == cat)
                    && let Some(params) = e.params_mut()
                    && let Some(v) = params.remove(&name)
                {
                    params.insert(to.clone(), v);
                }
            }
            to
        }
        None => name,
    };
    m.parameters.insert(name.clone(), param.clone());
    Ok(json!({"name": name, "parameter": param}))
}

fn family(s: &mut Session, p: &Value) -> R {
    let c = "family";
    // From a file: its fields under the name given, else its own, else the file's.
    let mut p = p.clone();
    if let Some(path) = string(c, &p, "path")? {
        let path = PathBuf::from(path);
        let size = crate::disk::len(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        if size >= MAX_FILE {
            return Err(bad(c, format!("{} is too large ({size} bytes)", path.display())));
        }
        let text = crate::disk::read_to_string(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        let loaded: Value = serde_json::from_str(&text).map_err(|e| bad(c, format!("{} is not a family: {e}", path.display())))?;
        let Value::Object(mut fields) = loaded else {
            return Err(bad(c, format!("{} is not a family: it isn't an object", path.display())));
        };
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let name = p.get("name").filter(|v| !v.is_null()).cloned().or_else(|| fields.remove("name")).unwrap_or(json!(stem));
        // Only the family it holds: what a file says to rename or delete isn't for it to say.
        let mut family: Map<String, Value> = ["category", "parameters", "types", "forms", "symbols"]
            .into_iter()
            .filter_map(|k| fields.remove(k).map(|v| (k.to_string(), v)))
            .collect();
        family.insert("name".into(), name);
        p = Value::Object(family);
    }
    let name =
        string(c, &p, "name")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).ok_or_else(|| bad(c, "`name` is required"))?;
    // Saved to a file a family of the project's is: the file `path` loads it back from.
    if let Some(file) = string(c, &p, "save")? {
        let f = s.model.families.get(&name).ok_or_else(|| bad(c, format!("there's no family `{name}` to save")))?;
        let mut v = serde_json::to_value(f).map_err(|e| bad(c, e))?;
        v["name"] = json!(name);
        let text = serde_json::to_string_pretty(&v).map_err(|e| bad(c, e))?;
        crate::disk::write(&file, text).map_err(|e| bad(c, format!("{file}: {e}")))?;
        return Ok(json!({"saved": name, "path": file}));
    }
    let m = s.model_mut();
    let users = |m: &Model, name: &str| -> Vec<Id> {
        m.elements.iter().filter(|(_, e)| e.family().is_some_and(|(f, _)| f == name)).map(|(id, _)| *id).collect()
    };
    if flag(c, &p, "delete")? {
        if !m.families.contains_key(&name) {
            return Err(bad(c, format!("there's no family `{name}`")));
        }
        let used = users(m, &name);
        if !used.is_empty() {
            let ids: Vec<String> = used.iter().map(Id::to_string).collect();
            return Err(bad(c, format!("elements {} are of `{name}`; delete them first", ids.join(", "))));
        }
        m.families.remove(&name);
        return Ok(json!({"deleted": name}));
    }
    let mut f = m.families.get(&name).cloned().unwrap_or_default();
    let field = |key: &str, e: serde_json::Error| bad(c, format!("`{key}`: {e}"));
    if let Some(v) = p.get("category").filter(|v| !v.is_null()) {
        f.category = serde_json::from_value(v.clone()).map_err(|e| field("category", e))?;
    }
    if let Some(v) = p.get("parameters").filter(|v| !v.is_null()) {
        f.parameters = serde_json::from_value(v.clone()).map_err(|e| field("parameters", e))?;
        if let Some(q) = f.parameters.iter().find(|q| paint::reserved(&q.name)) {
            return Err(bad(c, format!("parameter `{}`: an element holds its paint under that name; name it otherwise", q.name)));
        }
        if let Some(q) = f.parameters.iter().find(|q| massing::reserved(&q.name)) {
            return Err(bad(c, format!("parameter `{}`: a component holds nothing of a mass under that name; name it otherwise", q.name)));
        }
        if let Some(q) = f.parameters.iter().find(|q| phasing::reserved(&q.name)) {
            return Err(bad(
                c,
                format!("parameter `{}`: a component holds its phase and its design option under that name; name it otherwise", q.name),
            ));
        }
    }
    if let Some(v) = p.get("forms").filter(|v| !v.is_null()) {
        let mut forms: Vec<crate::family::Form> = serde_json::from_value(v.clone()).map_err(|e| field("forms", e))?;
        crate::library::keep_parts(&mut f.forms, &mut forms);
        f.forms = forms;
    }
    if let Some(v) = p.get("symbols").filter(|v| !v.is_null()) {
        let mut symbols: Vec<crate::family::Symbol> = serde_json::from_value(v.clone()).map_err(|e| field("symbols", e))?;
        crate::library::keep_parts(&mut f.symbols, &mut symbols);
        f.symbols = symbols;
    }
    if let Some(v) = p.get("types").filter(|v| !v.is_null()) {
        let mut types = serde_json::to_value(&f.types).map_err(|e| bad(c, e))?;
        merge(&mut types, v);
        f.types = serde_json::from_value(types).map_err(|e| field("types", e))?;
    }
    let name = match string(c, &p, "rename")? {
        Some(to) => {
            let to = to.trim().to_string();
            if to.is_empty() {
                return Err(bad(c, "`rename` needs a name"));
            }
            if to != name && m.families.contains_key(&to) {
                return Err(bad(c, format!("there's a family `{to}` already")));
            }
            m.families.remove(&name);
            m.library.family_renamed(&name, &to);
            for e in m.elements.values_mut() {
                if let Some(f) = e.family_mut()
                    && *f == name
                {
                    f.clone_from(&to);
                }
            }
            to
        }
        None => name,
    };
    m.families.insert(name.clone(), f.clone());
    Ok(json!({"name": name, "family": f}))
}

fn component(s: &mut Session, p: &Value) -> R {
    let c = "component";
    let m = &s.model;
    let family = string(c, p, "family")?.ok_or_else(|| bad(c, "`family` is required; `inspect` lists the families"))?;
    let f = m.families.get(&family).ok_or_else(|| {
        let names: Vec<&str> = m.families.keys().map(String::as_str).collect();
        bad(c, format!("family `{family}` doesn't exist (there are: {})", names.join(", ")))
    })?;
    let type_name = match string(c, p, "type")? {
        Some(t) => t,
        None => f.types.keys().next().cloned().ok_or_else(|| bad(c, format!("family `{family}` has no type")))?,
    };
    let params = match p.get("params") {
        Some(Value::Object(o)) => o.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), v.clone())).collect(),
        v if absent(v) => BTreeMap::new(),
        v => return Err(bad(c, format!("`params` is {{parameter: value}}, not {}", v.unwrap_or(&Value::Null)))),
    };
    let comp = Component {
        family,
        type_name,
        level: level_param(c, m, p, "level")?,
        at: point_req(c, p, "at")?,
        angle: num(c, p, "angle")?.unwrap_or(0.0),
        offset: num(c, p, "offset")?.unwrap_or(0.0),
        mirrored: flag(c, p, "mirrored")?,
        params,
    };
    let id = s.model_mut().insert(Element::Component(comp));
    Ok(json!({"id": id}))
}

/// The project's pictures, each its name and its size in pixels.
fn images(m: &Model) -> Value {
    Value::Object(m.images.iter().map(|(id, p)| (id.clone(), json!({"name": p.name(), "size": [p.size().0, p.size().1]}))).collect())
}

fn favorite(s: &mut Session, p: &Value) -> R {
    let c = "favorite";
    let action = string(c, p, "action")?.unwrap_or_else(|| "list".into());
    let kind = string(c, p, "kind")?.unwrap_or_else(|| "material".into());
    if !matches!(kind.as_str(), "material" | "family") {
        return Err(bad(c, format!("`kind` is material or family, not `{kind}`")));
    }
    let path = match string(c, p, "path")? {
        Some(path) => PathBuf::from(path),
        None => favorites::default_path().ok_or_else(|| bad(c, "there's no folder to keep favorites in here: give `path`"))?,
    };
    let mut kept = Favorites::read(&path).map_err(|e| bad(c, e))?;
    let clean = |n: String| Some(n.trim().to_string()).filter(|n| !n.is_empty());
    let name = string(c, p, "name")?.and_then(clean);
    let rename = string(c, p, "as")?.and_then(clean);
    let need = || name.clone().ok_or_else(|| bad(c, format!("`name` is required: the {kind}'s")));
    match action.as_str() {
        "list" => {
            let materials: Vec<Value> = kept
                .materials
                .iter()
                .map(|(n, e)| json!({"name": n, "color": e.material.color, "texture": e.material.texture.as_ref().map(|t| t.kind.name())}))
                .collect();
            let families: Vec<Value> = kept
                .families
                .iter()
                .map(|(n, e)| json!({"name": n, "category": e.family.category, "types": e.family.types.keys().collect::<Vec<_>>()}))
                .collect();
            Ok(json!({"path": path, "materials": materials, "families": families}))
        }
        "save" => {
            let name = need()?;
            let kept_as = rename.unwrap_or_else(|| name.clone());
            if kind == "material" {
                let entry = MaterialEntry::of(&s.model, &name).ok_or_else(|| bad(c, format!("the library has no `{name}`")))?;
                kept.materials.insert(kept_as.clone(), entry);
            } else {
                let entry = FamilyEntry::of(&s.model, &name).ok_or_else(|| bad(c, format!("the project has no family `{name}`")))?;
                kept.families.insert(kept_as.clone(), entry);
            }
            kept.version = 1;
            kept.check().and_then(|()| kept.write(&path)).map_err(|e| bad(c, e))?;
            Ok(json!({"saved": kept_as, "kind": kind, "path": path}))
        }
        "delete" => {
            let name = need()?;
            let gone = if kind == "material" { kept.materials.remove(&name).is_some() } else { kept.families.remove(&name).is_some() };
            if !gone {
                return Err(bad(c, format!("there's no favorite {kind} `{name}`")));
            }
            kept.write(&path).map_err(|e| bad(c, e))?;
            Ok(json!({"deleted": name, "kind": kind}))
        }
        "load" => {
            let name = need()?;
            let as_name = rename.unwrap_or_else(|| name.clone());
            let replace = flag(c, p, "replace")?;
            let m = s.model_mut();
            if kind == "material" {
                let entry = kept.materials.get(&name).ok_or_else(|| bad(c, format!("there's no favorite material `{name}`")))?;
                if m.materials.contains_key(&as_name) && !replace {
                    return Err(bad(c, format!("the library has `{as_name}` already: load it `as` another name, or `replace` it")));
                }
                entry.bring_into(m, &as_name).map_err(|e| bad(c, e))?;
                prune_images(m);
            } else {
                let entry = kept.families.get(&name).ok_or_else(|| bad(c, format!("there's no favorite family `{name}`")))?;
                if m.families.contains_key(&as_name) && !replace {
                    return Err(bad(c, format!("the project has family `{as_name}` already: load it `as` another name, or `replace` it")));
                }
                // The materials it's made of that the project hasn't come with it; the project's own of the same name stay.
                for (n, material) in &entry.materials {
                    if !m.materials.contains_key(n) {
                        material.bring_into(m, n).map_err(|e| bad(c, e))?;
                    }
                }
                m.families.insert(as_name.clone(), entry.family.clone());
                m.adopt_materials();
            }
            Ok(json!({"loaded": as_name, "kind": kind}))
        }
        other => Err(bad(c, format!("`action` is list, save, load or delete, not `{other}`"))),
    }
}

/// The families, each its category, its types and its parameters' names.
fn families(m: &Model) -> Value {
    let all: Map<String, Value> = m
        .families
        .iter()
        .map(|(n, f)| {
            let parameters: Vec<&str> = f.parameters.iter().map(|p| p.name.as_str()).collect();
            (n.clone(), json!({"category": f.category, "types": f.types.keys().collect::<Vec<_>>(), "parameters": parameters}))
        })
        .collect();
    Value::Object(all)
}

/// A family's parameters' values, worked out, as JSON.
fn values_json(v: BTreeMap<String, crate::family::Val>) -> Value {
    Value::Object(
        v.into_iter()
            .map(|(k, v)| match v {
                crate::family::Val::Number(x) => (k, json!(x)),
                crate::family::Val::Text(t) => (k, json!(t)),
            })
            .collect::<Map<String, Value>>(),
    )
}

/// `"#rrggbb"`, or `[r, g, b]`, each 0 to 255.
pub(crate) fn colour(v: &Value) -> Option<[u8; 3]> {
    match v {
        Value::String(hex) => {
            let hex = hex.strip_prefix('#').filter(|h| h.len() == 6 && h.is_ascii())?;
            let byte = |i: usize| hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok());
            Some([byte(0)?, byte(2)?, byte(4)?])
        }
        Value::Array(a) => match a.as_slice() {
            [r, g, b] => {
                let byte = |x: &Value| x.as_u64().and_then(|x| u8::try_from(x).ok());
                Some([byte(r)?, byte(g)?, byte(b)?])
            }
            _ => None,
        },
        _ => None,
    }
}

fn edit_type(s: &mut Session, p: &Value) -> R {
    let c = "type";
    let cat = category(&string(c, p, "category")?.ok_or_else(|| bad(c, "`category` is required: wall, floor, ceiling, door or window"))?);
    let name = string(c, p, "name")?.filter(|n| !n.trim().is_empty()).ok_or_else(|| bad(c, "`name` is required"))?;
    let (thickness, width, height, sill) = (num(c, p, "thickness")?, num(c, p, "width")?, num(c, p, "height")?, num(c, p, "sill")?);
    let need = |what: &str| bad(c, format!("a new {cat} type needs `{what}`"));
    let m = s.model_mut();
    let t = match cat.as_str() {
        "wall" => {
            let (thickness, layers) = layered(c, p, m.types.wall.get(&name).map(|t| (t.thickness, t.layers.as_slice())), thickness, &need)?;
            let t = WallType { thickness, layers };
            m.types.wall.insert(name.clone(), t.clone());
            json!(t)
        }
        "floor" => {
            let (thickness, layers) =
                layered(c, p, m.types.floor.get(&name).map(|t| (t.thickness, t.layers.as_slice())), thickness, &need)?;
            let t = FloorType { thickness, layers };
            m.types.floor.insert(name.clone(), t.clone());
            json!(t)
        }
        "door" | "window" => {
            let map = if cat == "door" { &mut m.types.door } else { &mut m.types.window };
            let old = map.get(&name).cloned();
            let t = OpeningType {
                width: width.or(old.as_ref().map(|t| t.width)).ok_or_else(|| need("width"))?,
                height: height.or(old.as_ref().map(|t| t.height)).ok_or_else(|| need("height"))?,
                sill: sill.or(old.as_ref().map(|t| t.sill)).unwrap_or(if cat == "window" { 900.0 } else { 0.0 }),
            };
            map.insert(name.clone(), t);
            json!(t)
        }
        "column" | "structuralcolumn" | "footing" => {
            let types = match cat.as_str() {
                "structuralcolumn" => &mut m.types.structural_column,
                "footing" => &mut m.types.footing,
                _ => &mut m.types.column,
            };
            let old = types.get(&name).cloned();
            let width = width.or(old.as_ref().map(|t| t.width)).ok_or_else(|| need("width"))?;
            let round = match p.get("round") {
                v if absent(v) => old.as_ref().is_some_and(|t| t.round),
                _ => flag(c, p, "round")?,
            };
            let depth = if round { width } else { num(c, p, "depth")?.or(old.as_ref().map(|t| t.depth)).unwrap_or(width) };
            let t = ColumnType { width, depth, round };
            types.insert(name.clone(), t.clone());
            json!(t)
        }
        // A beam's or another run's: a section `width` across by `depth` deep, or round; made of a material.
        run if crate::model::RUN_CATEGORIES.contains(&run) => {
            let types = m.types.runs.entry(cat.clone()).or_default();
            let old = types.get(&name).cloned();
            let width = width.or(old.as_ref().map(|t| t.width)).ok_or_else(|| need("width"))?;
            let round = match p.get("round") {
                v if absent(v) => old.as_ref().is_some_and(|t| t.round),
                _ => flag(c, p, "round")?,
            };
            let depth = if round { width } else { num(c, p, "depth")?.or(old.as_ref().map(|t| t.depth)).ok_or_else(|| need("depth"))? };
            let material = string(c, p, "material")?.or(old.map(|t| t.material)).unwrap_or_default();
            let t = crate::model::RunType { width, depth, round, material };
            types.insert(name.clone(), t.clone());
            json!(t)
        }
        "roof" => {
            let (thickness, layers) = layered(c, p, m.types.roof.get(&name).map(|t| (t.thickness, t.layers.as_slice())), thickness, &need)?;
            let t = RoofType { thickness, layers };
            m.types.roof.insert(name.clone(), t.clone());
            json!(t)
        }
        "railing" => {
            let old = m.types.railing.get(&name).cloned();
            let t = RailingType {
                height: height.or(old.as_ref().map(|t| t.height)).ok_or_else(|| need("height"))?,
                spacing: num(c, p, "spacing")?.or(old.as_ref().map(|t| t.spacing)).unwrap_or(0.0),
            };
            m.types.railing.insert(name.clone(), t.clone());
            json!(t)
        }
        "ceiling" => {
            let old = m.types.ceiling.get(&name).cloned();
            let (thickness, layers) = layered(c, p, old.as_ref().map(|t| (t.thickness, t.layers.as_slice())), thickness, &need)?;
            let t = CeilingType { thickness, grid: num(c, p, "grid")?.or(old.as_ref().map(|t| t.grid)).unwrap_or(0.0), layers };
            m.types.ceiling.insert(name.clone(), t.clone());
            json!(t)
        }
        other => {
            return Err(bad(
                c,
                format!(
                    "`{other}` has no types; use wall, floor, ceiling, door, window, column, structural column, footing, beam, \
                     strip footing, pipe, conduit, cable tray, wire, down conductor, earthing conductor, cable, roof or railing"
                ),
            ));
        }
    };
    Ok(json!({"category": cat, "name": name, "type": t}))
}

/// A layered type's thickness and layers from `type`'s params, over the one named that way already
/// (`old`). With layers, it's as thick as they are; a new thickness alone goes to its first
/// structure layer (else its thickest).
fn layered(
    c: &str,
    p: &Value,
    old: Option<(f64, &[Layer])>,
    thickness: Option<f64>,
    need: &dyn Fn(&str) -> String,
) -> Result<(f64, Vec<Layer>), String> {
    let layers: Option<Vec<Layer>> = match p.get("layers") {
        v if absent(v) => None,
        Some(v @ Value::Array(a)) if a.len() <= crate::model::MAX_LAYERS => {
            Some(serde_json::from_value(v.clone()).map_err(|e| bad(c, format!("`layers`: {e}")))?)
        }
        Some(_) => return Err(bad(c, format!("`layers` is a list of at most {} layers", crate::model::MAX_LAYERS))),
        None => None,
    };
    // Each layer at a place it had keeps its id and GUID.
    let layers = layers.map(|mut new| {
        crate::library::keep_parts(&mut old.map(|(_, l)| l.to_vec()).unwrap_or_default(), &mut new);
        new
    });
    match (layers, thickness) {
        (Some(layers), given) if !layers.is_empty() => {
            let sum: f64 = layers.iter().map(|l| l.thickness).sum();
            if let Some(t) = given.filter(|t| (t - sum).abs() > 1e-6) {
                return Err(bad(c, format!("its layers add up to {sum} mm, not the {t} mm given")));
            }
            Ok((sum, layers))
        }
        (Some(_), None) => Err(need("thickness")),
        (Some(_), Some(t)) => Ok((t, Vec::new())),
        (None, Some(t)) => {
            let mut layers = old.map(|(_, l)| l.to_vec()).unwrap_or_default();
            if let Some((was, _)) = old.filter(|(_, l)| !l.is_empty()) {
                let core = layers.iter().position(|l| l.function == crate::model::Function::Structure);
                let k = core.or_else(|| (0..layers.len()).max_by(|a, b| layers[*a].thickness.total_cmp(&layers[*b].thickness)));
                if let Some(l) = k.and_then(|k| layers.get_mut(k)) {
                    l.thickness += t - was;
                    if l.thickness <= 0.0 {
                        return Err(bad(c, format!("at {t} mm its structure would be {} mm thick; change its layers", l.thickness)));
                    }
                }
            }
            Ok((t, layers))
        }
        (None, None) => old.map(|(t, l)| (t, l.to_vec())).ok_or_else(|| need("thickness")),
    }
}

fn modify(s: &mut Session, p: &Value) -> R {
    let c = "modify";
    let fields = p.as_object().ok_or_else(|| bad(c, "params must be {id | ids, …fields}"))?;
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let (mut edited, mut rescaled) = (Vec::new(), Vec::new());
    for &id in &ids {
        let e = m.elements.get(id).ok_or_else(|| bad(c, format!("no element {id}")))?;
        let mut v = serde_json::to_value(e).map_err(|e| bad(c, e))?;
        let obj = v.as_object_mut().ok_or_else(|| bad(c, format!("element {id} can't be edited")))?;
        for (k, val) in fields {
            let val = match k.as_str() {
                "id" | "ids" => continue,
                "category" if obj.get("category") == Some(val) => continue,
                "category" => return Err(bad(c, "`category` can't change; delete the element and create another")),
                "level" | "top_level" if val.is_string() => json!(level_param(c, m, p, k)?),
                _ => val.clone(),
            };
            match obj.get_mut(k) {
                // Into a field that is an object (a plan's view range), the new one merges.
                Some(old) if old.is_object() && val.is_object() => merge(old, &val),
                _ => {
                    let mut new = Value::Null;
                    merge(&mut new, &val);
                    obj.insert(k.clone(), new);
                }
            }
        }
        let mut e2 = serde_json::from_value::<Element>(v).map_err(|e| bad(c, format!("element {id}: {e}")))?;
        graphics::check_followed(m, e, &e2).map_err(|err| bad(c, format!("element {id}: {err}")))?;
        // A wall put on another of its lines stays where it is: its line moves across it instead.
        if let (Element::Wall(old), Element::Wall(w)) = (e, &mut e2)
            && old.location != w.location
            && !fields.contains_key("start")
            && !fields.contains_key("end")
        {
            let (before, after) = (walls::faces(m, &Wall { location: old.location, ..w.clone() }).0, walls::faces(m, w).0);
            let axis = Axis::of(w);
            let length = axis.length();
            (w.start, w.end) = (micron(axis.at(0.0, before - after)), micron(axis.at(length, before - after)));
            // Along an arc, its doors' and windows' offsets grow or shrink with it.
            let k = Axis::of(w).length() / length.max(1e-9);
            if (k - 1.0).abs() > 1e-12 {
                for (oid, oe) in &m.elements {
                    if let Element::Door(o) | Element::Window(o) = oe
                        && m.same(o.host, id)
                    {
                        let mut o = o.clone();
                        o.offset *= k;
                        rescaled.push((*oid, if matches!(oe, Element::Door(_)) { Element::Door(o) } else { Element::Window(o) }));
                    }
                }
            }
        }
        edited.push((id, e2));
    }
    edited.extend(rescaled);
    let m = s.model_mut();
    for (id, e) in edited {
        m.elements.insert(id, e);
    }
    match (p.get("ids"), ids.as_slice()) {
        (None, [one]) => Ok(element_json(&s.model, *one)),
        _ => Ok(json!({"modified": ids})),
    }
}

fn match_properties(s: &mut Session, p: &Value) -> R {
    let c = "match_properties";
    let m = &s.model;
    let from = element_id(c, m, p.get("from").ok_or_else(|| bad(c, "`from` (an element id or GUID) is required"))?)?;
    let source = m.elements.get(from).ok_or_else(|| bad(c, format!("no element {from}")))?;
    let v = serde_json::to_value(source).map_err(|e| bad(c, e))?;
    let typ =
        v.get("type").and_then(Value::as_str).filter(|t| !t.is_empty()).ok_or_else(|| bad(c, format!("element {from} has no type")))?;
    let mut fields = json!({"type": typ});
    if let Some(family) = v.get("family").and_then(Value::as_str) {
        fields["family"] = json!(family);
    }
    let (matched, skipped): (Vec<Id>, Vec<Id>) = ids_param(c, m, p)?
        .into_iter()
        .filter(|id| *id != from)
        .partition(|id| m.elements.get(*id).is_some_and(|e| e.category() == source.category()));
    if matched.is_empty() {
        return Err(bad(c, format!("none of the elements is in the category of element {from} ({})", source.category())));
    }
    fields["ids"] = json!(matched);
    modify(s, &fields)?;
    Ok(json!({"modified": matched, "skipped": skipped}))
}

/// `patch` merged into `into`, as JSON Merge Patch (RFC 7386) does: objects merge key by key, a
/// null removes its key, anything else replaces what was there.
fn merge(into: &mut Value, patch: &Value) {
    let Value::Object(b) = patch else {
        *into = patch.clone();
        return;
    };
    // Into what isn't an object yet, a new one: its nulls are values taken away, never kept.
    if !into.is_object() {
        *into = Value::Object(Map::new());
    }
    if let Value::Object(a) = into {
        for (k, v) in b {
            if v.is_null() {
                a.remove(k);
            } else {
                merge(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
    }
}

/// `p` to the micron: what a point meant to be exact is, without float noise.
pub(crate) fn micron(p: P2) -> P2 {
    // `+ 0.0`: no `-0` in files.
    P2((p.0 * 1e3).round() / 1e3 + 0.0, (p.1 * 1e3).round() / 1e3 + 0.0)
}

/// How a command takes elements: through `map`, a translation, a turn or a mirror. A translation
/// slides a door or window on its own along its wall by `slide`; a mirror flips the way doors and
/// windows face, as a mirror image does.
pub(crate) struct Motion<'a> {
    pub(crate) map: &'a dyn Fn(P2) -> P2,
    pub(crate) slide: Option<P2>,
    pub(crate) mirror: bool,
}

/// The element taken by `mo`. Levels have no place in plan; a door or window on its own goes
/// only along its wall, by a translation, else only with its wall.
pub(crate) fn transformed(c: &str, m: &Model, id: Id, mo: &Motion, with: &BTreeSet<Id>) -> Result<Element, String> {
    let e = m.elements.get(id).cloned().ok_or_else(|| bad(c, format!("no element {id}")))?;
    transform(c, m, e, id, mo, with)
}

/// Element `e`, element `id` of `m` or a detail group's member, taken by `mo`, as [`transformed`].
pub(crate) fn transform(c: &str, m: &Model, mut e: Element, id: Id, mo: &Motion, with: &BTreeSet<Id>) -> Result<Element, String> {
    let f = mo.map;
    let within = |g: Guid| m.elements.id(g).is_some_and(|i| with.contains(&i));
    let category = e.category();
    // Its sketch goes with it, line for line.
    if let Some(lines) = e.sketch_mut() {
        lines.iter_mut().for_each(|l| *l = l.map(f));
    }
    match &mut e {
        Element::Level(_) => return Err(bad(c, format!("level {id} has no place in plan; change its elevation, or add a level"))),
        Element::Sheet(_) => return Err(bad(c, format!("sheet {id} has no place in plan; move the views on it"))),
        Element::Schedule(_) | Element::View3D(_) | Element::PlanView(_) => {
            return Err(bad(c, format!("{category} {id} has no place in plan; move its viewport on its sheet")));
        }
        Element::PointCloud(pc) => {
            if mo.mirror {
                return Err(bad(c, format!("point cloud {id} can't be mirrored: a scan is what was measured; move it or turn it")));
            }
            (pc.at, pc.angle) = turned(f, pc.at, pc.angle);
        }
        Element::Text(n) => {
            let (at, angle) = turned(f, n.at, n.angle);
            // A mirrored note still reads the right way round.
            (n.at, n.angle) = (at, if mo.mirror { n.angle } else { angle });
            n.leader = n.leader.map(f);
        }
        Element::Spot(sp) => sp.at = f(sp.at),
        Element::DetailLine(l) => l.curve = l.curve.map(|p| micron(f(p))),
        // Its sketch went with it, above; its pattern goes too.
        Element::FilledRegion(r) => (r.pattern_origin, r.pattern_angle) = turned(f, r.pattern_origin, r.pattern_angle),
        // Its members go with it; a mirror image is its type's, flipped.
        Element::DetailGroup(g) => {
            (g.at, g.angle) = turned(f, g.at, g.angle);
            g.mirrored ^= mo.mirror;
        }
        Element::RepeatingDetail(r) => {
            (r.start, r.end) = (f(r.start), f(r.end));
            r.mirrored ^= mo.mirror;
        }
        // A link is moved and turned as a whole; its file stays as it is.
        Element::Link(l) => (l.at, l.angle) = turned(f, l.at, l.angle),
        Element::Viewport(v) => v.at = f(v.at),
        Element::Column(col) | Element::StructuralColumn(col) | Element::Footing(col) => (col.at, col.angle) = turned(f, col.at, col.angle),
        // A run keeps its heights at its ends; its section is the same either way round.
        crate::model::run_arm!(r) => (r.start, r.end) = (f(r.start), f(r.end)),
        // A mirror image is the family's, flipped.
        Element::Component(cp) => {
            (cp.at, cp.angle) = turned(f, cp.at, cp.angle);
            cp.mirrored ^= mo.mirror;
        }
        // A mirror image turns the other way at its landings.
        Element::Stair(Stair { start, angle, landings, .. }) | Element::Ramp(Ramp { start, angle, landings, .. }) => {
            (*start, *angle) = turned(f, *start, *angle);
            if mo.mirror {
                landings.iter_mut().for_each(|l| l.turn = -l.turn + 0.0);
            }
        }
        Element::Railing(rl) => match rl.host {
            // Its stair's or ramp's, mirrored, its sides swap.
            Some(h) if within(h) => {
                if mo.mirror {
                    rl.side = rl.side.map(other_side);
                }
            }
            Some(_) => return Err(bad(c, format!("railing {id} goes only with its stair or ramp; select that too"))),
            None => rl.path.iter_mut().for_each(|q| *q = f(*q)),
        },
        // Its edges keep their numbers, its gables with them; a mirror image sweeps the other way.
        Element::Roof(r) => {
            r.boundary.iter_mut().chain(r.courtyards.iter_mut().flatten()).chain(r.openings.iter_mut().flatten()).for_each(|q| *q = f(*q));
            if let Some(x) = &mut r.extrusion {
                x.line = x.line.map(&f);
                if mo.mirror {
                    (x.from, x.to) = (-x.to, -x.from);
                }
            }
        }
        // A mirror image runs back the other way, so its exterior side is the mirror image of the old one.
        Element::Wall(w) => (w.start, w.end) = if mo.mirror { (f(w.end), f(w.start)) } else { (f(w.start), f(w.end)) },
        Element::Grid(g) => (g.start, g.end) = (f(g.start), f(g.end)),
        Element::Floor(fl) => {
            fl.boundary.iter_mut().chain(fl.openings.iter_mut().flatten()).for_each(|q| *q = f(*q));
            // A mirror turns the outline clockwise; floors keep theirs counter-clockwise.
            if mo.mirror {
                fl.boundary.reverse();
            }
            fl.arrow = fl.arrow.map(|a| a.map(f));
            (fl.pattern_origin, fl.pattern_angle) = turned(f, fl.pattern_origin, fl.pattern_angle);
        }
        Element::Ceiling(cl) => {
            cl.boundary.iter_mut().chain(cl.openings.iter_mut().flatten()).for_each(|q| *q = f(*q));
            if mo.mirror {
                cl.boundary.reverse();
            }
        }
        Element::Shaft(Shaft { boundary, .. }) => {
            boundary.iter_mut().for_each(|q| *q = f(*q));
            if mo.mirror {
                boundary.reverse();
            }
        }
        // Its top outline goes the same way, twin for twin.
        Element::Mass(Mass { boundary, top, .. }) => {
            for ring in std::iter::once(boundary).chain(top.as_mut()) {
                ring.iter_mut().for_each(|q| *q = f(*q));
                if mo.mirror {
                    ring.reverse();
                }
            }
        }
        Element::Room(r) => r.point = f(r.point),
        Element::Door(o) | Element::Window(o) if !within(o.host) => match (mo.slide, m.wall(o.host)) {
            (Some(by), Some(w)) => o.offset += by.dot((w.end - w.start).unit()),
            _ => return Err(bad(c, format!("{category} {id} turns and mirrors only with its wall; select the wall too"))),
        },
        Element::Door(_) | Element::Window(_) => {}
        Element::Dimension(d) => d.at = f(d.at),
        // A tag's place hangs on its door or window's: see `relocate` and `duplicate`.
        Element::Tag(_) => {}
        Element::Section(v) | Element::Elevation(v) => {
            (v.start, v.end) = (f(v.start), f(v.end));
            // A mirror image of it looks the mirrored way round: to the left of its line again.
            if mo.mirror {
                std::mem::swap(&mut v.start, &mut v.end);
            }
        }
    }
    Ok(e)
}

fn other_side(s: Side) -> Side {
    match s {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    }
}

/// Where `f` takes a point and the way `angle` degrees points from it.
fn turned(f: &dyn Fn(P2) -> P2, at: P2, angle: f64) -> (P2, f64) {
    let (a, b) = (f(at), f(at + heading(angle) * 1000.0));
    (a, degrees((b.1 - a.1).atan2(b.0 - a.0).to_degrees()))
}

/// Where `mo` takes the heads of the tags among `ids`.
fn tag_heads(m: &Model, ids: &[Id], mo: &Motion) -> Vec<(Id, P2)> {
    ids.iter()
        .filter_map(|id| match m.elements.get(id) {
            Some(Element::Tag(t)) => Some((*id, (mo.map)(m.anchor(t.host)? + t.offset))),
            _ => None,
        })
        .collect()
}

/// Tag `id`, its head put at `head` whatever its door or window did.
fn place_tag(m: &mut Model, id: Id, head: P2) {
    let Some(Element::Tag(t)) = m.elements.get(id) else { return };
    let Some(a) = m.anchor(t.host) else { return };
    if let Some(Element::Tag(t)) = m.elements.get_mut(id) {
        t.offset = micron(head - a);
    }
}

/// Take elements by `mo`, keeping walls joined: walls joined to a taken wall stretch to stay
/// joined, and doors and windows in walls that changed keep their place or go with their wall.
/// With `end` (`Some(true)` the start, `Some(false)` the end), only that end of one wall goes.
fn relocate(c: &str, s: &mut Session, ids: &[Id], mo: &Motion, end: Option<bool>) -> Result<(), String> {
    let m = &s.model;
    let f = mo.map;
    let moving: BTreeSet<Id> = ids.iter().copied().collect();
    let moving_guids: BTreeSet<Guid> = ids.iter().map(|id| m.guid(*id)).collect();
    let heads = tag_heads(m, ids, mo);
    // Doors and windows of walls taken whole ride along.
    let carried = |host: Guid| moving_guids.contains(&host) && end.is_none();
    let mut walls: BTreeMap<Id, Wall> = BTreeMap::new();
    let mut others: Vec<(Id, Element)> = Vec::new();
    for &id in ids {
        match transformed(c, m, id, mo, &moving)? {
            Element::Wall(mut w) => {
                match (end, m.wall(id)) {
                    (Some(true), Some(old)) => w.end = old.end,
                    (Some(false), Some(old)) => w.start = old.start,
                    _ => {}
                }
                walls.insert(id, w);
            }
            Element::Door(o) | Element::Window(o) if moving_guids.contains(&o.host) => {}
            e => others.push((id, e)),
        }
    }
    let old = |id: &Id| m.wall(*id);
    // A taken wall's end that was on a wall staying put slides back onto it, along the taken wall.
    // (A wall's end taken on its own goes where it's told.)
    for (id, w) in walls.iter_mut().filter(|_| end.is_none()) {
        let Some(before) = old(id).filter(|b| b.arc == 0.0) else { continue };
        let line = (w.start, w.end);
        for (end, was) in [(&mut w.start, before.start), (&mut w.end, before.end)] {
            let host = m.elements.iter().find_map(|(other, e)| match e {
                Element::Wall(o)
                    if !moving.contains(other)
                        && o.arc == 0.0
                        && stacked(m, o, before)
                        && geom::dist_to_segment(was, o.start, o.end) <= TOL =>
                {
                    Some((o.start, o.end))
                }
                _ => None,
            });
            if let Some((a, b)) = host {
                *end = geom::line_intersection(line.0, line.1, a, b).map_or(*end, micron);
            }
        }
    }
    // Walls staying put stretch to keep their joins with the taken ones: an end on a taken wall's
    // end goes with it; an end on a taken wall's span slides along its own wall onto it.
    let moved: Vec<(Wall, Wall)> = walls.iter().filter_map(|(id, w)| Some((old(id)?.clone(), w.clone()))).collect();
    for (&id, e) in &m.elements {
        let Element::Wall(w) = e else { continue };
        if moving.contains(&id) {
            continue;
        }
        let mut n = w.clone();
        for (end, at) in [(&mut n.start, w.start), (&mut n.end, w.end)] {
            for (before, after) in moved.iter().filter(|(before, _)| stacked(m, w, before)) {
                if at.dist(before.start) <= TOL || at.dist(before.end) <= TOL {
                    *end = if at.dist(before.start) <= TOL { after.start } else { after.end };
                    break;
                }
                if Axis::of(before).distance(at, 0.0) <= TOL {
                    let straight = w.arc == 0.0 && after.arc == 0.0;
                    *end =
                        geom::line_intersection(w.start, w.end, after.start, after.end).filter(|_| straight).map_or_else(|| f(at), micron);
                    break;
                }
            }
        }
        if n != *w {
            walls.insert(id, n);
        }
    }
    // Doors and windows in walls that changed keep their place, or go with their wall.
    for (&id, e) in &m.elements {
        let (Element::Door(o) | Element::Window(o)) = e else { continue };
        let Some(host) = m.elements.id(o.host) else { continue };
        let (Some(before), Some(after)) = (old(&host), walls.get(&host)) else { continue };
        let along = carried(o.host) || moving.contains(&id);
        let spot = Axis::of(before).at(o.offset, 0.0);
        let at = if along { f(spot) } else { spot };
        let mut o = o.clone();
        let (axis, straight) = (Axis::of(after), after.arc == 0.0);
        o.offset = if straight { (at - after.start).dot(axis.tangent(0.0)) } else { axis.project(at).0 };
        // Its mirror image runs back the other way: the door hangs on the other jamb, toward the same side.
        if mo.mirror && carried(o.host) {
            o.flip_hand = !o.flip_hand;
        }
        others.retain(|(other, _)| *other != id);
        others.push((id, if matches!(e, Element::Door(_)) { Element::Door(o) } else { Element::Window(o) }));
    }
    let m = s.model_mut();
    let mut flipped = Vec::new();
    for (id, w) in walls {
        m.elements.insert(id, Element::Wall(w));
    }
    for (id, e) in others {
        if mo.mirror && matches!(e, Element::Mass(_)) {
            flipped.push(id);
        }
        m.elements.insert(id, e);
    }
    for id in flipped {
        massing::mirrored(m, id);
    }
    // Dimensions keep to the same lines of mirrored walls, which run the other way: their starts
    // and ends swap, and their doors' and windows' jambs.
    if mo.mirror && end.is_none() {
        let hosts: BTreeMap<Guid, Guid> = m
            .elements
            .with_guids()
            .filter_map(|(_, id, e)| match e {
                Element::Door(o) | Element::Window(o) => Some((id, o.host)),
                _ => None,
            })
            .collect();
        let turned = |id: &Guid| moving_guids.contains(id) || hosts.get(id).is_some_and(|h| moving_guids.contains(h));
        for e in m.elements.values_mut() {
            if let Element::Dimension(d) = e {
                d.refs.iter_mut().filter(|r| turned(&r.id)).for_each(|r| r.face = r.face.mirrored());
            }
        }
    }
    for (id, head) in heads {
        place_tag(m, id, head);
    }
    Ok(())
}

/// Copies of elements taken by `mo`, with the doors and windows of copied walls; the copies' ids.
/// Copied dimensions and tags go onto the copies of what they measure and name, where there are.
fn duplicate(c: &str, s: &mut Session, ids: &[Id], mo: &Motion) -> R {
    let m = &s.model;
    let copying: BTreeSet<Id> = ids.iter().copied().collect();
    let copying_guids: BTreeSet<Guid> = ids.iter().map(|id| m.guid(*id)).collect();
    let heads: BTreeMap<Id, P2> = tag_heads(m, ids, mo).into_iter().collect();
    let mut copies = Vec::new();
    // Dimensions and tags, copied last: they may refer to copies.
    let mut notes = Vec::new();
    for &id in ids {
        match transformed(c, m, id, mo, &copying)? {
            // A copied wall brings its doors and windows, a stair or a ramp its railings; see below.
            Element::Door(o) | Element::Window(o) if copying_guids.contains(&o.host) => {}
            Element::Railing(Railing { host: Some(h), .. }) if copying_guids.contains(&h) => {}
            e @ (Element::Dimension(_) | Element::Tag(_) | Element::Spot(_)) => notes.push((id, e)),
            e => copies.push((id, e)),
        }
    }
    let hosted: Vec<(Id, Element)> = m
        .elements
        .iter()
        .filter(|(_, e)| match e {
            Element::Door(o) | Element::Window(o) => copying_guids.contains(&o.host),
            Element::Railing(r) => r.host.is_some_and(|h| copying_guids.contains(&h)),
            _ => false,
        })
        .map(|(&id, e)| {
            let mut e = e.clone();
            match &mut e {
                // On a mirrored copy, which runs back the other way: from its other end, on the other jamb.
                Element::Door(o) | Element::Window(o) if mo.mirror => {
                    o.offset = m.wall(o.host).map_or(o.offset, |w| Axis::of(w).length() - o.offset);
                    o.flip_hand = !o.flip_hand;
                }
                Element::Railing(r) if mo.mirror => r.side = r.side.map(other_side),
                _ => {}
            }
            (id, e)
        })
        .collect();
    let m = s.model_mut();
    let mut made = Vec::new();
    // Each copied element's copy.
    let mut hosts = BTreeMap::new();
    for (from, mut e) in copies.into_iter().chain(hosted) {
        let door = matches!(e, Element::Door(_));
        match &mut e {
            Element::Grid(g) => g.name = next_grid_name(m),
            Element::Section(v) => v.name = m.free_view_name(m.language.word("Section"), true),
            Element::Elevation(v) => v.name = m.free_view_name(m.language.word("Elevation"), true),
            Element::Room(r) => {
                r.number = next_number(m.elements.values().filter_map(|e| match e {
                    Element::Room(r) => Some(r.number.as_str()),
                    _ => None,
                }))
            }
            Element::Railing(r) => r.host = r.host.map(|h| hosts.get(&h).copied().unwrap_or(h)),
            Element::Door(o) | Element::Window(o) => {
                o.host = hosts.get(&o.host).copied().unwrap_or(o.host);
                o.mark = next_number(m.elements.values().filter_map(|e| match e {
                    Element::Door(o) if door => Some(o.mark.as_str()),
                    Element::Window(o) if !door => Some(o.mark.as_str()),
                    _ => None,
                }));
            }
            _ => {}
        }
        // A copy is no mass's floor or face.
        if let Some(params) = e.params_mut() {
            massing::detach(params);
        }
        let id = m.insert(e);
        hosts.insert(m.guid(from), m.guid(id));
        made.push(id);
    }
    // Copied walls attached to roofs or floors copied with them attach to the copies.
    for id in &made {
        if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
            for at in [&mut w.attach_top, &mut w.attach_base] {
                *at = at.map(|a| hosts.get(&a).copied().unwrap_or(a));
            }
        }
    }
    for (from, mut e) in notes {
        match &mut e {
            Element::Dimension(d) => {
                for r in &mut d.refs {
                    if let Some(&to) = hosts.get(&r.id) {
                        r.id = to;
                        if mo.mirror {
                            r.face = r.face.mirrored();
                        }
                    }
                }
            }
            Element::Tag(t) => t.host = hosts.get(&t.host).copied().unwrap_or(t.host),
            Element::Spot(sp) => sp.host = sp.host.map(|h| hosts.get(&h).copied().unwrap_or(h)),
            _ => {}
        }
        let id = m.insert(e);
        if let Some(&head) = heads.get(&from) {
            place_tag(m, id, head);
        }
        made.push(id);
    }
    Ok(json!({"ids": made}))
}

fn move_elements(s: &mut Session, p: &Value) -> R {
    let c = "move";
    let ids = ids_param(c, &s.model, p)?;
    let by = point_req(c, p, "by")?;
    // Moving one end: `Some(true)` the start, `Some(false)` the end.
    let end = match string(c, p, "end")?.as_deref() {
        None => None,
        Some("start") => Some(true),
        Some("end") => Some(false),
        Some(other) => return Err(bad(c, format!("`end` is `start` or `end`, not `{other}`"))),
    };
    if end.is_some() && !matches!(ids.as_slice(), [one] if s.model.wall(*one).is_some()) {
        return Err(bad(c, "`end` moves an end of one wall; give `ids` one wall"));
    }
    relocate(c, s, &ids, &Motion { map: &|q| q + by, slide: Some(by), mirror: false }, end)?;
    Ok(json!({"moved": ids}))
}

fn copy(s: &mut Session, p: &Value) -> R {
    let c = "copy";
    let ids = ids_param(c, &s.model, p)?;
    let by = point_req(c, p, "by")?;
    duplicate(c, s, &ids, &Motion { map: &|q| q + by, slide: Some(by), mirror: false })
}

fn rotate(s: &mut Session, p: &Value) -> R {
    let c = "rotate";
    let ids = ids_param(c, &s.model, p)?;
    let about = point_req(c, p, "about")?;
    let angle = num(c, p, "angle")?.ok_or_else(|| bad(c, "`angle` in degrees, counter-clockwise, is required"))?;
    let (sin, cos) = angle.to_radians().sin_cos();
    let turn = |q: P2| {
        let d = q - about;
        micron(about + P2(d.0 * cos - d.1 * sin, d.0 * sin + d.1 * cos))
    };
    let mo = Motion { map: &turn, slide: None, mirror: false };
    if flag(c, p, "copy")? {
        return duplicate(c, s, &ids, &mo);
    }
    relocate(c, s, &ids, &mo, None)?;
    Ok(json!({"rotated": ids}))
}

fn mirror(s: &mut Session, p: &Value) -> R {
    let c = "mirror";
    let ids = ids_param(c, &s.model, p)?;
    let axis = points(c, p, "axis")?;
    let (a, b) = match axis.as_slice() {
        [a, b] if a.dist(*b) > TOL => (*a, *b),
        _ => return Err(bad(c, "`axis` is two different points [[x, y], [x, y]]")),
    };
    let u = (b - a).unit();
    let reflect = |q: P2| {
        let d = q - a;
        micron(a + u * (2.0 * d.dot(u)) - d)
    };
    let mo = Motion { map: &reflect, slide: None, mirror: true };
    if flag(c, p, "copy")? {
        return duplicate(c, s, &ids, &mo);
    }
    relocate(c, s, &ids, &mo, None)?;
    Ok(json!({"mirrored": ids}))
}

fn corner(s: &mut Session, p: &Value) -> R {
    let c = "corner";
    let ids = ids_param(c, &s.model, p)?;
    let m = &s.model;
    let [a, b] = ids.as_slice() else { return Err(bad(c, "`ids` takes two walls")) };
    if let (Some(Element::DetailLine(la)), Some(Element::DetailLine(lb))) = (m.elements.get(*a), m.elements.get(*b)) {
        if la.view != lb.view {
            return Err(bad(c, format!("detail lines {a} and {b} are in different views; trim lines of one view")));
        }
        let keep = if absent(p.get("keep")) { vec![la.curve.middle(), lb.curve.middle()] } else { points(c, p, "keep")? };
        if keep.len() != 2 {
            return Err(bad(c, "`keep` is a point on each line, [[x, y], [x, y]]"));
        }
        let (ca, cb) = crate::draw::corner(&la.curve, keep[0], &lb.curve, keep[1]).ok_or_else(|| {
            bad(c, "the lines don't meet, run on as far as need be: they're parallel, or an arc's circle misses the other")
        })?;
        let (ia, ib) = (*a, *b);
        let (la, lb) = (la.clone(), lb.clone());
        let m = s.model_mut();
        m.elements.insert(ia, Element::DetailLine(DetailLine { curve: ca.map(micron), ..la }));
        m.elements.insert(ib, Element::DetailLine(DetailLine { curve: cb.map(micron), ..lb }));
        return Ok(json!({"ids": [ia, ib]}));
    }
    let (Some(wa), Some(wb)) = (m.wall(*a), m.wall(*b)) else { return Err(bad(c, "`ids` takes two walls")) };
    if wa.arc != 0.0 || wb.arc != 0.0 {
        return Err(bad(c, "it trims and extends straight walls; a curved one changes by its ends or its `arc`"));
    }
    let keep = if absent(p.get("keep")) { Vec::new() } else { points(c, p, "keep")? };
    if !keep.is_empty() && keep.len() != 2 {
        return Err(bad(c, "`keep` is a point on each wall, [[x, y], [x, y]]"));
    }
    let hit = micron(
        geom::line_intersection(wa.start, wa.end, wb.start, wb.end).ok_or_else(|| bad(c, "the walls are parallel: they never meet"))?,
    );
    for (i, &id) in [*a, *b].iter().enumerate() {
        let Some(w) = s.model.wall(id).cloned() else { continue };
        let u = (w.end - w.start).unit();
        let at = |q: P2| (q - w.start).dot(u);
        // The end that goes to the corner: away from the part kept, else the one nearer to it.
        let start = match keep.get(i) {
            Some(k) => at(*k) > at(hit),
            None => hit.dist(w.start) < hit.dist(w.end),
        };
        let by = hit - if start { w.start } else { w.end };
        if by.length() > 1e-6 {
            relocate(c, s, &[id], &Motion { map: &|q| q + by, slide: Some(by), mirror: false }, Some(start))?;
        }
    }
    Ok(json!({"corner": [hit.0, hit.1]}))
}

fn split(s: &mut Session, p: &Value) -> R {
    let c = "split";
    let m = &s.model;
    let id = id_opt(c, p, "id")?.ok_or_else(|| bad(c, "`id`, a wall or a detail line, is required"))?;
    if let Some(lid) = m.elements.id(id)
        && let Some(Element::DetailLine(l)) = m.elements.get(lid)
    {
        let (l, at) = (l.clone(), point_req(c, p, "at")?);
        return split_line(s, lid, l, at);
    }
    let w = m.wall(id).cloned().ok_or_else(|| bad(c, format!("{id} is not a wall")))?;
    let id = m.elements.id(id).unwrap_or_default();
    let at = point_req(c, p, "at")?;
    if off_centre(m, &w, at) > m.thickness(&w) / 2.0 + 100.0 {
        return Err(bad(c, format!("[{}, {}] isn't on wall {id}", at.0, at.1)));
    }
    let axis = Axis::of(&w);
    let len = axis.length();
    // Along a straight wall, to the micron; an arc keeps its exact share.
    let along = if w.arc == 0.0 { (axis.project(at).0 * 1e3).round() / 1e3 } else { axis.project(at).0 };
    if along <= TOL || along >= len - TOL {
        return Err(bad(c, "split between the wall's ends"));
    }
    let cut = micron(axis.at(along, 0.0));
    // Doors and windows go with the part they're in; one across the cut is in the way.
    let mut later = Vec::new();
    for (&oid, e) in &m.elements {
        let (Element::Door(o) | Element::Window(o)) = e else { continue };
        if !m.same(o.host, id) {
            continue;
        }
        let half = m.opening_type(e).map_or(0.0, |t| t.width / 2.0);
        if o.offset + half <= along + TOL {
            continue;
        }
        if o.offset - half < along - TOL {
            return Err(bad(c, format!("{} {oid} is in the way; split elsewhere", e.category())));
        }
        later.push(oid);
    }
    let (mut first, mut second) = (w.clone(), w);
    (first.end, second.start) = (cut, cut);
    // Neither piece is the mass's face now.
    massing::detach(&mut first.params);
    massing::detach(&mut second.params);
    // An arc's turn shared out as its length is.
    first.arc = degrees(first.arc * along / len);
    second.arc = degrees(second.arc - first.arc);
    let m = s.model_mut();
    m.elements.insert(id, Element::Wall(first));
    let new = m.insert(Element::Wall(second));
    let host = m.guid(new);
    for oid in later {
        if let Some(Element::Door(o) | Element::Window(o)) = m.elements.get_mut(oid) {
            o.host = host;
            o.offset -= along;
        }
    }
    Ok(json!({"ids": [id, new]}))
}

/// Detail line `id` split where it passes nearest `at`, or a closed one opened there.
fn split_line(s: &mut Session, id: Id, l: DetailLine, at: P2) -> R {
    let c = "split";
    let (t, off) = l.curve.nearest(at);
    if off > 100.0 {
        return Err(bad(c, format!("[{}, {}] isn't on detail line {id}: it passes {} mm from it", at.0, at.1, hundredth(off))));
    }
    let (first, second) = l.curve.split(t).ok_or_else(|| bad(c, "split between the line's ends"))?;
    let m = s.model_mut();
    m.elements.insert(id, Element::DetailLine(DetailLine { curve: first, ..l.clone() }));
    match second {
        Some(curve) => {
            let new = m.insert(Element::DetailLine(DetailLine { curve, ..l }));
            Ok(json!({"ids": [id, new]}))
        }
        None => Ok(json!({"ids": [id], "opened": true})),
    }
}

fn join_polyline(s: &mut Session, p: &Value) -> R {
    let c = "join_polyline";
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    if ids.len() < 2 {
        return Err(bad(c, "give two detail lines or more to join"));
    }
    let mut lines: Vec<DetailLine> = Vec::new();
    for &id in &ids {
        match m.elements.get(id) {
            Some(Element::DetailLine(l)) => lines.push(l.clone()),
            _ => return Err(bad(c, format!("{id} is not a detail line"))),
        }
    }
    let Some(first) = lines.first().cloned() else { return Err(bad(c, "give two detail lines or more to join")) };
    if let Some((i, _)) = lines.iter().enumerate().find(|(_, l)| l.view != first.view) {
        return Err(bad(c, format!("detail line {} is in another view than {}; join lines of one view", ids[i], ids[0])));
    }
    if let Some((i, l)) = lines.iter().enumerate().find(|(_, l)| !l.curve.is_poly() || l.curve.is_closed()) {
        return Err(bad(c, format!("detail line {} is a {}; only open lines, arcs and polylines join", ids[i], l.curve.kind())));
    }
    let curves: Vec<crate::curves::Curve> = lines.iter().map(|l| l.curve.clone()).collect();
    let curve =
        crate::curves::join(&curves).ok_or_else(|| bad(c, "the lines don't make one chain, each end meeting the next within 1 mm"))?;
    // The lines joined in go as Delete takes them: a dimension that measured one loses it (and goes, left with fewer than two).
    let joined_in: Vec<Guid> = ids[1..].iter().map(|id| m.guid(*id)).collect();
    let measuring: Vec<Id> = m
        .elements
        .iter()
        .filter(|(_, e)| matches!(e, Element::Dimension(d) if d.refs.iter().any(|r| joined_in.contains(&r.id))))
        .map(|(id, _)| *id)
        .collect();
    delete(s, &json!({"ids": &ids[1..]}))?;
    s.model_mut().elements.insert(ids[0], Element::DetailLine(DetailLine { curve, ..first }));
    let notes: Vec<String> = measuring
        .into_iter()
        .map(|d| match s.model.elements.get(d) {
            Some(_) => format!("dimension {d} measured a line that is joined in: it lost that reference"),
            None => format!("dimension {d} measured a line that is joined in, was left with fewer than two references, and is deleted"),
        })
        .collect();
    Ok(if notes.is_empty() { json!({"id": ids[0]}) } else { json!({"id": ids[0], "notes": notes}) })
}

fn offset(s: &mut Session, p: &Value) -> R {
    let c = "offset";
    let m = &s.model;
    let id = id_opt(c, p, "id")?.and_then(|g| m.elements.id(g)).ok_or_else(|| bad(c, "`id`, a detail line, is required"))?;
    let Some(Element::DetailLine(l)) = m.elements.get(id).cloned() else { return Err(bad(c, format!("{id} is not a detail line"))) };
    let d = num(c, p, "distance")?.ok_or_else(|| bad(c, "`distance`, mm, is required"))?;
    if !(d.is_finite() && d > 0.0 && d <= MAX_COORD) {
        return Err(bad(c, format!("`distance` is more than 0 mm, not {d}")));
    }
    let toward = point_req(c, p, "toward")?;
    let curve = crate::draw::pick(&l.curve, toward, d)
        .ok_or_else(|| bad(c, format!("a {} can't be offset by {d} mm (it would turn inside out, or fold over itself)", l.curve.kind())))?
        .map(micron);
    let copy = absent(p.get("copy")) || flag(c, p, "copy")?;
    let m = s.model_mut();
    let out = DetailLine { curve, ..l };
    if copy {
        let new = m.insert(Element::DetailLine(out));
        Ok(json!({"id": new}))
    } else {
        m.elements.insert(id, Element::DetailLine(out));
        Ok(json!({"id": id}))
    }
}

/// Whether `boundary` is drawn where detail line `l` is: another detail line of its view, or a plan element on the level
/// of the plan it is in (a grid, which has no level, in any plan). Its coordinates are the view's: a section's along and
/// up, a sheet's paper mm, not the plan's.
fn drawn_in_view(m: &Model, l: &DetailLine, boundary: Id) -> bool {
    match m.elements.get(boundary) {
        Some(Element::DetailLine(b)) => b.view == l.view,
        Some(e) => m.plan_level(l.view).is_some_and(|plan| m.level_of(e).is_none_or(|level| m.guid(plan) == level)),
        None => false,
    }
}

fn trim_extend(s: &mut Session, p: &Value) -> R {
    let c = "trim_extend";
    let m = &s.model;
    let boundary = id_opt(c, p, "boundary")?
        .and_then(|g| m.elements.id(g))
        .ok_or_else(|| bad(c, "`boundary`, a line to trim or extend to, is required"))?;
    let edge = crate::draw::element_curve(m, boundary).ok_or_else(|| bad(c, format!("{boundary} has no line to trim or extend to")))?;
    let ids = ids_param(c, m, p)?;
    let keep = points(c, p, "keep")?;
    if keep.len() != ids.len() {
        return Err(bad(c, format!("`keep` gives a point on each line to keep: {} for {} lines", keep.len(), ids.len())));
    }
    let mut done = Vec::new();
    for (&id, &k) in ids.iter().zip(&keep) {
        let Some(Element::DetailLine(l)) = m.elements.get(id) else { return Err(bad(c, format!("{id} is not a detail line"))) };
        if !drawn_in_view(m, l, boundary) {
            return Err(bad(
                c,
                format!("detail line {id} is in another view than {boundary} is drawn in; trim it to a line of its own view"),
            ));
        }
        let x = crate::draw::meet(&l.curve, &edge, k).ok_or_else(|| bad(c, format!("detail line {id} never meets {boundary}")))?;
        let curve = crate::draw::trim_to(&l.curve, k, x)
            .ok_or_else(|| bad(c, format!("detail line {id} can't be cut there: nothing would be left")))?;
        done.push((id, DetailLine { curve: curve.map(micron), ..l.clone() }));
    }
    let m = s.model_mut();
    for (id, l) in done {
        m.elements.insert(id, Element::DetailLine(l));
    }
    Ok(json!({"ids": ids}))
}

/// The doors and windows of straight wall `id` as they are once it is `before` becomes `after` (cut back at an end): each
/// where it was in the plan, so its offset along the wall moves by what the start moved. An error if one would be cut off.
fn keep_openings(c: &str, m: &Model, id: Id, before: &Wall, after: &Wall) -> Result<Vec<(Id, Element)>, String> {
    let (was, now) = (Axis::of(before), Axis::of(after));
    let mut out = Vec::new();
    for (&oid, e) in &m.elements {
        let (Element::Door(o) | Element::Window(o)) = e else { continue };
        if !m.same(o.host, id) {
            continue;
        }
        let half = m.opening_type(e).map_or(0.0, |t| t.width / 2.0);
        let along = ((was.at(o.offset, 0.0) - after.start).dot(now.tangent(0.0)) * 1e3).round() / 1e3;
        if along - half < -TOL || along + half > now.length() + TOL {
            return Err(bad(
                c,
                format!("{} {oid} is in the part of wall {id} the fillet cuts away; move it along the wall first", e.category()),
            ));
        }
        let o = Opening { offset: along, ..o.clone() };
        out.push((oid, if matches!(e, Element::Door(_)) { Element::Door(o) } else { Element::Window(o) }));
    }
    Ok(out)
}

fn fillet(s: &mut Session, p: &Value) -> R {
    let c = "fillet";
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let &[a, b] = ids.as_slice() else { return Err(bad(c, "`ids` is two detail lines")) };
    // Two straight walls: cut back, and an arc wall like the first between them.
    if let (Some(wa), Some(wb)) = (m.wall(a).cloned(), m.wall(b).cloned()) {
        let radius = num(c, p, "radius")?.ok_or_else(|| bad(c, "`radius`, mm, is required"))?;
        if !(radius.is_finite() && radius > TOL && radius <= MAX_COORD) {
            return Err(bad(c, format!("`radius` is more than {TOL} mm, not {radius}")));
        }
        if wa.arc != 0.0 || wb.arc != 0.0 {
            return Err(bad(c, "it rounds two straight walls; a curved one changes by its ends or its `arc`"));
        }
        let (ca, arc, cb) =
            crate::draw::fillet_lines(&crate::curves::Curve::line(wa.start, wa.end), &crate::curves::Curve::line(wb.start, wb.end), radius)
                .ok_or_else(|| {
                    bad(c, "the walls are straight, meet at an angle and are long enough for the arc; these aren't, or it doesn't fit")
                })?;
        let sweep = match &arc {
            crate::curves::Curve::Poly { arcs, .. } => arcs.first().copied().unwrap_or(0.0),
            _ => 0.0,
        };
        let (pa, pb) = (ca.vertices(), cb.vertices());
        let (&[a0, a1], &[b0, b1]) = (pa.as_slice(), pb.as_slice()) else { return Err(bad(c, "the walls can't be rounded")) };
        let mut round = Wall { start: micron(arc.start()), end: micron(arc.end()), arc: degrees(sweep), ..wa.clone() };
        massing::detach(&mut round.params);
        let (cut_a, cut_b) =
            (Wall { start: micron(a0), end: micron(a1), ..wa.clone() }, Wall { start: micron(b0), end: micron(b1), ..wb.clone() });
        // Doors and windows stay where they are in the plan, as `corner` leaves them, or the fillet is refused if it cuts one off.
        let mut hosted = keep_openings(c, m, a, &wa, &cut_a)?;
        hosted.extend(keep_openings(c, m, b, &wb, &cut_b)?);
        let m = s.model_mut();
        m.elements.insert(a, Element::Wall(cut_a));
        m.elements.insert(b, Element::Wall(cut_b));
        for (id, e) in hosted {
            m.elements.insert(id, e);
        }
        let id = m.insert(Element::Wall(round));
        return Ok(json!({"id": id, "ids": [a, b]}));
    }
    let line = |id: Id| match m.elements.get(id) {
        Some(Element::DetailLine(l)) => Ok(l.clone()),
        _ => Err(bad(c, format!("{id} is not a detail line"))),
    };
    let (la, lb) = (line(a)?, line(b)?);
    if la.view != lb.view {
        return Err(bad(c, format!("detail lines {a} and {b} are in different views")));
    }
    let radius = num(c, p, "radius")?.ok_or_else(|| bad(c, "`radius`, mm, is required"))?;
    if !(radius.is_finite() && radius > TOL && radius <= MAX_COORD) {
        return Err(bad(c, format!("`radius` is more than {TOL} mm, not {radius}")));
    }
    // Two straight lines, not a circle, a polyline or a spline (Revit rounds the corner of two lines).
    for (id, l) in [(a, &la), (b, &lb)] {
        if l.curve.kind() != "line" {
            return Err(bad(c, format!("detail line {id} is a {}: Fillet Arc rounds two straight lines", l.curve.kind())));
        }
    }
    let (ca, arc, cb) = crate::draw::fillet_lines(&la.curve, &lb.curve, radius).ok_or_else(|| {
        bad(c, "the lines meet at an angle and are long enough for the arc; these are parallel, or too short for it, or it doesn't fit")
    })?;
    let (ca, arc, cb) = (ca.map(micron), arc.map(micron), cb.map(micron));
    let m = s.model_mut();
    m.elements.insert(a, Element::DetailLine(DetailLine { curve: ca, ..la.clone() }));
    m.elements.insert(b, Element::DetailLine(DetailLine { curve: cb, ..lb }));
    let id = m.insert(Element::DetailLine(DetailLine { curve: arc, ..la }));
    Ok(json!({"id": id, "ids": [a, b]}))
}

/// Detail line `id`, as `p` names it, for a command `c` that changes its curve.
fn line_of(c: &str, m: &Model, p: &Value) -> Result<(Id, DetailLine), String> {
    let id = id_opt(c, p, "id")?.and_then(|g| m.elements.id(g)).ok_or_else(|| bad(c, "`id`, a detail line, is required"))?;
    match m.elements.get(id) {
        Some(Element::DetailLine(l)) => Ok((id, l.clone())),
        _ => Err(bad(c, format!("{id} is not a detail line"))),
    }
}

fn vertex(s: &mut Session, p: &Value) -> R {
    let c = "vertex";
    let (id, l) = line_of(c, &s.model, p)?;
    let at = point_req(c, p, "at")?;
    let delete = flag(c, p, "delete")?;
    // Near the line, as Split asks, or the vertex nearest: not a point across the drawing.
    let off = if delete { l.curve.vertices().iter().map(|v| v.dist(at)).fold(f64::INFINITY, f64::min) } else { l.curve.nearest(at).1 };
    if off > 100.0 && off.is_finite() {
        let on = if delete { "a vertex of detail line" } else { "detail line" };
        return Err(bad(c, format!("[{}, {}] isn't on {on} {id}: it is {} mm from it", at.0, at.1, hundredth(off))));
    }
    let curve = if delete { l.curve.without_vertex(at) } else { l.curve.with_vertex(at) }.ok_or_else(|| {
        bad(
            c,
            match delete {
                true => format!("detail line {id} is a {}: it has no vertex to spare", l.curve.kind()),
                false => format!("detail line {id} is a {}: only polylines and splines take a vertex, between their ends", l.curve.kind()),
            },
        )
    })?;
    s.model_mut().elements.insert(id, Element::DetailLine(DetailLine { curve: rounded(curve), ..l }));
    Ok(json!({"id": id}))
}

fn convert_spline(s: &mut Session, p: &Value) -> R {
    let c = "convert_spline";
    let (id, l) = line_of(c, &s.model, p)?;
    let curve = l.curve.converted().ok_or_else(|| bad(c, format!("detail line {id} is a {}, not a spline", l.curve.kind())))?;
    let kind = curve.kind();
    let curve = rounded(curve);
    s.model_mut().elements.insert(id, Element::DetailLine(DetailLine { curve, ..l }));
    Ok(json!({"id": id, "shape": kind}))
}

fn explode(s: &mut Session, p: &Value) -> R {
    let c = "explode";
    let m = &s.model;
    let id = id_opt(c, p, "id")?.ok_or_else(|| bad(c, "`id`, a detail line, is required"))?;
    let lid = m.elements.id(id).ok_or_else(|| bad(c, "there's no such element"))?;
    let Some(Element::DetailLine(l)) = m.elements.get(lid).cloned() else { return Err(bad(c, format!("{lid} is not a detail line"))) };
    let parts = crate::curves::explode(&l.curve);
    if parts.len() < 2 {
        return Err(bad(c, format!("detail line {lid} is a {}, already one piece", l.curve.kind())));
    }
    let m = s.model_mut();
    let mut ids = vec![lid];
    for (i, curve) in parts.into_iter().enumerate() {
        let part = Element::DetailLine(DetailLine { curve, ..l.clone() });
        if i == 0 {
            m.elements.insert(lid, part);
        } else {
            ids.push(m.insert(part));
        }
    }
    Ok(json!({"ids": ids}))
}

/// How an entry reads in a message: `wall type`, `family`, `family type`; and the library entry it is.
fn entry_of(c: &str, m: &Model, p: &Value) -> Result<(LibKind, String), String> {
    let (cat, family, ty) = (string(c, p, "category")?, string(c, p, "family")?, string(c, p, "type")?);
    match (cat, family, ty) {
        (Some(cat), None, Some(ty)) => {
            let cat = category(&cat);
            let names = m.type_names(&cat);
            if !names.contains(&ty.as_str()) {
                return Err(bad(c, format!("there is no {cat} type `{ty}`{}", known_names(&names))));
            }
            Ok((LibKind::Type(cat), ty))
        }
        (None, Some(f), None) => {
            if !m.families.contains_key(&f) {
                let names: Vec<&str> = m.families.keys().map(String::as_str).collect();
                return Err(bad(c, format!("there is no family `{f}`{}", known_names(&names))));
            }
            Ok((LibKind::Family, f))
        }
        (None, Some(f), Some(ty)) => {
            let fam = m.families.get(&f).ok_or_else(|| bad(c, format!("there is no family `{f}`")))?;
            if !fam.types.contains_key(&ty) {
                let names: Vec<&str> = fam.types.keys().map(String::as_str).collect();
                return Err(bad(c, format!("family `{f}` has no type `{ty}`{}", known_names(&names))));
            }
            Ok((LibKind::FamilyType, format!("{f}: {ty}")))
        }
        _ => Err(bad(c, "give `category` and `type`, or `family`, or `family` and `type`")),
    }
}

fn delete_type(s: &mut Session, p: &Value) -> R {
    let c = "delete_type";
    let m = &s.model;
    let (kind, name) = entry_of(c, m, p)?;
    let label = kind.label();
    let used = m.library_uses().get(&(kind.clone(), name.clone())).copied().unwrap_or(0);
    if used > 0 {
        return Err(bad(
            c,
            format!("{label} `{name}` is used {used} time{}: change or delete what uses it first", if used == 1 { "" } else { "s" }),
        ));
    }
    match &kind {
        LibKind::Type(cat) if m.type_names(cat).len() <= 1 => {
            return Err(bad(c, format!("`{name}` is the last {label}: a {cat} needs one; make another first")));
        }
        LibKind::FamilyType if name.split_once(": ").and_then(|(f, _)| m.families.get(f)).is_some_and(|f| f.types.len() <= 1) => {
            return Err(bad(c, format!("`{name}` is its family's last type: delete the family")));
        }
        _ => {}
    }
    s.model_mut().remove_entry(&kind, &name);
    Ok(json!({"deleted": {"kind": label, "name": name}}))
}

/// How many passes Purge Unused makes at most: what a pass takes out may leave more unused (a family's material), a few deep.
const PURGE_PASSES: usize = 16;

fn purge(s: &mut Session, p: &Value) -> R {
    let c = "purge";
    let kinds: Vec<String> = match p.get("kinds") {
        v if absent(v) => vec!["type".into(), "family".into()],
        Some(Value::Array(a)) => a
            .iter()
            .map(|k| {
                k.as_str()
                    .filter(|k| crate::library::PURGE_KINDS.contains(k))
                    .map(String::from)
                    .ok_or_else(|| bad(c, format!("`kinds` holds {}, not {k}", crate::library::PURGE_KINDS.join(", "))))
            })
            .collect::<Result<_, _>>()?,
        Some(v) => return Err(bad(c, format!("`kinds` is a list of what to purge, not {v}"))),
        None => vec!["type".into(), "family".into()],
    };
    let dry_run = flag(c, p, "dry_run")?;
    let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
    // On a copy, pass by pass: the list of a dry run is what the run would take out.
    let mut model = (*s.model).clone();
    let mut removed: Vec<(LibKind, String)> = Vec::new();
    for _ in 0..PURGE_PASSES {
        let pass = model.unused_entries(&kinds);
        if pass.is_empty() {
            break;
        }
        for (kind, name) in pass {
            if model.remove_entry(&kind, &name) {
                removed.push((kind, name));
            }
        }
    }
    if !dry_run && !removed.is_empty() {
        *s.model_mut() = model;
    }
    let listed: Vec<Value> = removed.iter().map(|(k, n)| json!({"kind": k.label(), "name": n})).collect();
    Ok(json!({"removed": listed, "count": listed.len(), "dry_run": dry_run}))
}

fn delete(s: &mut Session, p: &Value) -> R {
    let c = "delete";
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let mut gone: BTreeSet<Guid> = ids.iter().map(|id| m.guid(*id)).collect();
    // What hangs on what goes, goes: a wall's doors and windows, and their tags.
    loop {
        let hosted: Vec<Guid> = m
            .elements
            .with_guids()
            .filter(|(_, id, e)| {
                !gone.contains(id)
                    && match e {
                        Element::Door(o) | Element::Window(o) => gone.contains(&o.host),
                        Element::Railing(r) => r.host.is_some_and(|h| gone.contains(&h)),
                        // A plan view goes with its level and its parent; what is annotated in a view goes with it.
                        Element::PlanView(v) => gone.contains(&v.level) || v.parent.is_some_and(|p| gone.contains(&p)),
                        Element::Tag(t) => gone.contains(&t.host) || t.view.is_some_and(|v| gone.contains(&v)),
                        Element::Dimension(d) => d.view.is_some_and(|v| gone.contains(&v)),
                        Element::Viewport(v) => gone.contains(&v.sheet) || gone.contains(&v.view),
                        // Notes and spot elevations go with their view, spots with what they read.
                        Element::Text(n) => gone.contains(&n.view),
                        Element::DetailLine(l) => gone.contains(&l.view),
                        Element::FilledRegion(r) => gone.contains(&r.view),
                        Element::DetailGroup(g) => gone.contains(&g.view),
                        Element::RepeatingDetail(r) => gone.contains(&r.view),
                        Element::Spot(sp) => gone.contains(&sp.view) || sp.host.is_some_and(|h| gone.contains(&h)),
                        _ => false,
                    }
            })
            .map(|(_, id, _)| id)
            .collect();
        if hosted.is_empty() {
            break;
        }
        gone.extend(hosted);
    }
    // A dimension loses the references that go; left with fewer than two, it goes too.
    let mut kept = Vec::new();
    for (id, guid, e) in m.elements.with_guids() {
        if let Element::Dimension(d) = e
            && !gone.contains(&guid)
            && d.refs.iter().any(|r| gone.contains(&r.id))
        {
            let mut d = d.clone();
            d.refs.retain(|r| !gone.contains(&r.id));
            if d.refs.len() < 2 {
                gone.insert(guid);
            } else {
                kept.push((id, d));
            }
        }
    }
    let deleted: Vec<Id> = m.elements.with_guids().filter(|(_, g, _)| gone.contains(g)).map(|(id, ..)| id).collect();
    let m = s.model_mut();
    for id in &gone {
        m.elements.remove(id);
    }
    // What was made from a mass that goes is ordinary from then on.
    massing::forget(m, &gone);
    // No view keeps an override for what isn't there.
    for g in m.elements.values_mut().filter_map(graphics::graphics_mut) {
        g.elements.retain(|id, _| !gone.contains(id));
        g.linework.retain(|l| !gone.contains(&l.element));
    }
    for (id, d) in kept {
        m.elements.insert(id, Element::Dimension(d));
    }
    // Walls attached to a roof or a floor that goes come loose, and roofs running into one end nowhere.
    for e in m.elements.values_mut() {
        let ties = match e {
            Element::Wall(w) => vec![&mut w.attach_top, &mut w.attach_base],
            Element::Roof(r) => vec![&mut r.join],
            _ => Vec::new(),
        };
        for at in ties {
            if at.is_some_and(|a| gone.contains(&a)) {
                *at = None;
            }
        }
    }
    Ok(json!({"deleted": deleted}))
}

// ---------- reading ----------

fn units(s: &mut Session, p: &Value) -> R {
    let c = "units";
    if let Some(name) = string(c, p, "length")? {
        let u = Units::from_name(&name).ok_or_else(|| bad(c, format!("`length` is mm, cm, m, in, ft or ft-in, not `{name}`")))?;
        if u != s.model.units {
            s.model_mut().units = u;
        }
    }
    Ok(json!({"length": s.model.units.name()}))
}

/// `kind` `from` is called `to` now: its library entry, and everything that names it.
fn rename_entry(m: &mut Model, kind: &LibKind, from: &str, to: &str) {
    m.library.renamed(kind, from, to);
    m.lib_refs_mut(&mut |k, n| {
        if k == kind && n == from {
            *n = to.to_string();
        }
    });
}

/// An error naming the first key of `p` that isn't one of `keys`, the ones command `c` takes.
fn only(c: &str, p: &Value, keys: &[&str]) -> Result<(), String> {
    match p.as_object().and_then(|o| o.keys().find(|k| !keys.contains(&k.as_str()))) {
        Some(k) => Err(bad(c, format!("`{k}` isn't one of its parameters: {}", keys.join(", ")))),
        None => Ok(()),
    }
}

/// The name `p` gives under `key`, trimmed, unless it gives none or only spaces.
fn name_param(c: &str, p: &Value, key: &str) -> Result<Option<String>, String> {
    Ok(string(c, p, key)?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()))
}

fn line_pattern(s: &mut Session, p: &Value) -> R {
    let c = "line_pattern";
    only(c, p, &["name", "segments", "rename", "delete"])?;
    let Some(name) = name_param(c, p, "name")? else {
        return Ok(json!({"patterns": s.model.line_patterns}));
    };
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if m.line_patterns.remove(&name).is_none() {
            return Err(bad(c, format!("there's no line pattern `{name}`")));
        }
        if let Some((user, _)) = m.line_styles.iter().find(|(_, st)| st.pattern.as_deref() == Some(&*name)) {
            return Err(bad(c, format!("`{name}` is line style `{user}`'s pattern; give it another first")));
        }
        return Ok(json!({"deleted": name}));
    }
    let pattern = match (p.get("segments"), m.line_patterns.get(&name)) {
        (v, Some(was)) if absent(v) => was.clone(),
        (v, None) if absent(v) => {
            let there: Vec<&str> = m.line_patterns.keys().map(String::as_str).collect();
            return Err(bad(c, format!("there's no line pattern `{name}`; make one with `segments`{}", known_names(&there))));
        }
        (v, _) => serde_json::from_value(v.cloned().unwrap_or_default())
            .map_err(|e| bad(c, format!("`segments` is [{{\"dash\": mm}} | {{\"space\": mm}} | \"dot\", …]: {e}")))?,
    };
    let name = match name_param(c, p, "rename")? {
        Some(to) if to != name => {
            if m.line_patterns.contains_key(&to) {
                return Err(bad(c, format!("there's already a line pattern `{to}`")));
            }
            m.line_patterns.remove(&name);
            rename_entry(m, &LibKind::LinePattern, &name, &to);
            to
        }
        _ => name,
    };
    if name.eq_ignore_ascii_case(crate::lines::SOLID) {
        return Err(bad(c, format!("{} is a line with no pattern; name it another way", crate::lines::SOLID)));
    }
    pattern.check(&name).map_err(|e| bad(c, e))?;
    if m.line_patterns.get(&name) != Some(&pattern) {
        m.line_patterns.insert(name.clone(), pattern.clone());
    }
    Ok(json!({"name": name, "segments": pattern}))
}

fn line_style(s: &mut Session, p: &Value) -> R {
    let c = "line_style";
    only(c, p, &["name", "pen", "color", "pattern", "rename", "delete"])?;
    let Some(name) = name_param(c, p, "name")? else {
        return Ok(json!({"styles": s.model.line_styles}));
    };
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if m.line_styles.remove(&name).is_none() {
            return Err(bad(c, format!("there's no line style `{name}`")));
        }
        if let Some((id, _)) = m.elements.iter().find(|(_, e)| matches!(e, Element::DetailLine(l) if l.style == name)) {
            return Err(bad(c, format!("detail line {id} is drawn in `{name}`; give it another style first")));
        }
        let lineworked = |e: &Element| {
            graphics::graphics_of_element(e).is_some_and(|g| g.linework.iter().any(|l| l.style.as_deref() == Some(name.as_str())))
        };
        if let Some((id, _)) = m.elements.iter().find(|(_, e)| lineworked(e)) {
            return Err(bad(c, format!("view {id} draws an edge in `{name}` with Linework; take it off first")));
        }
        // What a filled region or a detail group's member is drawn in: the style is kept, and the error says where.
        let uses = |e: &Element| match e {
            Element::DetailLine(l) => l.style == name,
            Element::FilledRegion(r) => {
                r.line_style.as_deref() == Some(name.as_str()) || r.sketch.iter().any(|l| l.style.as_deref() == Some(name.as_str()))
            }
            _ => false,
        };
        if let Some((id, _)) = m.elements.iter().find(|(_, e)| matches!(e, Element::FilledRegion(_)) && uses(e)) {
            return Err(bad(c, format!("filled region {id} is bounded in `{name}`; give it another line style first")));
        }
        if let Some((group, _)) = m.detail_groups.iter().find(|(_, g)| g.members.iter().any(|x| uses(&x.element))) {
            return Err(bad(c, format!("detail group `{group}` has a member drawn in `{name}`; ungroup it and change that member first")));
        }
        return Ok(json!({"deleted": name}));
    }
    let mut style = match (m.line_styles.get(&name), p.get("pen")) {
        (Some(was), _) => was.clone(),
        (None, Some(_)) => LineStyle { pen: crate::pen::Pen::FINE, color: [0, 0, 0], pattern: None },
        (None, None) => return Err(bad(c, format!("there's no line style `{name}`; make one with `pen`"))),
    };
    if let Some(v) = p.get("pen").filter(|v| !v.is_null()) {
        style.pen = serde_json::from_value(v.clone()).map_err(|e| bad(c, format!("`pen`: {e}")))?;
    }
    if let Some(v) = p.get("color").filter(|v| !v.is_null()) {
        style.color = colour(v).ok_or_else(|| bad(c, format!("`color` is \"#rrggbb\" or [r, g, b], each 0 to 255, not {v}")))?;
    }
    if let Some(v) = p.get("pattern") {
        style.pattern = match v {
            Value::Null => None,
            Value::String(n) if n.trim().eq_ignore_ascii_case(crate::lines::SOLID) => None,
            Value::String(n) => Some(n.trim().to_string()),
            v => return Err(bad(c, format!("`pattern` is a line pattern's name, or \"{}\", not {v}", crate::lines::SOLID))),
        };
    }
    let name = match name_param(c, p, "rename")? {
        Some(to) if to != name => {
            if m.line_styles.contains_key(&to) {
                return Err(bad(c, format!("there's already a line style `{to}`")));
            }
            m.line_styles.remove(&name);
            rename_entry(m, &LibKind::LineStyle, &name, &to);
            to
        }
        _ => name,
    };
    style.check(&name, &m.line_patterns).map_err(|e| bad(c, e))?;
    if m.line_styles.get(&name) != Some(&style) {
        m.line_styles.insert(name.clone(), style.clone());
    }
    Ok(json!({"name": name, "style": style}))
}

/// The keys of a curve's fields, as a command takes them.
const CURVE_KEYS: &[&str] = &[
    "start",
    "end",
    "through",
    "arc",
    "points",
    "arcs",
    "closed",
    "centre",
    "radius",
    "radii",
    "angle",
    "span",
    "control",
    "knots",
    "fit",
    "start_tangent",
    "end_tangent",
];

/// The curve `p`'s curve fields give, checked: an error says which shape's fields to give.
fn curve_param(c: &str, p: &Value) -> Result<crate::curves::Curve, String> {
    let fields: serde_json::Map<String, Value> = p
        .as_object()
        .map(|o| o.iter().filter(|(k, _)| CURVE_KEYS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let f: crate::curves::Fields =
        serde_json::from_value(Value::Object(fields)).map_err(|e| bad(c, format!("{e}: a line is {}", crate::curves::FORMS)))?;
    // Checked as given first, so that a rule names the number that was given: 359.9999999999° is rounded to 360 below, and
    // would be refused for turning a whole circle, where the rule that refuses it is how far its huge circle reaches.
    let given = f.curve().map_err(|e| bad(c, e))?;
    given.check("the line").map_err(|e| bad(c, e))?;
    let curve = rounded(given);
    curve.check("the line").map_err(|e| bad(c, e))?;
    Ok(curve)
}

/// A curve with its points to the micron and an arc's turn to a billionth of a degree, what was meant to be exact without
/// float noise (a quarter turn drawn from three clicks is 90, not 90.00000000000001); nothing else of it changes.
fn rounded(c: crate::curves::Curve) -> crate::curves::Curve {
    use crate::curves::Curve;
    match c {
        Curve::Poly { points, arcs, closed } => {
            let arcs: Vec<f64> = arcs.into_iter().map(degrees).collect();
            let arcs = if arcs.iter().all(|a| *a == 0.0) { Vec::new() } else { arcs };
            Curve::Poly { points: points.into_iter().map(micron).collect(), arcs, closed }
        }
        Curve::Circle { centre, radius } => Curve::Circle { centre: micron(centre), radius },
        Curve::Ellipse { centre, radii, angle, span } => Curve::Ellipse { centre: micron(centre), radii, angle, span },
        Curve::Spline { control, knots } => Curve::Spline { control: control.into_iter().map(micron).collect(), knots },
        Curve::Fit { points, tangents } => Curve::Fit { points: points.into_iter().map(micron).collect(), tangents },
    }
}

fn detail_line(s: &mut Session, p: &Value) -> R {
    let c = "detail_line";
    only(c, p, &[&["view", "level", "style", "insulation", "centre_mark", "cloud", "revision"], CURVE_KEYS].concat())?;
    let m = &s.model;
    let curve = curve_param(c, p)?;
    let insulation = num(c, p, "insulation")?;
    let centre_mark = flag(c, p, "centre_mark")?;
    let cloud = num(c, p, "cloud")?;
    let revision = string(c, p, "revision")?;
    let view = view_param(c, m, p, true)?;
    let style = match name_param(c, p, "style")? {
        Some(n) => n,
        // Thin Lines, in the project's language or the English of a file from before them.
        None => crate::lines::default_style(&m.line_styles, m.language)
            .cloned()
            .ok_or_else(|| bad(c, "the project has no line style; make one first"))?,
    };
    let l = DetailLine { view: m.guid(view), curve, style, insulation, centre_mark, cloud, revision };
    let id = s.model_mut().insert(Element::DetailLine(l));
    Ok(json!({"id": id}))
}

fn linework(s: &mut Session, p: &Value) -> R {
    let c = "linework";
    only(c, p, &["view", "level", "id", "at", "style", "invisible", "remove"])?;
    let m = &s.model;
    let key = view_param(c, m, p, false)?;
    if m.plan_level(key).is_none() {
        return Err(bad(c, "Linework works in plans: give a level's plan or a plan view as `view`"));
    }
    let id = element_id(c, m, p.get("id").unwrap_or(&Value::Null))?;
    let at = point_req(c, p, "at")?;
    let (remove, invisible) = (flag(c, p, "remove")?, flag(c, p, "invisible")?);
    let style = name_param(c, p, "style")?;
    if let Some(st) = &style
        && !m.line_styles.contains_key(st)
    {
        let there: Vec<&str> = m.line_styles.keys().map(String::as_str).collect();
        return Err(bad(c, format!("there's no line style `{st}`{}", known_names(&there))));
    }
    if !remove && style.is_none() && !invisible {
        return Err(bad(c, "give `style`, a line style, or `invisible: true`; `remove: true` takes Linework off"));
    }
    let guid = m.guid(id);
    let source = m.plan_source(key);
    if remove {
        // The edge the element draws nearest `at`, and the Linework on that edge, no other: both faces of a thin wall lie
        // within reach of one pick, and the nearer may have none. Linework on no edge the element draws there (the edge
        // changed shape: a wall across a styled face cut it in two) is stale, and still styles what it overlaps. When the
        // nearest edge has none, the nearest stale entry within reach goes, so it can still be picked off; never one on
        // another edge that is drawn.
        let edges = render::edges_at(m, key, id, at, LINEWORK_REACH);
        let g =
            s.model_mut().elements.get_mut(source).and_then(graphics::graphics_mut).ok_or_else(|| bad(c, "that view has no graphics"))?;
        let before = g.linework.len();
        match edges.first() {
            Some(&(from, to)) if g.linework.iter().any(|l| l.element == guid && on_edge(l, from, to)) => {
                g.linework.retain(|l| !(l.element == guid && on_edge(l, from, to)));
            }
            _ => {
                let stale = |l: &&graphics::Linework| l.element == guid && !edges.iter().any(|&(from, to)| on_edge(l, from, to));
                let nearest = (g.linework.iter().enumerate().filter(|(_, l)| stale(l)))
                    .map(|(i, l)| (geom::dist_to_segment(at, l.from, l.to), i))
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                if let Some((_, i)) = nearest.filter(|(dist, _)| *dist <= LINEWORK_REACH) {
                    g.linework.remove(i);
                }
            }
        }
        return Ok(json!({"removed": before - g.linework.len()}));
    }
    let (from, to) = render::edge_at(m, key, id, at, LINEWORK_REACH).ok_or_else(|| {
        bad(c, format!("element {} draws no edge within {LINEWORK_REACH} mm of [{}, {}] in that view", m.shown(id), at.0, at.1))
    })?;
    let g = s.model_mut().elements.get_mut(source).and_then(graphics::graphics_mut).ok_or_else(|| bad(c, "that view has no graphics"))?;
    // The same edge again takes the newer style.
    g.linework.retain(|l| !(l.element == guid && on_edge(l, from, to)));
    g.linework.push(graphics::Linework { element: guid, from: micron(from), to: micron(to), style: if invisible { None } else { style } });
    Ok(json!({"from": [from.0, from.1], "to": [to.0, to.1]}))
}

/// How near an edge a Linework pick is taken, mm.
const LINEWORK_REACH: f64 = 200.0;

/// Whether `l` lies on the edge from `from` to `to` that [`render::edge_at`] found. Its ends come out of a pixel view round the
/// pick, in f32, so another pick finds them a little off the micron-rounded ends `l` holds: a thousandth of a mm, and a part
/// in a million of how long the edge is (8 times what two f32 roundings leave). So an edge is the same within 0.05 mm and that.
fn on_edge(l: &graphics::Linework, from: P2, to: P2) -> bool {
    let tol = 0.05 + 1e-6 * from.dist(to);
    l.from.dist(from) < tol && l.to.dist(to) < tol
}

fn import_fill_patterns(s: &mut Session, p: &Value) -> R {
    let c = "import_fill_patterns";
    only(c, p, &["path", "text", "replace"])?;
    let replace = flag(c, p, "replace")?;
    // A PAT file's text is far longer than a name: `text` takes as much as a file does (`pat::read` refuses more).
    let text = match p.get("text") {
        Some(Value::String(t)) => Some(t.as_str()),
        Some(v) if !v.is_null() => return Err(bad(c, "`text` must be the text of a PAT file")),
        _ => None,
    };
    let read = match (string(c, p, "path")?, text) {
        (Some(path), None) => crate::pat::read_file(&PathBuf::from(path)),
        (None, Some(text)) => crate::pat::read(text),
        _ => return Err(bad(c, "give `path`, a PAT file, or its `text`")),
    }
    .map_err(|e| bad(c, e))?;
    let m = s.model_mut();
    // A hostile file may leave out thousands: the reply names the first few and counts them all.
    let (mut imported, mut left_out, mut count) = (Vec::new(), Vec::new(), read.left_out_count);
    for (name, pattern) in read.patterns {
        if m.fill_patterns.contains_key(&name) && !replace {
            count += 1;
            if left_out.len() < crate::pat::MAX_LEFT_OUT {
                left_out.push(json!({"name": name, "why": "the project has a pattern of that name"}));
            }
            continue;
        }
        m.fill_patterns.insert(name.clone(), pattern);
        imported.push(name);
    }
    let room = crate::pat::MAX_LEFT_OUT.saturating_sub(left_out.len());
    left_out.extend(read.left_out.into_iter().take(room).map(|(name, why)| json!({"name": name, "why": why})));
    Ok(json!({"imported": imported, "left_out": left_out, "left_out_count": count}))
}

fn export_fill_patterns(s: &mut Session, p: &Value) -> R {
    let c = "export_fill_patterns";
    only(c, p, &["path", "names"])?;
    let m = &s.model;
    let names: Option<Vec<String>> = shaped(c, p, "names", "[name, …]")?;
    if let Some(missing) = names.iter().flatten().find(|n| !m.fill_patterns.contains_key(*n)) {
        return Err(bad(c, format!("there's no fill pattern `{missing}`")));
    }
    let chosen = m.fill_patterns.iter().filter(|(n, _)| names.as_ref().is_none_or(|w| w.contains(n)));
    let text = crate::pat::write(chosen);
    match string(c, p, "path")? {
        Some(path) => {
            crate::autosave::atomic_write(&PathBuf::from(&path), &text).map_err(|e| bad(c, e))?;
            Ok(json!({"path": path}))
        }
        None => Ok(json!({"text": text})),
    }
}

fn fill_pattern(s: &mut Session, p: &Value) -> R {
    let c = "fill_pattern";
    only(c, p, &["name", "target", "lines", "rename", "delete"])?;
    let Some(name) = name_param(c, p, "name")? else {
        return Ok(json!({"patterns": s.model.fill_patterns}));
    };
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if m.fill_patterns.remove(&name).is_none() {
            return Err(bad(c, format!("there's no fill pattern `{name}`")));
        }
        let uses = |t: &FilledRegionType| [&t.foreground, &t.background].into_iter().flatten().any(|n| *n == name);
        if let Some((user, _)) = m.region_types.iter().find(|(_, t)| uses(t)) {
            return Err(bad(c, format!("filled region type `{user}` fills with `{name}`; give it another first")));
        }
        let shows = |x: &crate::model::Material| [&x.cut_pattern, &x.surface_pattern].into_iter().flatten().any(|n| *n == name);
        if let Some((user, _)) = m.materials.iter().find(|(_, x)| shows(x)) {
            return Err(bad(c, format!("material `{user}` shows `{name}`; give it another first")));
        }
        return Ok(json!({"deleted": name}));
    }
    let mut pattern = match (m.fill_patterns.get(&name), absent(p.get("lines")) && absent(p.get("target"))) {
        (Some(was), _) => was.clone(),
        (None, false) => FillPattern::default(),
        (None, true) => {
            let there: Vec<&str> = m.fill_patterns.keys().map(String::as_str).collect();
            return Err(bad(c, format!("there's no fill pattern `{name}`; make one with `lines`{}", known_names(&there))));
        }
    };
    if let Some(t) = shaped::<Target>(c, p, "target", "drafting or model")? {
        pattern.target = t;
    }
    if let Some(lines) = shaped(c, p, "lines", "[{angle, spacing, origin?: [x, y], shift?, dashes?: [mm, -mm, …]}, …]")? {
        pattern.lines = lines;
    }
    let name = match name_param(c, p, "rename")? {
        Some(to) if to != name => {
            if m.fill_patterns.contains_key(&to) {
                return Err(bad(c, format!("there's already a fill pattern `{to}`")));
            }
            m.fill_patterns.remove(&name);
            rename_entry(m, &LibKind::FillPattern, &name, &to);
            to
        }
        _ => name,
    };
    pattern.check(&name).map_err(|e| bad(c, e))?;
    if m.fill_patterns.get(&name) != Some(&pattern) {
        m.fill_patterns.insert(name.clone(), pattern.clone());
    }
    Ok(json!({"name": name, "pattern": pattern}))
}

fn create_group(s: &mut Session, p: &Value) -> R {
    let c = "create_group";
    only(c, p, &["ids", "name"])?;
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    if ids.is_empty() {
        return Err(bad(c, "`ids`: the detail lines, filled regions and text notes to group"));
    }
    if ids.len() > crate::groups::MAX_MEMBERS {
        return Err(bad(c, format!("{} elements; a group holds at most {}", ids.len(), crate::groups::MAX_MEMBERS)));
    }
    let mut view = None;
    let mut members = Vec::new();
    let mut extent: Vec<P2> = Vec::new();
    for &id in &ids {
        let e = m.elements.get(id).ok_or_else(|| bad(c, format!("no element {id}")))?;
        if !crate::groups::holds(e) {
            return Err(bad(
                c,
                format!("{} {id} can't be in a detail group: it holds detail lines, filled regions and text notes", e.category()),
            ));
        }
        let v = m.annotation_plan(e);
        if view.is_some_and(|w| Some(w) != v) {
            return Err(bad(c, "the elements are in different views; a group's members are all in one"));
        }
        view = v;
        match e {
            Element::DetailLine(l) => extent.extend(l.curve.points()),
            Element::FilledRegion(r) => extent.extend(r.sketch.iter().flat_map(|l| l.points())),
            Element::Text(n) => extent.push(n.at),
            _ => {}
        }
        members.push(e.clone());
    }
    let view = view.ok_or_else(|| bad(c, "the elements are in no view"))?;
    let (lo, hi) = extent.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
        (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
    });
    if extent.is_empty() {
        return Err(bad(c, "the elements have no extent"));
    }
    let origin = micron((lo + hi) * 0.5);
    let name = match name_param(c, p, "name")? {
        Some(n) if m.detail_groups.contains_key(&n) => return Err(bad(c, format!("there's already a detail group `{n}`"))),
        Some(n) => n,
        None => (1..).map(|k| format!("Group {k}")).find(|n| !m.detail_groups.contains_key(n)).unwrap_or_default(),
    };
    // About the origin, in no view; a region's boundary lines get ids of their own again when they're elements' once more.
    let back = move |q: P2| q - origin;
    let mo = Motion { map: &back, slide: None, mirror: false };
    let mut held = Vec::new();
    for (e, id) in members.into_iter().zip(&ids) {
        let mut e = transform(c, m, e, *id, &mo, &BTreeSet::new())?;
        crate::groups::set_view(&mut e, Guid::default());
        if let Some(lines) = e.sketch_mut() {
            lines.iter_mut().for_each(|l| (l.id, l.guid) = (0, Guid::default()));
        }
        held.push(crate::groups::Member { id: 0, guid: Guid::default(), element: e });
    }
    delete(s, &json!({"ids": ids}))?;
    let m = s.model_mut();
    m.detail_groups.insert(name.clone(), crate::groups::GroupType { members: held });
    let g = crate::groups::DetailGroup { view, type_name: name.clone(), at: origin, angle: 0.0, mirrored: false };
    let id = m.insert(Element::DetailGroup(g));
    Ok(json!({"id": id, "name": name}))
}

fn detail_group(s: &mut Session, p: &Value) -> R {
    let c = "detail_group";
    only(c, p, &["type", "at", "angle", "mirrored", "view", "level"])?;
    let m = &s.model;
    let type_name = name_param(c, p, "type")?.ok_or_else(|| bad(c, "`type`, the detail group to place, is required"))?;
    if !m.detail_groups.contains_key(&type_name) {
        let there: Vec<&str> = m.detail_groups.keys().map(String::as_str).collect();
        return Err(bad(c, format!("there's no detail group `{type_name}` (there are: {})", there.join(", "))));
    }
    let at = point_req(c, p, "at")?;
    let angle = degrees(num(c, p, "angle")?.unwrap_or(0.0));
    let view = m.guid(view_param(c, m, p, true)?);
    let g = crate::groups::DetailGroup { view, type_name, at, angle, mirrored: flag(c, p, "mirrored")? };
    let id = s.model_mut().insert(Element::DetailGroup(g));
    Ok(json!({"id": id}))
}

fn repeating_detail(s: &mut Session, p: &Value) -> R {
    let c = "repeating_detail";
    only(c, p, &["type", "start", "end", "spacing", "mirrored", "view", "level"])?;
    let m = &s.model;
    let type_name = name_param(c, p, "type")?.ok_or_else(|| bad(c, "`type`, the detail group to repeat, is required"))?;
    let (start, end) = (point_req(c, p, "start")?, point_req(c, p, "end")?);
    let spacing = num(c, p, "spacing")?.ok_or_else(|| bad(c, "`spacing`, mm from one group to the next, is required"))?;
    let view = m.guid(view_param(c, m, p, true)?);
    let r = crate::groups::RepeatingDetail { view, type_name, start, end, spacing, mirrored: flag(c, p, "mirrored")? };
    let count = r.places().map_or(0, |v| v.len());
    let id = s.model_mut().insert(Element::RepeatingDetail(r));
    Ok(json!({"id": id, "count": count}))
}

fn ungroup(s: &mut Session, p: &Value) -> R {
    let c = "ungroup";
    only(c, p, &["id"])?;
    let m = &s.model;
    let id = element_id(c, m, p.get("id").unwrap_or(&Value::Null))?;
    let Some(Element::DetailGroup(g)) = m.elements.get(id) else {
        return Err(bad(c, format!("element {id} isn't a detail group")));
    };
    let members = crate::groups::placed(m, g);
    delete(s, &json!({"ids": [id]}))?;
    let m = s.model_mut();
    let ids: Vec<Id> = members.into_iter().map(|e| m.insert(e)).collect();
    Ok(json!({"ids": ids}))
}

fn group_type(s: &mut Session, p: &Value) -> R {
    let c = "group_type";
    only(c, p, &["name", "rename", "delete"])?;
    let placed = |m: &Model, name: &str| -> Vec<Id> {
        let of = |e: &Element| match e {
            Element::DetailGroup(g) => g.type_name == name,
            Element::RepeatingDetail(r) => r.type_name == name,
            _ => false,
        };
        m.elements.iter().filter(|(_, e)| of(e)).map(|(id, _)| *id).collect()
    };
    let Some(name) = name_param(c, p, "name")? else {
        let m = &s.model;
        let all: Map<String, Value> =
            m.detail_groups.iter().map(|(n, g)| (n.clone(), json!({"members": g.members.len(), "placed": placed(m, n).len()}))).collect();
        return Ok(json!({"groups": all}));
    };
    if !s.model.detail_groups.contains_key(&name) {
        return Err(bad(c, format!("there's no detail group `{name}`")));
    }
    if flag(c, p, "delete")? {
        if let Some(id) = placed(&s.model, &name).first() {
            return Err(bad(c, format!("detail group {id} is of `{name}`; delete or ungroup it first")));
        }
        s.model_mut().detail_groups.remove(&name);
        return Ok(json!({"deleted": name}));
    }
    let name = match name_param(c, p, "rename")? {
        Some(to) if to != name => {
            let m = s.model_mut();
            if m.detail_groups.contains_key(&to) {
                return Err(bad(c, format!("there's already a detail group `{to}`")));
            }
            if let Some(g) = m.detail_groups.remove(&name) {
                m.detail_groups.insert(to.clone(), g);
            }
            rename_entry(m, &LibKind::DetailGroup, &name, &to);
            to
        }
        _ => name,
    };
    Ok(json!({"name": name}))
}

fn filled_region_type(s: &mut Session, p: &Value) -> R {
    let c = "filled_region_type";
    only(c, p, &["name", "foreground", "foreground_color", "background", "background_color", "masking", "rename", "delete"])?;
    let Some(name) = name_param(c, p, "name")? else {
        return Ok(json!({"types": s.model.region_types}));
    };
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if m.region_types.remove(&name).is_none() {
            return Err(bad(c, format!("there's no filled region type `{name}`")));
        }
        if let Some((id, _)) = m.elements.iter().find(|(_, e)| matches!(e, Element::FilledRegion(r) if r.type_name == name)) {
            return Err(bad(c, format!("filled region {id} is of `{name}`; give it another type first")));
        }
        return Ok(json!({"deleted": name}));
    }
    let mut t = m.region_types.get(&name).cloned().unwrap_or_default();
    let pattern = |key: &str| -> Result<Option<Option<String>>, String> {
        match p.get(key) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(Value::String(n)) => Ok(Some(Some(n.trim().to_string()).filter(|n| !n.is_empty()))),
            Some(v) => Err(bad(c, format!("`{key}` is a fill pattern's name, or null for none, not {v}"))),
        }
    };
    if let Some(f) = pattern("foreground")? {
        t.foreground = f;
    }
    if let Some(b) = pattern("background")? {
        t.background = b;
    }
    for (key, slot) in [("foreground_color", &mut t.foreground_color), ("background_color", &mut t.background_color)] {
        if let Some(v) = p.get(key).filter(|v| !v.is_null()) {
            *slot = colour(v).ok_or_else(|| bad(c, format!("`{key}` is \"#rrggbb\" or [r, g, b], each 0 to 255, not {v}")))?;
        }
    }
    if !absent(p.get("masking")) {
        t.masking = flag(c, p, "masking")?;
    }
    let name = match name_param(c, p, "rename")? {
        Some(to) if to != name => {
            if m.region_types.contains_key(&to) {
                return Err(bad(c, format!("there's already a filled region type `{to}`")));
            }
            m.region_types.remove(&name);
            rename_entry(m, &LibKind::FilledRegionType, &name, &to);
            to
        }
        _ => name,
    };
    t.check(&name, &m.fill_patterns).map_err(|e| bad(c, e))?;
    if m.region_types.get(&name) != Some(&t) {
        m.region_types.insert(name.clone(), t.clone());
    }
    Ok(json!({"name": name, "type": t}))
}

fn filled_region(s: &mut Session, p: &Value) -> R {
    let c = "filled_region";
    only(c, p, &["sketch", "view", "level", "type", "line_style"])?;
    let m = &s.model;
    let lines = sketch_param(c, p, "sketch", &[])?.ok_or_else(|| bad(c, "`sketch`, its outlines' lines, is required"))?;
    let profile = sketch::solve(&lines)
        .map_err(|e| bad(c, format!("the sketch doesn't close into loops: {}", sketch::say(&e, &sketch::namer(&lines)))))?;
    let view = view_param(c, m, p, true)?;
    // Solid Black, in the project's language or in English, or its first type.
    let type_name = match name_param(c, p, "type")? {
        Some(n) => n,
        None => crate::fills::default_region_type(&m.region_types, m.language)
            .cloned()
            .ok_or_else(|| bad(c, "the project has no filled region type; make one first"))?,
    };
    // Thin Lines unless told; null (or "Invisible") draws none.
    let line_style = match p.get("line_style") {
        None => crate::lines::default_style(&m.line_styles, m.language).cloned(),
        Some(Value::Null) => None,
        Some(Value::String(n)) if n.trim().eq_ignore_ascii_case("invisible") => None,
        Some(Value::String(n)) => Some(n.trim().to_string()),
        Some(v) => return Err(bad(c, format!("`line_style` is a line style's name, or null for invisible lines, not {v}"))),
    };
    let r = FilledRegion { view: m.guid(view), type_name, line_style, sketch: lines, pattern_origin: P2(0.0, 0.0), pattern_angle: 0.0 };
    let id = s.model_mut().insert(Element::FilledRegion(r));
    Ok(json!({"id": id, "area_m2": m2(profile.area())}))
}

fn line_weights(s: &mut Session, p: &Value) -> R {
    let c = "line_weights";
    let reset = match p.get("reset") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(v) => return Err(bad(c, format!("`reset` is true or false, not {v}"))),
    };
    if reset {
        if s.model.line_weights.is_some() {
            s.model_mut().line_weights = None;
        }
    } else if !absent(p.get("model")) || !absent(p.get("annotation")) {
        let mut lw = s.model.line_weights().into_owned();
        if let Some(v) = p.get("model").filter(|v| !v.is_null()) {
            lw.model = serde_json::from_value(v.clone())
                .map_err(|e| bad(c, format!("`model` is [{{scale, widths: [the 16 pens' widths, mm]}}, …]: {e}")))?;
        }
        if let Some(v) = p.get("annotation").filter(|v| !v.is_null()) {
            lw.annotation =
                serde_json::from_value(v.clone()).map_err(|e| bad(c, format!("`annotation` is the 16 pens' widths, mm: {e}")))?;
        }
        let lw = lw.sorted();
        lw.check().map_err(|e| bad(c, e))?;
        if s.model.line_weights.as_ref() != Some(&lw) {
            s.model_mut().line_weights = Some(lw);
        }
    }
    let m = &s.model;
    let lw = m.line_weights();
    let model: Vec<Value> =
        lw.model.iter().map(|col| json!({"scale": col.scale, "name": m.units.scale(col.scale), "widths": col.widths})).collect();
    Ok(json!({"model": model, "annotation": lw.annotation, "own": m.line_weights.is_some()}))
}

fn object_styles(s: &mut Session, p: &Value) -> R {
    let c = "object_styles";
    let reset = match p.get("reset") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(v) => return Err(bad(c, format!("`reset` is true or false, not {v}"))),
    };
    if reset {
        if !s.model.object_styles.is_empty() {
            s.model_mut().object_styles.clear();
        }
    } else if let Some(v) = p.get("categories").filter(|v| !v.is_null()) {
        /// What a category is given: a pen or both.
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Given {
            projection: Option<crate::pen::Pen>,
            cut: Option<crate::pen::Pen>,
        }
        let given: BTreeMap<String, Given> = serde_json::from_value(v.clone())
            .map_err(|e| bad(c, format!("`categories` is {{category: {{projection?: pen, cut?: pen}}, …}}: {e}")))?;
        let mut styles = s.model.object_styles.clone();
        for (cat, g) in given {
            crate::styles::known(&cat).map_err(|e| bad(c, e))?;
            let was = s.model.object_style(&cat);
            let style = crate::styles::ObjectStyle { projection: g.projection.unwrap_or(was.projection), cut: g.cut.unwrap_or(was.cut) };
            style.check(&cat).map_err(|e| bad(c, e))?;
            if style == crate::styles::default_of(&cat) {
                styles.remove(&cat);
            } else {
                styles.insert(cat, style);
            }
        }
        if styles != s.model.object_styles {
            s.model_mut().object_styles = styles;
        }
    }
    let m = &s.model;
    let all: Vec<Value> = crate::graphics::MODEL
        .iter()
        .map(|cat| {
            let st = m.object_style(cat);
            json!({"category": cat, "projection": st.projection, "cut": st.cut, "own": m.object_styles.contains_key(*cat)})
        })
        .collect();
    Ok(json!({"categories": all}))
}

fn inspect(s: &mut Session, _: &Value) -> R {
    let m = &s.model;
    let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
    for e in m.elements.values() {
        *counts.entry(e.category()).or_default() += 1;
    }
    let levels: Vec<Value> = m.levels().iter().map(|(id, l)| json!({"id": id, "name": l.name, "elevation": l.elevation})).collect();
    let warnings: Vec<String> = m
        .elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Room(r) if m.room_region(r).is_none() => {
                Some(format!("room {id} ({} {}) is not enclosed: the walls around it no longer close", r.number, r.name))
            }
            _ => None,
        })
        .collect();
    Ok(json!({
        "name": m.name, "units": "mm", "project_units": m.units.name(), "language": m.language.code(), "path": s.path, "levels": levels,
        "counts": counts,
        "types": m.types, "materials": m.materials, "images": images(m), "families": families(m), "parameters": m.parameters,
        "view_filters": m.view_filters, "view_templates": m.view_templates, "phases": m.phases, "phase_filters": m.phase_filters,
        "phase_overrides": m.phase_overrides, "option_sets": m.option_sets, "warnings": warnings,
        "history": s.history(),
    }))
}

fn elements(s: &mut Session, p: &Value) -> R {
    let c = "elements";
    let m = &s.model;
    let cat = string(c, p, "category")?.map(|k| category(&k));
    let level = if absent(p.get("level")) { None } else { Some(level_param(c, m, p, "level")?) };
    let (limit, offset) = (count_opt(c, p, "limit", 500)?, count_opt(c, p, "offset", 0)?);
    let hits: Vec<Id> = m
        .elements
        .iter()
        .filter(|(_, e)| cat.as_deref().is_none_or(|k| e.category() == k))
        .filter(|(_, e)| level.is_none_or(|l| m.level_of(e) == Some(l)))
        .map(|(id, _)| *id)
        .collect();
    let page: Vec<Value> = hits.iter().skip(offset).take(limit).map(|id| element_json(m, *id)).collect();
    Ok(json!({"count": hits.len(), "returned": page.len(), "elements": page}))
}
