//! The command set: everything the CLI, scripts and MCP can do. Parameters are JSON, lengths are
//! millimetres. Commands build elements; [`Model::check`] then validates the whole model, so a
//! command only has to read its parameters carefully.

use bimcraft_i18n::{msg, ui_msg};

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::cad::{Cad, Colors};
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
    CEILING_HEIGHT, CadImport, Ceiling, CeilingType, Column, ColumnType, Component, DEFAULT_HEIGHT, DimKind, Dimension, Dimension3D,
    Element, End, Extrusion, Face, Facing, Floor, FloorType, Grid, Guid, Id, Key, Landing, Layer, Level, Line, Location, MAX_COORD,
    MAX_POINTS, MAX_SLOPE, MAX_TEXT, Mass, Model, Note, Opening, OpeningType, Orient, PARAMETER_CATEGORIES, ParameterKind, Pattern,
    Perspective, PlanView, PointCloud, ProjectParameter, RAMP_SLOPE, RAMP_WIDTH, RISER, Railing, RailingType, Ramp, Rebar, Ref3, Reference,
    Roof, RoofType, Room, RoomArea, RoomSeparator, STAIR_WIDTH, Schedule, Section, SectionBox, Shaft, Sheet, Side, Spot, Stair, TOL, TREAD,
    Tag, Unbound, View3D, Viewport, Wall, WallType, heading, paper,
};
use crate::paint;
use crate::phasing;
use crate::rebar;
use crate::render::Drawing;
use crate::sketch::{self, SketchLine};
use crate::texture::{self, Kind, Picture, Texture};
use crate::units::Units;
use crate::walls::{self, Axis, stacked};
use crate::workplane::{Surface, WorkPlane};
use crate::{pdf, render, schedule, sheet, snap, stairs, view3d};

type R = Result<Value, String>;

const fn edit(id: &'static str, label: &'static str, aliases: &'static [&'static str], params: &'static str, run: Run) -> Command {
    Command { id, label, aliases, params, run, undoable: true, menu: None }
}

/// A command that is no undo step: it only reads, or replaces the whole project.
const fn other(id: &'static str, label: &'static str, params: &'static str, run: Run) -> Command {
    Command { id, label, aliases: &[], params, run, undoable: false, menu: None }
}

pub static ALL: &[Command] = &[
    other(
        "new",
        "New",
        "{name?, language?: en|pt|es|sv|cs|fr|nl} an empty project with Level 1 (0) and Level 2 (3000), named in its language (Nível 1, \
         Nível 2 in Portuguese; Nivel 1, Nivel 2 in Spanish; Nivå 1, Nivå 2 in Swedish; Niveau 1, Niveau 2 in Dutch)",
        new,
    ),
    other(
        "open",
        "Open",
        "{path} a .bimcraft project (under 256 MiB), or an .ifc (under 512 MiB) read into a new one (its `elements` by \
         category, `generic` the components of IFC classes BIMCraft has no element for, `skipped` what was left out and why)",
        open,
    ),
    other("highrise", "Synthetic Highrise", "{storeys?: 20} replace the project with an original 1–80 storey performance sample", highrise),
    other(
        "tower",
        "Reference Tower",
        "{storeys?: 100, plate?: reference|small, annotation?: false, sheets?: false} replace the project with the original reference tower \
         of 1–100 storeys, about 10,000 elements and 195 rooms a storey: the baseline the core is measured on \
         (docs/spatial-core-plan.md); the `small` plate, 36 × 36 m, is the one compared with Revit (tools/revit-crosscheck). \
         With `annotation`, each storey's plan is detailed too: about 2,000 detail lines, text notes, filled regions and \
         dimensions on its walls; with `sheets`, a drawing set: each storey's floor plan, ceiling plan and two enlarged \
         halves on sheets of their own (A1-001 to A4-100), with general notes, and the project's 3D view, elevations, \
         section and schedules (A0-000, A5- to A7-)",
        tower,
    ),
    other(
        "autosave",
        ui_msg!("Autosave"),
        "{path} atomically write a recovery snapshot, keeping the project path and undo history",
        autosave,
    ),
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
    )
    .menu("Manage > Link Model"),
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
        "{path?} .bimcraft saves the project (Save As with a path); .ifc exports IFC4; .glb exports the default 3D view's scene as \
         glTF 2.0 (metres, y up, a node for each element named by its id, GUID and category in `extras`; returns `elements` and \
         `triangles`); .svg or .png exports the lowest level's plan; .pdf prints every sheet, a page each",
        save,
    ),
    other("undo", "Undo", "{count?} takes back the last `count` steps (1 by default)", undo),
    other("redo", "Redo", "{count?} does again the last `count` steps undone (1 by default)", redo),
    edit(
        "level",
        ui_msg!("Level"),
        &["LL"],
        "{elevation, name?, extent?: [[x,y],[x,y]]} a level at an elevation in mm; `extent`, the rectangle in plan its \
         plane stretches over (south-west corner, then north-east), where sections, elevations and the 3D view show it \
         (without one, the model's, a metre past it)",
        level,
    )
    .menu("Architecture > Level"),
    edit("grid", ui_msg!("Grid"), &["GR"], "{start: [x,y], end: [x,y], name?} names follow the last grid: 1, 2… or A, B…", grid).menu("Architecture > Grid"),
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
        "import_cad",
        "Import CAD",
        &[],
        "{path | cad, name?, level?, view?, at?: [x, y], position?, units?, scale?, angle?, layers?, colors?, link?} brings in a \
         DXF drawing (`path`: an ASCII DXF of any version, or a DWG, which the desktop and the command line read as DXF) or places one the project holds again (`cad`, \
         its name), as an element in every plan of `level`, or with `view` (a plan) in that view only. `units` of the drawing: \
         auto (the default: what its file says, else mm), mm, cm, m, in or ft; or `scale`, mm to each of its units. `position`: \
         center (the default: the middle of the drawing on `at`, the origin unless given) or origin (its origin on `at`); \
         `angle` turns it about `at`, degrees. `layers`: visible (the default: those its file has on and thawed), all, or a list \
         of those to show. `colors`: preserve (the default), invert or black. `link: true` remembers the file, so `reload_cad` \
         and opening the project read it again. Returns its id, the drawing's name, its unit, entities and layers, its extents \
         in the model, how many entities lie far from the rest (they're left out of its extents) and what was left out, with `notes` on what a DWG's read left out",
        import_cad,
    ),
    edit(
        "cad_layers",
        "CAD Layers",
        &[],
        "{id, hide?: [layer, …], show?: [layer, …], view?} a CAD import's layers, each with its colour and linetype and whether \
         it's shown; `hide` and `show` change which it shows in every view, or with `view` in that view only (its \
         Visibility/Graphics, by the drawing's name)",
        cad_layers,
    ),
    edit(
        "reload_cad",
        "Reload CAD",
        &[],
        "{cad} reads a linked CAD drawing's file again, so every import of it shows what the file holds now (opening the project \
         does too). Returns whether it changed",
        reload_cad,
    ),
    other(
        "cad_query",
        "Query",
        "{id, at: [x, y], reach?} what of a CAD import lies nearest `at`, within `reach` mm (100 unless given): its kind, layer, \
         colour, linetype and weight, as Revit's Query",
        cad_query,
    ),
    edit(
        "wall",
        msg!("Wall"),
        &["WA"],
        "{points: [[x,y],…], closed?, type?, level?, top_level?, top_offset?, height?, base_offset?, arc?, location?, attach_top?, \
         attach_base?, room_bounding?} one wall per segment of its location line; up to the next level, or `height` mm tall (unconnected), or up \
         to `top_level`, `top_offset` mm above it (0; negative, below it; the top must stay above the base). `arc`: degrees each turns from its start to its end, counter-clockwise (180, a half circle bulging to \
         its right). `location`: the line of it the points are on: centre (the default), core-centre, exterior-finish, \
         interior-finish, exterior-core or interior-core; its exterior side is to the left of its start to its end, so walls \
         drawn clockwise face out. `attach_top`, `attach_base`: a roof or floor its top follows from below, or its base stands \
         on. `room_bounding`: false for a wall rooms run past (a low wall, a balustrade); true, the default, bounds them",
        wall,
    )
    .menu("Architecture > Wall"),
    edit(
        "door",
        ui_msg!("Door"),
        &["DR"],
        "{at: [x,y] | wall: id, offset?, type?, family?, level?, mark?, flip_facing?, flip_hand?} in the wall under `at`, \
         or in `wall` centred `offset` mm from its start; with `family`, a door family's `type` (its first by default), \
         which sizes the opening and draws the door; it may rise past a low wall's top, as Revit lets it",
        door,
    )
    .menu("Architecture > Door"),
    edit(
        "window",
        "Window",
        &["WN"],
        "{at: [x,y] | wall: id, offset?, sill?, type?, family?, level?, mark?, flip_facing?} like door; `sill` defaults to the \
         type's (a family's `Sill Height`) and must be under the wall's top, while its head may rise past the top, as Revit \
         lets it (the wall is notched up to its top)",
        window,
    )
    .menu("Architecture > Window"),
    edit(
        "floor",
        ui_msg!("Floor"),
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
    )
    .menu("Architecture > Floor"),
    edit(
        "ceiling",
        ui_msg!("Ceiling"),
        &[],
        "{point: [x,y] | boundary: [[x,y],…], openings?: [[[x,y],…],…] | sketch: [[[x,y],[x,y]],…], level?, type?, offset?} a ceiling, its underside \
         `offset` mm (2600) above the level, open through `openings` (holes inside it, apart): in the room around `point`, out \
         to its walls' faces and round the walls standing inside it, or along `boundary`. Reflected ceiling plans show it",
        ceiling,
    )
    .menu("Architecture > Ceiling"),
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
    )
    .menu("Architecture > Shaft"),
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
        "edit_shape",
        "Modify Sub Elements",
        &[],
        "{id, points?: [{at: [x,y], z, id?}, …], lines?: [{a: [x,y], b: [x,y], id?}, …], absolute?} gives a floor's top, or a \
         flat roof's by footprint (its underside following), its shape anew, all at once: points `z` mm above (negative, below) where its top would be without them (with `absolute`, \
         at that elevation), and split lines it folds along, between which it's flat triangles, merged where neighbours lie \
         in one plane: falls to drains, ridges, valleys. Its outline's and openings' corners stay at 0 unless a point is \
         there; a line's end that is no point and no corner becomes a point at the height the top has there. Sent back with \
         its `id`, a point or a line keeps it. A shaped floor has no slope arrow, a shaped roof no `slope`. Without `points` or `lines`, gives the \
         shape: its points and lines with their ids, its corners at their heights, how many planes (`faces`) and its top's \
         lowest and highest (`top`, mm above zero)",
        edit_shape,
    ),
    edit(
        "add_point",
        "Add Point",
        &[],
        "{id, at: [x,y], z?, absolute?, offset?} a point on a floor's or flat roof's top: `offset` mm over the top as it is \
         there now (Revit's Along Surface), or `z` mm above where it would be without its shape (with `absolute`, at that \
         elevation); with neither, at the height the top has there now. Within a millimetre of a point it has, sets that \
         one's height",
        add_point,
    ),
    edit(
        "add_split_line",
        "Add Split Line",
        &[],
        "{id, a: [x,y], b: [x,y]} a line a floor's or flat roof's top folds along; an end that is no point of its shape and no corner of its \
         outline or an opening becomes a point, at the height the top has there now",
        add_split_line,
    ),
    edit(
        "reset_shape",
        "Reset Shape",
        &[],
        "{id} takes a floor's or roof's points and split lines all off: it's flat at its offset again",
        reset_shape,
    ),
    edit(
        "falls",
        "Falls",
        &[],
        "{id, drains: [[x,y], …], fall: 1.5, ridges?: [[[x,y], [x,y]], …]} gives a floor's or flat roof's top falls to its \
         drains, at no less than `fall` %: `ridges` (each from the outline, an opening or another ridge to another) split it \
         into parts, each needing a drain; in a part each drain takes what is nearest it, a ridge runs at the top's height \
         where two meet, a valley runs from each drain to each corner of its part (round an opening's corners, which lie on \
         the drain's cone, where it can't see straight), and each drain goes down as far as `fall` takes its farthest corner. \
         Its shape is given anew (points and split lines, as `edit_shape` would); the reply gives each drain's depth, how many \
         planes (`faces`) and its top's lowest and highest. Refused, saying why and where: a corner a drain can reach only \
         over the outline or a ridge, or a plane that can't fall as asked to a drain",
        falls,
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
        ui_msg!("Mass Floors"),
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
        ui_msg!("Wall by Face"),
        &["WB"],
        "{mass, faces?: [edge, …], type?, location?} a wall on each edge of the mass's footprint (numbered from its first \
         point), or the `faces` given, as tall as the mass; its exterior finish on the edge, facing out (`location` changes \
         that line). Ordinary walls from then on",
        wall_by_face,
    ),
    edit(
        "floor_by_face",
        ui_msg!("Floor by Face"),
        &["FB"],
        "{mass, type?} a floor on the mass's top face: its top at the mass's top level, or `height` over its base. An ordinary \
         floor from then on",
        floor_by_face,
    ),
    edit(
        "roof_by_face",
        ui_msg!("Roof by Face"),
        &["RB"],
        "{mass, type?, slope?, overhang?} a roof on the mass's top face, flat unless it has a `slope` (degrees), over the mass's \
         top level or `height` over its base. An ordinary roof from then on",
        roof_by_face,
    ),
    edit(
        "update_by_face",
        ui_msg!("Update to Face"),
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
    edit(
        "room",
        ui_msg!("Room"),
        &["RM"],
        "{point: [x,y], level?, name?, number?, top_level?, limit_offset?, base_offset?, frozen?} bounded by the walls and the \
         room separation lines around the point (walls the level's `computation_height` cuts; separation lines whatever their \
         height). Placed where nothing encloses it, or in a space another room has (two rooms can't share one; the first \
         keeps it), it is placed unbounded, a 1000 mm square, and the reply has a `warning`. Its height is Revit's: from \
         `base_offset` mm over its level (0) up to `limit_offset` mm over its upper limit `top_level` (null: Not Bounded, \
         over its own level). Unless given, 300 mm under the level above (`top_level` that level, `limit_offset` -300), or \
         with none above, not bounded and 2500 mm tall. `elements` gives its `unbounded_height` and `volume_m3`, its area \
         times that height. \
         `frozen` (a yes or no; also `modify {id, frozen}`) keeps the boundary the room has now: it keeps its size whatever \
         the walls do, and the walls are never searched for it again, which saves time in a project of many rooms. Unfrozen, \
         it follows the walls again. Only an enclosed room can be frozen",
        room,
    )
    .menu("Architecture > Room"),
    edit(
        "room_separator",
        "Room Separator",
        &[],
        "{points: [[x,y],…], level?, closed?} room separation lines along `points`, one per segment (`closed` joins the last \
         to the first): they bound rooms as walls do, to split an open space (a living room and its corridor) into rooms, \
         and are nothing else, with no solid, no cut and no area. Returns their ids",
        room_separator,
    ),
    edit(
        "rebar",
        "Rebar",
        &[],
        "{points: [[x,y,z],…], diameter, host?, grade?, layer?, closed?, count?, step?, level?} a reinforcing bar bent along `points` \
         (mm; z over the level), `diameter` mm thick (3 to 100), of steel `grade` (CA-50 unless told). `host` is the beam, \
         column, footing or floor it reinforces (the level is then its level) and it goes with; `closed` joins the last point \
         to the first, a hoop. With `count` bars (up to 1000), each is `step` [dx,dy,dz] from the last: a layer of bars, or \
         hoops along a beam. Returns their ids. Its schedule lists \
         Grade, Diameter, Length and Weight (kg, of steel at 7850 kg/m³), and a grouped one is a bar schedule",
        rebar_bars,
    ),
    edit(
        "reinforce",
        "Reinforce",
        &[],
        "{host | hosts: [id, …], preset?, cover?, grade?, grade_stirrup?, clearance?, …} bars by rules for a beam, a column, a footing or a floor: the engineer \
         says how many, how thick and how far apart (no standard's number sets them) and it lays them inside the cover and checks \
         that they fit, saying what to change. All: `cover` (mm: one number, or {top, bottom, sides}, top and bottom the faces of a \
         beam, a footing or a floor and the ends of a column; else the host's Cobrimento or ConcreteCover parameter), `grade` \
         (CA-50), `grade_stirrup` (the stirrups' and ties'), `clearance` (least mm between bars; their thickness, at least 20, \
         unless told). A spacing is the most between bars: they are spread evenly, so the last is where the first is from the \
         other end. Beam: `bottom` and `top` {count, diameter, hook?} (`hook`, the straight leg in mm turned up from the bottom bars and down from the top ones at both ends, or {start, end}); `bottom.bent` {count, angle?, top?} bends `count` of the inner bottom bars up at each end (layer `bent`): they run `top` mm along the top face from each end (a fifth of the length unless told), slope down `angle` degrees (15 to 75, 45 unless told) and run along the bottom, and have no hook; `stirrup` {diameter, spacing, ends?: {length, spacing}, hook?} \
         (`ends` closer in the first and last `length` mm; `hook` a tail in mm at the ends); `skin` {diameter, spacing} up each \
         side between the rows; `anchor` {start?, end?} mm the bars run past the ends (0 flush, none a cover short). Column: \
         `bars` {diameter, x?, y?, hook?, jog?} (`hook`, a leg turned into the section at the foot and the top, or {below, above}; `jog` {by, from_top, slope?} steps the bars in `by` mm toward the axis, starting `from_top` mm below the top and running `slope` mm up for each mm across (6 unless told), to leave room for the column above; bars along each face across its width and across its depth, corners counted, \
         2 each; a round column takes `count` instead of x and y, evenly round its circle, with hoops of 24 sides), `stirrup`, `ties` {diameter, hook?} \
         across a rectangular section between bars that are not at a corner (`hook`, a tail in mm at each end), `anchor` {below?, above?}. \
         Footing: `mesh` {x?, y?: {diameter, spacing}, hook?} (hook turns the ends up; a round footing's bars stop at its circle). Floor: `bottom` and \
         `top` {x?, y?: {diameter, spacing}, hook?} (bars along east and north; `hook`, a leg turned up from a bottom mesh and down from a top one), \
         stopping a cover short of its outline and its openings; a sloped floor's bars follow its top, the cover square to it. \
         The bars are `rebar` elements of the host, each on a `layer`; reinforcing again replaces the ones it made and leaves \
         any drawn by hand. `check` \"NBR 6118\" (with `aggressiveness` 1 to 4 for the cover, and `fck` in MPa) adds `notes` to the reply where the bars made fall short of that standard's usual limits (cover, bar sizes and spacings, stirrups, least and most steel, skin bars): they are notes, never a refusal, the numbers typed from handbooks and not the standard's text, so check one against a copy before relying on it. `preset` names a `reinforcement_preset` of the host's kind, whose parameters stand for the ones not given here. `hosts` arms each of a list alike (up to 1000, and 20000 bars in all); if one refuses, none is armed, \
         and the refusal names it. Returns ids, count, hosts, removed, weight_kg and notes",
        reinforce_host,
    ),
    edit(
        "reinforcement_preset",
        "Reinforcement Presets",
        &[],
        "{name?, kind?: beam|column|footing|floor, params?, rename?, delete?: true} a named set of the parameters `reinforce` takes for \
         one kind of host (`params`: its `cover`, `grade`, `grade_stirrup`, `clearance` and what its kind takes, and not `host`), kept in \
         the project; `reinforce` with `preset` takes it and lets what is given with it replace its own. Without `name`, lists them. \
         It is not checked against any host until it arms one",
        reinforcement_preset,
    ),
    edit(
        "voltage",
        "Voltage Definitions",
        &[],
        "{name?, nominal?, min?, max?, rename?, delete?: true} a voltage of the project's electrical settings, in volts: `nominal`, and the \
         band a device may read within, `min` to `max` (10 per cent either way unless given). Without `name`, lists them. A voltage a \
         distribution system names cannot be deleted; renamed, the systems follow",
        voltage,
    ),
    edit(
        "distribution_system",
        "Distribution Systems",
        &[],
        "{name?, phases?: 1|2|3, wires?, l_l?, l_n?, l_g?, ground?, rename?, delete?: true} a distribution system of the project's \
         electrical settings. `wires` counts the line conductors and the neutral, not the protective one (3 phases: 3 or 4). `l_l`, `l_n` \
         and `l_g` name the voltages between two lines, a line and the neutral, and a line and earth (at least one; `l_l` from 2 phases; with 3 \
         phases `l_l` is about 1.73 times `l_n`), each a `voltage` of the project; \"\" takes one away. `ground` is the earthing: TN-S, TN-C, \
         TN-C-S, TT or IT. A voltage the project lacks is refused, and the refusal lists the ones it has. Without `name`, lists them",
        distribution_system,
    ),
    edit(
        "wire_type",
        "Wire Types",
        &[],
        "{name?, material?: copper|aluminium, temperature?, insulation?, note?, sizes?: [{section, ampacity, label?}], rename?, delete?: true} \
         a wire type of the project's electrical settings: a conductor's `material`, working `temperature` (°C) and `insulation`, and \
         its table, `sizes`: the `section` of each row in mm² (smallest first, never less than the row before) and the `ampacity` it \
         carries in amperes, with a `label` for how its standard names it (12 AWG). Giving `sizes` replaces the table. `note` says where the \
         ampacities come from. Without `name`, lists them",
        wire_type,
    ),
    edit(
        "column",
        ui_msg!("Column"),
        &["CL"],
        "{at: [x,y], type?, angle?, level?, top_level?, height?, base_offset?} a column, its width along `angle` degrees; up to \
         the next level, or `height` mm tall, or up to `top_level`",
        column,
    )
    .menu("Architecture > Column"),
    edit(
        "structural_column",
        ui_msg!("Structural Column"),
        &["CL"],
        "{at: [x,y], type?, angle?, level?, top_level?, height?, base_offset?} a column of the structure, as `column` makes one but \
         in its own category with its own types (`type` with category structural column): up to the next level, or `height` mm \
         tall, or up to `top_level`. CL is `column`'s too; on the desktop's Structure tab it is this one",
        structural_column,
    ),
    edit(
        "footing",
        ui_msg!("Isolated Footing"),
        &["FI"],
        "{at: [x,y], type?, thickness?, offset?, angle?, level?} an isolated (pad) footing, its plan the type's (category \
         footing), `thickness` mm thick (400) with its top `offset` mm over the level (0: at it), so it hangs below",
        footing,
    ),
    edit(
        "beam",
        ui_msg!("Beam"),
        &["BM"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a beam for each \
         segment of the points: its section the type's (category beam: `width` across, `depth` deep, or round), its `justify` \
         line (top by default) `offset` mm over the level at its start (0: at the level, so it hangs below) and `end_offset` at \
         its end (the same: level), for a single segment; `system` names the system it belongs to. It slopes up to 75°",
        beam,
    ),
    edit(
        "strip_footing",
        ui_msg!("Wall Footing"),
        &["FW"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, system?} a continuous footing under a wall for each segment of the \
         points, its section the type's (category stripfooting), its top `offset` mm over the level (0: at it)",
        strip_footing,
    ),
    edit(
        "pipe",
        ui_msg!("Pipe"),
        &["PI"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a pipe for each \
         segment of the points: its section the type's (category pipe: round, `width` the outside diameter), its `justify` line \
         (centre by default) `offset` mm over the level at its start (0: at the level) and `end_offset` at its end (the same: level), \
         for a single segment, so a drain falls and a riser rises; the same point twice and two heights make one stand upright. \
         `system` names the system it belongs to (\"Água fria\", \"Esgoto\")",
        pipe,
    ),
    edit(
        "duct",
        ui_msg!("Duct"),
        &["DU"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a duct for each \
         segment of the points: its section the type's (category duct: `width` across by `depth` deep, or round, `width` the \
         diameter), its `justify` line (centre by default) `offset` mm over the level at its start (0: at the level) and \
         `end_offset` at its end (the same: level), for a single segment; the same point twice and two heights make one stand \
         upright. `system` names the system it belongs to (\"Supply air\", \"Exhaust\")",
        duct,
    ),
    edit(
        "conduit",
        ui_msg!("Conduit"),
        &["CN"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a conduit (eletroduto) \
         for each segment of the points, as `pipe` lays them: round, `width` its outside diameter; `system` names its circuit",
        conduit,
    ),
    edit(
        "cable_tray",
        ui_msg!("Cable Tray"),
        &["CT"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a cable tray (eletrocalha) \
         for each segment of the points, as `pipe` lays them: its section the type's `width` across by `depth` deep",
        cable_tray,
    ),
    edit(
        "wire",
        ui_msg!("Wire"),
        &["EW"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a wire (condutor) for \
         each segment of the points, as `pipe` lays them: round, `width` its outside diameter; `system` names its circuit",
        wire,
    ),
    edit(
        "down_conductor",
        ui_msg!("Down Conductor"),
        &["DC"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} a down conductor \
         (condutor de descida, NBR 5419) for each segment of the points, as `pipe` lays them: round, or a flat strip; the same point \
         twice and two heights make one stand upright down a wall; `system` names the protection system it belongs to",
        down_conductor,
    ),
    edit(
        "ground_conductor",
        ui_msg!("Earthing Conductor"),
        &["GC"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} an earthing conductor \
         (condutor de aterramento, the ring or mesh of NBR 5419) for each segment of the points, as `pipe` lays them, usually under \
         the level (a negative `offset`)",
        ground_conductor,
    ),
    edit(
        "cabling",
        ui_msg!("Cabling"),
        &["CB"],
        "{points: [[x,y],…], closed?, type?, level?, offset?, end_offset?, justify?: top|centre|bottom, system?} structured cabling \
         (cabeamento estruturado, NBR 14565) for each segment of the points, as `pipe` lays them: round, `width` the cable's outside \
         diameter; `system` names the link or the network it belongs to",
        cabling,
    ),
    edit(
        "stair",
        ui_msg!("Stair"),
        &[],
        "{start: [x,y], end?: [x,y] | angle?, level?, top_level?, height?, width?, riser?, tread?, landings?: [{at, turn?}, …], \
         railings?} a stair from the middle of its first riser at `start`, climbing toward `end` (or `angle` degrees) up to the next level: \
         as many risers as keep each at most `riser` mm (180), treads `tread` deep (280), `width` wide (1000). One straight \
         run, as long as that makes it; or with landings (each as deep as the stair is wide) after `at` risers, the next run \
         going on as `turn` says: 0 straight on, 90 or -90 turned left or right, 180 or -180 back beside it, to its left or right. \
         With `railings`, a railing down each side too",
        stair,
    )
    .menu("Architecture > Stair"),
    edit(
        "ramp",
        ui_msg!("Ramp"),
        &[],
        "{start: [x,y], end?: [x,y] | angle?, level?, top_level?, height?, width?, slope?, landings?: [{at, turn?}, …], railings?} a ramp \
         from the middle of its foot at `start`, climbing toward `end` (or `angle` degrees) up to the next level, or `height` mm, \
         `width` wide (1200), at `slope` percent (8.33, 1 in 12), as long as that makes it; with landings `at` mm up, turning \
         as a stair's do, and with `railings`, a railing down each side. Solid down to its level",
        ramp,
    )
    .menu("Architecture > Ramp"),
    edit(
        "railing",
        ui_msg!("Railing"),
        &[],
        "{host: a stair or a ramp, side?: left|right, type?} | {path: [[x,y],…], level?, base_offset?, type?} a railing along \
         the stair's or the ramp's sides (both, or the one going up it on `side`), 50 mm in from its edges and climbing with it, \
         going where it goes; or along `path`, `base_offset` mm above the level. Its type sets how high its rail stands over \
         what it runs along, and how far apart its balusters are",
        railing,
    )
    .menu("Architecture > Railing"),
    edit(
        "roof",
        ui_msg!("Roof"),
        &[],
        "{level?, boundary?: [[x,y],…], courtyards?: [[[x,y],…],…] | sketch?: [[[x,y],[x,y]],…], slope?, slopes?: [degrees, …], overhang?, gables?: [edge, …], gable?, offset?, \
         extrusion?, openings?: [[[x,y],…],…], join?, type?} a roof on `level` (by default the one above the lowest), its \
         base `offset` mm above it. Over `boundary` or else the walls of the level below, `overhang` mm (500) past their \
         faces, round any `courtyards` (holes in it, inside it and apart, with eaves of their own; over the walls, by \
         default each loop of them standing free inside, joined to none of the outer ones and with no room in it): flat \
         at `slope` 0, else every edge sloping up at `slope` degrees (30), hips, valleys and ridges where the slopes meet, \
         but the edges in `gables` (edge i runs from point i to the next, the courtyards' edges numbered on after the \
         boundary's) left upright, gable ends; `gable` leaves a rectangle's short ends upright. `slopes`: a slope in \
         degrees (up to 75) for each edge, numbered as `gables` are, 0 for an edge that takes `slope`; none, every edge \
         does; the eaves stay level and an edge in `gables` stays upright whatever its slope. A `sketch` line may carry \
         a `slope` for the edges along it. Or with `extrusion` {line: \
         [[x,y],[x,y]], profile: [[along, up],…], from?, to}: its underside's profile in the upright plane through `line`, \
         along it from its start and up from the base, swept square to it from `from` (0) to `to` mm to its left. \
         `openings` are holes through it in plan; with `join`, a roof it runs into, it ends where it meets that one's top",
        roof,
    )
    .menu("Architecture > Roof"),
    edit(
        "dormer",
        ui_msg!("Dormer"),
        &[],
        "{host, at: [x, y], width?, height?, slope?, window?, type?} a dormer on roof `host`, its front wall across the \
         slope through `at`: walls of `type` `width` mm (1600) apart, standing on the roof and `height` mm (1200) tall at \
         the front, under a gable roof of its own sloping `slope` degrees (30) that runs back into the host; the host open \
         under it, and with `window` (a window type) a window in its front. Replies with its roof, walls and window",
        dormer,
    )
    .menu("Architecture > Dormer"),
    edit(
        "dimension",
        ui_msg!("Aligned Dimension"),
        &["DI"],
        "{refs: [ref, …], at: [x, y], level?, kind?: aligned|linear|angular|radial|diameter|arc_length, orient?: \
         horizontal|vertical} aligned (the default): the distances between parallel references, along a line through `at` \
         (between points, along the first two; points and lines, square to the lines); linear: the same with its line \
         horizontal or vertical as `orient` says, or without one as `at` lies: east or west of the points, vertical, else \
         horizontal (its lines must run square to it); angular: the angle between two references that meet, on the side \
         of `at`; radial: a round column's, curved wall's, circle's or arc's radius, toward `at`; diameter: the same \
         across, through its centre; arc_length: how long a curved wall's centre line or face, or an arc, is along it \
         (all of it, or with two point refs after it, between them), its line an arc through `at`. A ref is {id, face?}: a wall's centre (the default), left or right face (of its \
         start-to-end direction), start or end; a door's or window's centre, or its jamb toward its wall's start or end; \
         a grid's line; a straight detail line, or its start or end (radial: a circle or an arc of one). With `point: \
         start|end`, {id, face?, point} is the point at that end of the line: a wall's centre line's end, a face's \
         corner, a grid's, a detail line's (of any shape) or a jamb's end. Or a point [x, y] on one of those lines (for \
         radial and diameter, on a round edge; for arc_length, on the arc), or {endpoint: [x, y]}, the end of one nearest \
         it, within 100 mm. It follows what it measures",
        dimension,
    ),
    edit(
        "dimension_3d",
        ui_msg!("Aligned Dimension in 3D"),
        &[],
        "{refs: [ref, ref], at: [x, y, z], view?, type?} an aligned dimension in a 3D view (the default 3D view unless `view` \
         names another, by name or id), kept by the view and drawn in it and on a sheet that holds it: how far apart its two \
         references are, along a line through `at` moved square off them. A ref is {at: [x, y, z]}, a point of the model, or \
         {at, edge: [[x, y, z], [x, y, z]]}, a point on an edge, which it measures square to (from a point, to the edge's line; \
         between two parallel edges, how far apart they are). `type` is a dimension type, the aligned one's unless told. On the \
         desktop, Aligned Dimension (DI) in the 3D view runs it. Returns its id and its length",
        dimension_3d,
    ),
    edit(
        "text",
        ui_msg!("Text"),
        &["TX"],
        "{text, at: [x, y], view?, type?, angle?, leader?: [x, y], override_format?, runs?: [{from, to, bold?, italic?, \
         underline?}, …]} a text note, its first line's top left at `at`, in a view by name or id: a level's plan (the lowest \
         by default; `at` in plan mm), a section or an elevation (`at` along its line from its start, and up), or a sheet by \
         number (`at` in paper mm). Lettered as its text `type` says (2.5mm unless told; `text_type` lists and makes them: \
         its font, its size, its border), turned `angle` degrees counter-clockwise; with `leader`, a line to that point. \
         Lines break at \\n. With `override_format`, each run makes characters `from` up to `to` (counted in characters, a \
         line break one) bold, italic or underlined (true), or not (false), whatever the type says; they stay when the \
         type changes, and `modify` with a new `text` alone keeps them on what didn't change. Font and size are the type's",
        text_note,
    ),
    edit(
        "text_type",
        "Text Types",
        &[],
        "{name?, font?, size?, bold?, italic?, underline?, width_factor?, color?, border?, border_offset?, border_pen?, \
         like?, rename?, delete?: true} a text type of the project's: its `font`, any the system has, or one BIMCraft carries \
         (IBM Plex Sans, IBM Plex Mono, BIMCraft Lettering: single-stroke capitals); Arial, the default, letters on any system \
         (where it isn't installed, Liberation Sans, Arimo, Helvetica or the like stands in, else IBM Plex Sans), and a font a \
         file names that this system hasn't is stood in for likewise. Its capitals' height on paper (`size`, mm, 2.5 for a new \
         one), bold, italic and underlined or not (a family's own bold and italic faces where it has them), `width_factor` (1: \
         as the font draws it), its colour (\"#rrggbb\" or [r, g, b], black unless told), and whether a border is drawn round \
         its notes, `border_offset` mm on paper out from their text (where a leader starts too; 1), in pen `border_pen` (0 to \
         16; 1). A new one starts as `like` says, or as 2.5mm Arial. A project starts with 2.5mm, 3.5mm and 5mm. `rename` \
         renames it, its notes following; `delete` takes it away while no note is of it. Without `name`, it lists every type, \
         the fonts this system has, and the font that letters each type here (`drawn_with`)",
        text_type,
    ),
    edit(
        "dimension_constraint",
        "Dimension Constraints",
        &[],
        "{id, equal?: bool, locked?: [segment, …]} an aligned or linear dimension's constraints, as Revit's EQ and padlocks: \
         `equal` keeps its segments equal (what it measures to between its first and last moves to keep them so, its ends \
         staying); `locked` lists the segments (counted from 0, west to east or south to north) kept at the lengths they \
         have now. After any command, what a constrained dimension measures to moves with what the command moved, or the \
         command is refused, saying which dimension can't be kept",
        dimension_constraint,
    ),
    edit(
        "dimension_text",
        "Dimension Text",
        &[],
        "{id, segment?, prefix?, suffix?, above?, below?, replace?} a dimension's text, as Revit's Dimension Text: what goes \
         before and after its numbers, over and under them, or in place of them. With `segment` (its numbers counted from 0, \
         west to east or south to north, as `labels` lists them), that one's own, over the dimension's where it says \
         something; an empty text clears it, and a segment with none left goes back to the dimension's",
        dimension_text,
    ),
    edit(
        "dimension_type",
        "Dimension Types",
        &[],
        "{name?, like?, rename?, delete?: true, …values} a dimension type of the project's, as Revit's type parameters, \
         lengths mm on paper. Graphics: `string` continuous|baseline|ordinate (Dimension String Type; baseline lines \
         `baseline_spacing` apart, 7), `tick` diagonal|arrow|open_arrow|dot|none, `tick_size` (2), `line_pen` and `tick_pen` \
         (0 to 16; 4 and 7), `color` (\"#rrggbb\" or [r, g, b]), `line_extension` past the outer witness lines (0.8), `witness` \
         gap_to_element|fixed (Witness Line Control), `witness_length` when fixed (5), `witness_gap` off what it measures \
         (0.6), `witness_extension` past the dimension line (0.8), `centre_mark` on radial and diameter ones. Text: `font` (as \
         text types take; Arial), `text_size` (2.5), `bold`, `italic`, `underline`, `width_factor` (1), `text_offset` off \
         the line (1), `background` opaque|transparent, `read` up_then_left|up_then_right (Read Convention), `units` (mm, cm, \
         m, in, ft, ft-in; null for the project's) and `decimals` (0 to 6; null as the units show), `radius_symbol` (R), \
         `diameter_symbol` (Ø), `equality_text` (EQ). A new one starts as `like` says, else as the default. A project starts \
         with `Diagonal - 2.5mm Arial` (aligned, linear, angular and arc length dimensions start in it) and `Arrow - 2.5mm \
         Arial` (radial and diameter ones). `rename` renames it, its dimensions following; `delete` takes it away while no \
         dimension is of it. Without `name`, it lists every type",
        dimension_type,
    ),
    edit(
        "spot",
        ui_msg!("Spot Elevation"),
        &["EL"],
        "{at: [x, y], view?, host?} a spot elevation: in a level's plan (the lowest by default), the height of the top of \
         what stands on the level at `at` (or of `host`), following it; in a section or an elevation, of its point `at`, \
         along and up",
        spot,
    ),
    edit(
        "spot_slope",
        ui_msg!("Spot Slope"),
        &[],
        "{at: [x, y], view?, host?} a spot slope in a level's plan (the lowest by default): how steeply the top of the floor \
         or roof at `at` (or `host`) falls there, %, an arrow pointing downhill; it follows the top. The reply gives `slope` \
         and `toward`, the way down in plan (null on the level)",
        spot_slope,
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
        "{category: wall|floor|ceiling|door|window|column|structuralcolumn|footing|roof|railing|beam|stripfooting|pipe|duct|conduit|\
         cabletray|wire|downconductor|groundconductor|cable, name, thickness? | width?, height?, sill?, operation? | width?, depth?, round?, \
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
        "{name, color?: \"#rrggbb\" | [r, g, b], transparency?: 0-100, cut?, surface?, texture?: {kind?, size?: [w, h], angle?, offset?: [x, y], path?, \
         image?} | null, rename?, delete?: true} a material of the project's library, created or changed: its colour in shaded 3D \
         views, its `transparency` there (percent of what lies behind it that shows through, 0 opaque; the starter Glass has 70), \
         the pattern its cut shows in plans and sections (where a layer is wide enough to hold it), and the one its \
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
    )
    .menu("Manage > Materials"),
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
    )
    .menu("Manage > Favorites"),
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
    )
    .menu("Manage > Project Parameters"),
    edit(
        "family",
        "Load Family",
        &[],
        "{name?, category?: furniture|casework|plumbing|lighting|generic|door|window, parameters?: [{name, kind?: length|number|\
         angle|yesno|material, instance?, value?, formula?}], types?: {type: {parameter: value}}, forms?: [{plane?: xy|xz|yz, \
         profile?: [[a, b], …], circle?: {centre: [a, b], radius}, axis?: [[a, b], [a, b]], from?, to, material?, visible?, \
         void?}], symbols?: [{line?: [[x, y], …] | arc?: {centre: [x, y], radius, from?, to} | text?: {at: [x, y], height, value, align?: left|center|right} | dot?: {centre: [x, y], radius}, visible?}], form?: {id | guid, …a \
         form's fields}, symbol?: {id | guid, …a symbol's fields}, delete_form?: [id | guid, …], delete_symbol?: [id | guid, …], \
         operation?: swing|double-swing|sliding|double-sliding|folding|fixed (a door's or window's, for IFC and schedules), \
         kind?: which fixture or fitting it is, for IFC's predefined type (plumbing: wc|lav|sink|laundry|shower|bath|bidet|urinal; \
         drain: floor_drain|floor_waste|roof_drain; pipefitting: bend|tee|cross|transition|coupling; \"\" clears it), \
         ifc?: {class, predefined?, object_type?} | null (the IFC class a generic family's components go out as, which one \
         opened from IFC remembers: `IFCPLATE`, `CURTAIN_PANEL`; one IFC4 lacks goes out as a proxy that names it), \
         connectors?: [{kind: power|switch, system: power-balanced|power-unbalanced|switch, poles?: 1-3, voltage?, load_va?, \
         power_factor?, classification?}], rename?, delete?: true, dry_run?, path?, save?, overwrite?: no|version|version_and_values} a family of \
         the project's, BIMCraft's own parametric component: created, changed, read, or loaded from the .json file at `path` (a \
         family as `inspect` shows it, with its `name`). Without `name` it lists the project's families, each its category and \
         how many types, forms and symbols it has; with only a `name` it reads one — its definition, each of its types' \
         parameter values worked out, and `warnings` — and changes nothing: no undo step, no file. `dry_run` reads a change the \
         same way and replies what it would do (`changes`: the parameters and types added, taken away and changed, the forms \
         and symbols replaced, patched and deleted by id, the `rename`) instead of doing it; a change that's refused comes back \
         as an error, as it would without it. `form` merges its fields over the one form of that `id` or `guid` (a `null` takes \
         a field away; its place, its id and its GUID stay), `delete_form` takes those forms away; `symbol` and `delete_symbol` \
         do the same for symbols — where `forms` and `symbols` still replace whole lists. Every reply carries `warnings`: what's \
         worth knowing and doesn't stop the change (a void cutting no solid, a form outside the opening or thicker than its \
         wall, a door's or window's plan with no symbols). On `path`, `overwrite` chooses `no` (keep the project family), \
         `version` (replace its definition, keeping compatible project values), or `version_and_values` (also use incoming \
         values). Omitted: legacy merge. Explicit reloads retain project-only types and instance overrides; formulas come \
         from the file. Its forms are profiles drawn in its own axes (x along a component's \
         angle, y to its left, z up from its base) and swept: `xy` up from `from` to `to`, `xz` along y, `yz` along x. With an \
         `axis`, a line in that plane, the profile is turned about it instead, `from` to `to` degrees (up to 360, right-handed \
         from the axis' first point toward its second; it can't cross the axis): a pipe, a bowl, a lamp shade. A `void` is no \
         solid: it cuts the solid forms listed before it that it passes through, a hole, a notch, a sink's bowl, and is never \
         seen itself (forms listed after it are whole). A `door` or `window` family stands in a wall, placed with `door` or \
         `window` and a `family`: its forms are drawn in the opening's axes (x along the wall from the middle of the opening, y \
         across it, 0 in the middle of the wall and + toward its exterior, z up from the sill), it must have length parameters \
         `Width` and `Height` (the opening it makes; `Sill Height` is a window's default sill), and its formulas read \
         `Wall Thickness`, the wall's. Sizes are mm, or \
         formulas over its parameters as Revit's are: `Width / 2`, `if(Depth > 600, 4, 2)`, with + - * / ^, < > = <= >=, if, \
         and, or, not, min, max, abs, sqrt, round, roundup, rounddown, sin, cos, tan (degrees). A parameter is each type's, or \
         with `instance` each component's. `types` merges into its types (a null takes one away); `parameters` and `forms` \
         replace its own; `material` is a material's name or a material parameter's; `visible` a yes/no parameter or formula. \
         `inspect` lists the families. A device's `connectors` say what it draws: one `power` connector at most (a second is \
         refused), `voltage` in volts, `load_va` in VA (a number or a formula over its parameters, so each type has its load; \
         `power-unbalanced` may take a list, one load for each pole), `power_factor` over 0 up to 1, and its `classification` \
         (`lighting`…); a `switch` connector carries no load. A component reports them as `connectors` in `elements`; a \
         lighting, outlet or switch family without one counts as zero load, which `inspect` warns of; `connectors` replace \
         their own, as `parameters`, `forms` and `symbols` do. The `symbols` of a family of any category are what its plan \
         draws besides its forms, in its own axes, turned and moved (and mirrored) with the component or opening and hidden at \
         coarse detail: each one `line` (points), `arc` (degrees from +x counter-clockwise, up to a full turn), `text` or `dot`, \
         one of them, with `visible` a yes/no parameter or formula. A `text` is lettered in BIMCraft's own strokes, capitals \
         `height` mm tall, `at` the middle of the line of text vertically (`align` says whether `at` is where it starts, its \
         middle or its end); its anchor moves with the component but its letters always read upright, left to right, never \
         turned or mirrored; `{Parameter Name}` in its \
         `value` is replaced by that parameter's value (a name the family lacks is refused), up to 256 characters. A `dot` is \
         a solid disc in ink of a radius above zero. Symbols draw in the plan and ceiling plan of the component's level whatever height \
         it has, and on no other level's. IFC leaves symbols out",
        family,
    )
    .menu("Manage > Family Editor"),
    edit(
        "symbol_set",
        "Load Symbol Set",
        &[],
        "{set: arei, names?: en|nl|fr} loads a set of families that are plan symbols and nothing else, each of one type \
         `Standard` with no forms: `arei` is the Belgian AREI/RGIE's graphical symbols (Table 2.23: switches, sockets, light \
         points, appliances, sources, domotics, protection), 76 families, switches in the switch category, sockets in outlet, \
         luminaires in lighting and the rest generic. `names` is the language of their names, English, Dutch or French as the \
         source prints them; as given none, French in a French project and English otherwise. A family the project has by \
         that name stays as it is, so loading twice changes nothing; the reply lists `loaded` and `kept`. Place them as \
         components",
        symbol_set,
    ),
    edit(
        "component",
        ui_msg!("Component"),
        &["CM"],
        "{family, type?, at: [x,y], level?, angle?, offset?, mirrored?, params?: {parameter: value}} a family's type placed, its \
         first by default: at a point on a level, turned `angle` degrees counter-clockwise, `offset` mm up from the level, and \
         with `mirrored` flipped across its own x axis; `params` sets its instance parameters (`modify` merges into them, a null \
         taking one back to the family's)",
        component,
    )
    .menu("Architecture > Component"),
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
        ui_msg!("Match Properties"),
        &["MA"],
        "{from: id, ids: [id,…]} gives the elements in `ids` the type of `from` (and, for components, its family), as Revit's Match \
         Properties does; only elements of `from`'s category take it, the rest come back in `skipped`. Returns {modified, skipped}",
        match_properties,
    ),
    edit(
        "align",
        ui_msg!("Align"),
        &["AL"],
        "{id, reference: [[x,y],[x,y]], line: [[x,y],[x,y]], lock?: true, to?: id} aligns an element's picked line with a \
         reference in plan: turns by the smaller angle about the picked line's midpoint, then moves it onto the reference, keeping \
         its size. Doors and windows slide along their host wall; they cannot turn independently. With `lock`, Revit's padlock: \
         the line and the reference (of element `to`, else whatever has a line along it) are kept on one line after every \
         command, as a locked dimension keeps its length; refused when that can't hold with the other locks. Returns {aligned, \
         angle, by, line} (the picked line where it went), and the lock's id",
        align_element,
    ),
    edit(
        "dimension_value",
        ui_msg!("Dimension Value"),
        &[],
        "{id, value, segment?} types a length into an aligned or a linear dimension's segment (the first unless told), as a \
         click on its number does: what the segment ends at (its second reference, as the dimension runs west to east or south \
         to north) moves along the dimension so the segment is `value` mm; a locked segment is locked at it, and the other \
         locks hold, or it is refused. Returns {id, moved, by}",
        dimension_value,
    ),
    edit(
        "move",
        ui_msg!("Move"),
        &["MV"],
        "{ids: [id,…], by: [dx, dy], end?: start|end} moves elements in plan; walls joined to a moved wall stretch to stay \
         joined, and doors and windows slide along their walls. With `end`, only that end of one wall moves",
        move_elements,
    ),
    edit(
        "copy",
        msg!("Copy"),
        &["CO"],
        "{ids: [id,…], by: [dx, dy]} copies elements, with the doors and windows of copied walls; returns the copies' ids",
        copy,
    ),
    edit(
        "rotate",
        ui_msg!("Rotate"),
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
        ui_msg!("Trim/Extend to Corner"),
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
        ui_msg!("Split"),
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
        ui_msg!("Fillet Arc"),
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
        "{name?, segments?: [{dash: mm} | {space: mm} | \"dot\" | {dot_length: mm} | {offset: {length, count, distance}} | {wave: {length, frequency, amplitude}} | {zigzag: {length, frequency, amplitude}} | {text: {length, size, text}}, …], rename?, delete?: true} a line pattern of the project's: \
         1–32 freely ordered graphic segments, each 0.05–100 mm long on paper, so it reads the \
         same at any scale. Offsets have 2–9 lines spaced symmetrically; frequency is cycles per segment, 0.1–100; \
         amplitude and spacing are mm. Text uses bundled IBM Plex Sans, size is maximum capital height (shrinks to fit length), \
         1–256 single-line characters, following the curve. All graphics preserve the centerline. A project starts with Dash, Long Dash, Dot, Dash Dot, Hidden and Centre; Solid is a line with \
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
    )
    .menu("Manage > Line Styles"),
    edit(
        "fill_pattern",
        "Fill Patterns",
        &[],
        "{name?, target?: drafting|model, lines?: [{angle, spacing, origin?: [x, y], shift?, dashes?: [mm, -mm, …]}, …], \
         drawn?: scale | null, rename?, delete?: true} a fill pattern of the project's, as Revit's (and a PAT file's): families of parallel lines, each at \
         `angle` degrees through `origin`, its lines `spacing` apart and each `shift` along from the last, whole or dashed \
         (`dashes`: a dash's length, a space's as a negative one, 0 a dot); no lines, it fills solid. A drafting pattern's \
         lengths are mm on paper, the same at any scale; a model pattern's mm in the building. A project starts with Solid \
         fill, Diagonal Up and Down, Crosshatch, Diagonal Crosshatch, Horizontal, Vertical, Sand, Brick 225 x 75 and Tile 300. \
         `rename` renames it, the region types filling with it following; `delete` takes it away while no type uses it. \
         `drawn` is the scale (1 : n, its n) of the view one made of lines (`make_fill_pattern`) was drawn in: such a one given \
         a new `target` and no `lines` is rescaled, a drafting one being the drawing on paper at that scale; null forgets it. \
         Without `name`, it lists every pattern",
        fill_pattern,
    )
    .menu("Manage > Fill Patterns"),
    edit(
        "make_fill_pattern",
        "Make Fill Pattern",
        &[],
        "{ids, from: [x, y], to: [x, y], name, target?: model|drafting} a fill pattern made of the detail lines `ids` (all of \
         one view), as pyRevit's Make Pattern makes one: the rectangle between corners `from` and `to` (in the view's terms) \
         is the tile the pattern repeats, and what of the lines lies outside it is cut off. Each line becomes a family that \
         runs on into the next tiles' copies of itself; one at a slope that never meets itself again is turned to the \
         simplest slope within 0.25° that does (else the nearest, at most 48 tiles across or up). A model pattern (the \
         default) is as drawn; a drafting one is the drawing on paper, at the view's scale. The tile is 0.05 to 100000 \
         each way, and holds at most 256 lines. It returns the pattern's `name` and the `pattern`",
        make_fill_pattern,
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
        "export_crosscheck",
        "Export Cross-Check",
        "{path?} the project's levels, walls (by their centre lines), floors, shafts, doors, windows and rooms as plain \
         numbers in mm, which tools/revit-crosscheck builds again in another BIM tool to hold its numbers against ours \
         (compare_crosscheck): written to `path`, or returned as `description` without it",
        export_crosscheck,
    ),
    other(
        "compare_crosscheck",
        "Compare Cross-Check",
        "{path? | text?} another tool's numbers of the project export_crosscheck described, a table `kind,id,quantity,value` \
         (rooms' enclosed, area_m2, perimeter_m; walls' length_m, openings; floors' area_m2, volume_m3), from the file at \
         `path` or `text`, held against the project's own: how many were `compared`, those that `differ` (ours and \
         theirs), and those `only_ours` and `only_theirs`, the first 100 of each with their counts",
        compare_crosscheck,
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
        "rename_type",
        "Rename Type",
        &[],
        "{category?: wall|floor|door|window|column|roof|ceiling|railing|…, family?, type, name} renames a type of a category \
         (`category` and `type`) or one of a family's types (`family` and `type`) to `name`, as Rename in Type Properties does: \
         the elements of it, and what else names it, follow, and it keeps its GUID; refused for a name another type of it has. \
         Text types and filled region types rename with `rename` on `text_type` and `filled_region_type`",
        rename_type,
    ),
    edit(
        "purge",
        "Purge Unused",
        &["PU"],
        "{kinds?: [type|family|material|picture|filter|template, …], dry_run?} takes out what nothing uses, again for what that \
         leaves unused: types and families unless `kinds` says; a category keeps a type and a family a type; with `dry_run` it \
         lists what would go and changes nothing",
        purge,
    )
    .menu("Manage > Purge Unused"),
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
    )
    .menu("Manage > Line Weights"),
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
    )
    .menu("Manage > Object Styles"),
    edit(
        "units",
        "Project Units",
        &["UN"],
        "{length?: mm|cm|m|in|ft|ft-in} how drawings, schedules and the desktop show lengths, and read typed ones: \
         millimetres, centimetres, metres, fractional inches, decimal feet, or feet and fractional inches (to 1/16\"); areas \
         in m², or ft² with inches and feet. The model, and every command, keep millimetres",
        units,
    )
    .menu("Manage > Project Units"),
    edit(
        "room_area",
        "Area and Volume Computations",
        &[],
        "{at?: centre|finish} where the project measures its rooms' areas, perimeters and IFC spaces, as Revit's Area and \
         Volume Computations: at the walls' centre lines (`centre`, as a project starts) or at their finish faces \
         (`finish`, the floor inside the walls: the área útil Brazilian codes ask for). Rooms keep their points; only their \
         measured outline changes. IFC spaces carry both areas in Qto_SpaceBaseQuantities whichever is chosen",
        room_area,
    )
    .menu("Manage > Room Area Computation"),
    edit(
        "georeference",
        "Project Location",
        &[],
        "{crs?, eastings?, northings?, height?, rotation?, scale?, description?, geodetic_datum?, vertical_datum?, map_projection?, \
         map_zone?, clear?: true} where the project lies on a map, as IFC's map conversion to a projected coordinate reference \
         system: the CRS by name (an EPSG code, `EPSG:31983`) and what else is known of it, the map point of the project's origin \
         (`eastings`, `northings`, `height`, mm), how far the project's x axis is turned from the map's east (`rotation`, degrees \
         counterclockwise) and a distance on the map over the same in the project (`scale`, 1 as it starts). A project with none \
         takes one with `crs`; what isn't given keeps its value. `clear` takes it away. With nothing, it says it (null for none). \
         IFC import keeps the file's (moved with the model where the import brings it near the origin) and export writes it back as \
         IfcMapConversion to an IfcProjectedCRS in metres; the model's own coordinates and drawings don't move",
        georeference,
    ),
    edit(
        "language",
        "Project Language",
        &[],
        "{code?: en|pt|es|sv|cs|fr|nl} the language the project's drawings speak (a stair's UP, a title block's SCALE, a schedule's \
         headings) and new names come in (levels, sections, elevations, sheets, rooms, schedules): English, Brazilian \
         Portuguese, Spanish, Swedish, Czech, French or Dutch. What's already named stays named",
        language,
    )
    .menu("Manage > Project Language"),
    edit(
        "standard",
        "Technical Standards",
        &[],
        "{set?: nbr|iso|ansi|eu|jis, adapt_all?: bool, adapt_line_weights?: bool, adapt_object_styles?: bool, adapt_units?: bool} configures the project's technical standard (NBR, ISO, ANSI, EU, JIS) \
         and adapts line weights (pens), sheet layouts, object styles, and dimensioning conventions. \
         NBR includes NBR 16861, NBR 16752, NBR 17006, NBR 17067, NBR 17068, and NBR 6492. \
         The numbers each standard sets (margins, folds, pens, per-category pens) came with the contribution that added them and \
         have not been checked against the standards named. Without `set`, reports the current standard and available standards",
        standard,
    )
    .menu("Manage > Technical Standards"),
    other("inspect", "Inspect", "{} levels, element counts, types, warnings and undo history", inspect),
    other(
        "render",
        "Render",
        "{family?, type?, mode?: 3d|plan|elevation, wall_thickness?, view?: a level's, section's or elevation's name or id, a sheet's \
         number, or {3D}, ceiling?, width?, height?, path?, \
         azimuth?, elevation?, eye_elevation?} a family on its own with `family` — its `type` (the first by default) standing in a \
         model of its own, in a wall `wall_thickness` mm thick (200) if it's a door's or a window's, nothing of this project's \
         touched or left behind: `3d` (as seen from the south-east), `plan` (what its symbols draw) or `elevation` (front-on, \
         as seen from the north), a PNG as `pngBase64` or at `path`. Without it: a level's floor plan (with `ceiling`, its \
         reflected ceiling plan, looking up from 2300 mm), a section, an elevation, a sheet (at 5 px a paper mm), or the 3D view seen from \
         `azimuth` degrees round from east (counter-clockwise) and `elevation` degrees up (by default from the south-east, \
         30° up), fitted; in perspective (see `perspective`) through its lens, and with `eye_elevation` from an eye that high \
         (mm) looking at the same point, as a PNG (pngBase64), or written to a path: .png, .svg for a plan, .pdf for a sheet. A plan, section, \
         elevation or sheet that could not draw something as asked (a fill pattern too dense for the view, more ink behind \
         a region than is kept above it) says so in the reply's `notes`",
        render_view,
    ),
    edit(
        "schedule",
        ui_msg!("Schedule"),
        &[],
        "{name?, category?, categories?, fields?, filters?, sort?, grouped?, path?} a schedule view, as Revit's View > Schedules makes one, \
         and its table: columns, rows, the `elements` each row lists, and totals (and a .csv file with a path, or an .xlsx workbook for a path ending .xlsx). A new one needs `name` and `category`: rooms, \
         doors, windows, walls, floors, ceilings, roofs, columns, structural columns, footings, beams, strip footings, components, \
         stairs, ramps, railings or levels, or `multicategory`, as Revit's Multi-Category Schedule: a row for each element of \
         `categories` ([\"walls\", \"doors\"…]; none, every built one but rooms, levels and masses), with the fields they share \
         (Category, Family, Type, Level, Count, Width, Height, Length, Thickness, Area: a wall's length by its height, a door's \
         width by its height; Material: its type's structure layer's, a run's type's, a family's), each empty where its \
         category has none; `category` \
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
         | null, phase_filter?: name | null, options?: {set: option | null, …}, layers?: {drawing: [layer, …] | null, …}, template?: name | null, \
         background_grid?: {visible?, main?: {enabled?, color?: [r,g,b], pen?: 0..16}, sub?: {enabled?, color?, pen?}, \
         spacing?: [X,Y] (mm), divisions?: [X,Y] (1..1000), rotation?: degrees (-360..360)} | null, \
         reset?: true} what a view hides and how it draws the rest, as Revit's Visibility/Graphics does; with \
         none of those, the view's settings as they are. A category hidden (walls, floors, rooms, grids, dimensions, levels…, \
         in any case, singular or plural) leaves all its elements out of that view, and `hide_elements` the elements given. \
         In a 3D view rooms are hidden unless shown (`show: [rooms]`), every new 3D view so; shown, each is a \
         see-through solid. Subcategories are hidden and shown as categories are, and go with their category: under rooms, `roomreference` (the \
         cross at each room's point, hidden unless shown, as Revit's Rooms > Reference) and `roomseparator` (room separation \
         lines). An \
         override is {hidden?, projection?: style, cut?: style, halftone?, transparency?}, a style {color?, weight? (a pen, \
         0 to 16: 0 a hairline, the thinnest line the screen or printer draws at any scale; 1 to 16 widths on paper from 0.10 to \
         4 mm, 4 being 0.18 mm and 10 0.35 mm, a cut wall's outline), dash? (solid, dashed, dotted), fill?, foreground?, background?}, a colour [r, g, b] or \"#rrggbb\". \
         Foreground and background override independent pattern layers: {pattern: a fill pattern's name | null, color?: [r,g,b]}; \
         a null pattern hides that layer; a null foreground or background restores inheritance. They draw in plans, ceiling \
         plans, section cuts and filled regions, using drafting patterns at paper scale and model patterns at model size. \
         Projection is what a plan sees below its cut plane, a section beyond its line and the annotation; cut is what the view \
         cuts; `fill` is the cut's poché, or a 3D view's surface; in a 3D view, and in what a section sees beyond its line, a \
         projection's `color` is the colour of the edges (weight and dash draw lines, and those are vectors). Halftone draws it \
         halfway to the paper; transparency (percent) sees through it in a 3D view. Halftone and transparency only add: a \
         filter or an element can lighten what its category doesn't, not take its category's away. Overrides merge into the last (null takes a field or the override away); an element's own \
         beats the filters applied (the first prevailing), which beat the category's. `filters` replaces the list applied; \
         `template` follows a view template; `reset` takes away all of the view's own settings. `phase` is the phase the view \
         shows (the project's last unless it says) and `phase_filter` how it shows each status of what's in it (see `phase`); \
         `options` chooses the design option it shows of a set (the set's primary unless it says; see `option`); `layers` the \
         layers of each CAD drawing it hides, by the drawing's name (null shows them all again)",
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
        "perspective",
        "Perspective",
        &[],
        "{view?: a 3D view's name or id ({3D} unless given), on?: true | false (true unless given), lens?: mm} a 3D view seen \
         in perspective, as through a camera, or with `on: false` parallel (axonometric) again, as the ViewCube's Toggle to \
         Perspective and Toggle to Parallel do. `lens` is the focal length it is seen through, 10 to 600 mm on a 35 mm camera \
         (whose 36 mm of film is the picture's width): 35 unless given or set before; turned off, it keeps it. The eye stands \
         back from the middle of the picture as far as the lens and the zoom say; `render` takes how high it stands",
        perspective,
    ),
    edit(
        "graphic_display_options",
        "Graphic Display Options",
        &["GD"],
        "{view?: a 3D view's name or id ({3D} unless given), edges?: true | false, silhouette?: px, ambient_shadows?: true | \
         false} how a 3D view draws its edges and shadows, as Revit's Graphic Display Options: `edges`, the lines where its \
         surfaces meet in Shaded (on unless turned off; Hidden Line always draws them); `silhouette`, an outline that many \
         pixels wide (a whole number, 0 to 10; 0, the default, for none) round what stands in front of what is further off \
         or of nothing; `ambient_shadows`, soft shadows where faces meet inside a corner (off unless turned on). What is left \
         out stays as it was; with none of them it only says what the view has",
        graphic_display_options,
    ),
    edit(
        "camera",
        ui_msg!("Camera"),
        &[],
        "{eye?: [x, y, z], target?: [x, y, z], lens?: mm, view?: a 3D view's name or id, name?} a 3D camera as Revit's \
         Camera tool makes one: with `eye` (where the camera stands, mm) and `target` (what it looks at, mm) it makes a new \
         named perspective 3D view looking from `eye` at `target` through `lens` (35 mm unless given, 10 to 600), named \
         `name` or by number (\"Camera 1\", \"Camera 2\"…). With `view` too, it gives that 3D view the camera instead (its \
         id stays; `{3D}` may be given one): a point left out stays as that camera has it, so `lens` alone or one point \
         alone changes just that, and a view without a camera yet needs both. With `view` and none of `eye`, `target` or \
         `lens` it reads the camera back: `eye`, `target`, `lens` and whether the view is in perspective (null eye/target \
         for a view with no camera yet). The eye is never on the target, so the view always has a direction to look in",
        camera,
    ),
    edit(
        "work_plane",
        "Work Plane",
        &[],
        "{view?: a 3D view's name or id ({3D} unless given), level?: a level's name or id, points?: [[x, y, z], [x, y, z], \
         [x, y, z]], surface?: {point: [x, y, z], normal: [x, y, z]}, offset?: mm} what a 3D view sketches on, as Revit's \
         Work Plane > Set: a level; the plane through three `points` (not along a line), facing up unless upright; or a \
         surface of the model, a point of it and the way it faces (a wall's face, picked). Any of them may be moved `offset` mm \
         along its normal (up, for a level; 0 unless given), and may lie at any angle, upright too: what a tool may sketch on \
         it is up to the tool (a floor, a ceiling, a roof and a shaft need a level plane, and stand on the highest level at or \
         below it). Given only `offset`, the plane as it is is moved by that instead. Given nothing, it's the lowest level \
         again, as it is until set. A level deleted takes its work plane with it. Says the plane: a point of it (`origin`), its \
         `normal`, its height at the origin where it isn't upright (`z`) and its `slope` in degrees from level",
        work_plane,
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
        "{name, from?: a level's, section's, elevation's or 3D view's name or id, like?: another template's name, \
         kind?: plan | section | 3d | null, rename?, controls?: [categories, filters, range, \
         phase, options, scale, detail, style, background_grid], view_range?: {top?, cut?, bottom?, depth?}, phase?, phase_filter?, options?, \
         scale?: 1 | … | 100000, detail?: coarse | medium | fine, visual_style?: shaded | hidden_line | wireframe, \
         background_grid?: grid settings as in graphics, hide?, show?, categories?, filters?, reset?, delete?: true} a view template, as Revit's: settings under a name for \
         views to follow with `graphics {template}`. `kind` is the views it is for (plans, elevations and sections, or 3D \
         views; null, any), which `from` sets; `like` makes a new one a copy of another (Duplicate); `rename` names it anew, \
         and its views follow it under that name. `from` takes a view's categories (hidden and overridden), filters and, a \
         plan's, view range, its phase and phase filter, its design options, its scale, detail level and visual style; \
         `controls` is what views following it get from it (all nine unless it says) and cannot change for themselves: its \
         categories, its filters, a plan's view range, its phase with its phase filter, its design options, its scale (`scale` \
         command), its detail level (`detail`), its visual style (`visual_style`). `hide`, `show`, `categories`, `filters`, \
         `phase`, `phase_filter`, `options`, `scale`, `detail`, `visual_style`, `background_grid` and `reset` change it as they change a view; \
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
    )
    .menu("Manage > Phasing"),
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
    )
    .menu("Manage > Phasing"),
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
    )
    .menu("Manage > Design Options"),
    Command {
        id: "measure",
        label: ui_msg!("Measure"),
        aliases: &["ME"],
        params: "{from: [x, y], to: [x, y]} what a ruler laid between two points reads (mm): its `length`, its parts `dx` and `dy`, \
                 and its `angle` in degrees from east, counter-clockwise; it changes nothing",
        run: measure,
        undoable: false,
        menu: None,
    },
    other(
        "circuit",
        "Size a Circuit",
        "{load, apparent?, demand?, power_factor?, phases?: FN|FF|3F|3FN, system?: 127/220|220/380|230/400, use?: lighting|power, length?, \
         method?, insulation?: pvc|epr, conductor?: flexible|rigid, ambient?, grouped?, bundle?, spacing?, soil?, section?, breaker?, \
         drop_limit?, reduced_neutral?} one electrical circuit sized as NBR 5410:2004 asks, for copper: `load` in W (or VA with \
         `apparent`), `demand` (0 to 1, 1) and `power_factor` (over 0 to 1, 1; a first value, not a claim about the load), `phases` \
         (FN one phase and neutral, the default; FF; 3F; 3FN balanced), the voltage `system` (127/220), `use` (power: 2.5 mm² at least; \
         lighting 1.5), `length` to the farthest load (mm; none: no drop is checked), the installation `method` of Tabela 33 (7: a \
         conduit in masonry; also 1 to 8, 11, 11A, 61, 61A), `insulation` (pvc 70 °C, or epr 90 °C), `conductor` (flexible, the larger \
         resistance; or rigid), `ambient` °C (30 in air, 20 in soil), `grouped` circuits in the conduit (this one included), `bundle` \
         (method 11 cables bundled), `spacing` between buried conduits (mm) and `soil` resistivity (K·m/W), and a `section` (mm²) and \
         `breaker` (A) already chosen, which are never made smaller. Replies with the design current, the section and breaker that give \
         I_B ≤ I_n ≤ I_Z (which gives I_2 = 1.45 I_n ≤ 1.45 I_Z for the breakers of NBR NM 60898) with the drop within `drop_limit` (4 %), the current the section carries, the drop, \
         the conductors (phases, neutral, protective), the factors, and what the chosen section and breaker give. It changes nothing; \
         aluminium, trays, several layers, short circuit and harmonics are not checked. It has no shortcut: Revit has no tool for it",
        circuit,
    ),
    other(
        "conduit_fill",
        "Conduit Fill",
        "{section, phases?: FN|FF|3F|3FN, circuits?, reduced_neutral?, family?, size?} the fill of a conduit as NBR 5410 asks, for the \
         conductors of a circuit (its phases, neutral and protective conductor, of `section` mm²) times `circuits` in the same conduit \
         (1): their cross-sections by outside diameter over the conduit's inside, at most 53 % for one cable, 31 % for two, 40 % for \
         three or more. `family` (pvc-thread, pvc-solder, steel-heavy, steel-rir; pvc-thread) and `size` (its mark, e.g. 3/4\"; none: \
         the smallest that fits). The dimensions are the makers' own from their catalogues, each family naming maker, document, page \
         and address; the inside diameter is the outside less twice the wall. The data covers the flexible 750 V PVC cable of 0.5 to \
         240 mm² only: there is no `insulation` key, and a section above 240 mm² is `pending`, never guessed. `sections_mm2` lists the \
         conductors of one circuit (`circuits` of them are in the conduit). Cable trays are not sized. It has no shortcut: Revit has no \
         tool for it. It changes nothing",
        conduit_fill,
    ),
    other(
        "drain_size",
        "Size a Sanitary Drain",
        "{units?, fixtures?: {wc?, lav?, sink?, shower?, laundry?}, role?: branch|main|stack, slope?, floors?, min_dn?} the DN of a \
         sanitary drain as NBR 8160 asks, by the fixture units (UHC) it carries: `units`, or `fixtures` counted (Tabela 3: a water \
         closet 6 UHC and DN 100, a lavatory 1 and 40, a sink 3 and 50, a shower 2 and 40, a laundry tub 3 and 40; the drain is never \
         smaller than the fixtures' own). `role`: a `branch` (ramal de esgoto, Tabela 5, the default), a `main` (subcoletor or coletor \
         predial, Tabela 7 by `slope`, a fraction, 0.01 by default; DN 100 at least) or a `stack` (tubo de queda, Tabela 6 by the \
         building's `floors`, up to three or more). `min_dn` is the largest drain flowing into it. Replies with the `dn`, what it \
         carries (`capacity`) and the `minimum_dn` the fixtures ask. A blank in the standard's table means not allowed: DN 100 and 150 \
         carry nothing in a building drain at 0.5 %, and a branch or a building drain never borrows the other's table. For a `main` \
         every fixture given counts, where item 5.1.4.2 counts only the largest fixture of each bathroom in a residential building: pass `units` already \
         reduced to follow it. It has no shortcut: Revit has no tool for it. It changes nothing",
        drain_size,
    ),
    other(
        "vent_size",
        "Size a Sanitary Vent",
        "{units, wc?, stack_dn?, length?, trap_dn?} the DN of a sanitary vent as NBR 8160 asks, by the units (UHC) of what it vents: a \
         branch vent (ramal de ventilação, Tabela 8; `wc` says a water closet is in the group), or, with `stack_dn` (the DN of the \
         stack or drain served) and `length` (mm, from its foot to its open end), a vent stack or header (Tabela 2). `trap_dn` adds the \
         longest run from a trap of that DN to its vent (Tabela 1). It has no shortcut: Revit has no tool for it. It changes nothing",
        vent_size,
    ),
    other(
        "hydrant_size",
        "Size Hydrants",
        "{column?: 1-5, area?, type?: 1|2|3|4-40|4-65|5, hydrants?, inside?, nominal?, material?: steel|steel-wet|copper|pvc|pex, length?, rise?} \
         hydrants and hose reels as IT 22/2025 of the CBPMESP asks. With `column` (of Tabela 3, by the occupation) and `area` (m², \
         built) it replies the system type and the fire reserve (m³). With `type` (Tabela 2; 4 comes as 4-40 or 4-65 by its nozzle) \
         it replies the design flow, that of the two most unfavourable jets (`hydrants`, 2 by default), and the head at the nozzle; \
         with `inside` (mm) it also replies the speed (5 m/s at most), the Hazen-Williams head loss over `length` (m) of the \
         `material` (Tabela 1: `steel` is black steel in a dry system, C 100, the lowest steel row and the default, `steel-wet` is galvanised or wet, C 120), the head needed at the start of the pipe with its `rise` (mm, up) and, with `nominal` (mm), whether \
         the pipe is big enough (DN 65; DN 50 for types 1 and 2). `length` and `rise` are in millimetres. `need_mca` is the nozzle \
         head + the head loss of the straight pipe + the rise: it leaves out the hose, the fittings and the pressure at the source \
         (`need_covers` says so), and `ok` is only the nozzle head within 100 mca, the speed and the pipe's size (`ok_covers`), not \
         a verdict on the design. A `type` that contradicts `column` and `area` is an error. Notes 1 and 2 of Tabela 3 (a system one type lower in some cases) are not applied. `region` is SP, the only one: this is the \
         São Paulo rule, and no other state's. It has no shortcut: Revit has no tool for it. It changes nothing",
        hydrant_size,
    ),
    other(
        "load_points",
        "Residential Load Points",
        "{rooms?: [{name, area, perimeter, kind?: bath|kitchen|service|dry|veranda|other, points?}], alternative?} the lighting and the \
         general-use outlets (TUG) of a dwelling's rooms as NBR 5410:2004 item 9.5.2 asks. The rooms are `rooms` (`area` in m², \
         `perimeter` in millimetres, `points` the outlets the room has, the least it needs when absent) or, without it, the rooms of the \
         model, measured inside the walls' finish. A room's kind is known by its name (Portuguese or English: banheiro, cozinha, copa, \
         área de serviço, lavanderia, sala, dormitório, varanda…) unless `kind` gives it; a name that says none is `other`. Lighting: 1 \
         ceiling point of 100 VA up to 6 m² and 60 VA more for each whole 4 m² above. Outlets: a bathroom 1; kitchen, pantry, service \
         area, laundry one for each 3.5 m of perimeter; room, bedroom one for each 5 m; veranda 1; the others 1 up to 6 m² and one \
         for each 5 m above. Power: in a bathroom, kitchen, pantry, service area or laundry 600 VA for each of the first 3 points and \
         100 VA for the others; elsewhere 100 VA a point; on the points a room has. Where those rooms have more than 6 points \
         together `alternative: true` takes 600 VA for the first 2 of each (`alternative_possible` says when). A kitchen or pantry \
         also needs 2 outlets over the sink's counter (`counter_outlets`). Not applied: the exceptions to the lighting point and to \
         a veranda's outlet, the placement of the outlets and the TUE (`not_covered`). It has no shortcut: Revit has no tool for it. \
         It changes nothing",
        load_points,
    ),
    other(
        "dr_check",
        "Check Residual-Current Protection",
        "{circuits: [{name, points: [{place, kind, rating?, height?, outdoor?}], residual?, it_continuity?}], building?: residential|other} \
         which circuits NBR 5410:2004 asks additional protection by a high-sensitivity residual-current device (DR, 30 mA at most) \
         for. A circuit's `points` are what it serves: `place` is bath (a place with a bathtub or a shower), external, kitchen (or \
         pantry-kitchen), laundry, service (service area), garage, wet (another wet internal area) or dry; `kind` is outlet, lighting or \
         fixed (another point of use); `rating` an outlet's rated current in A (32 A at most is covered; unknown, it is taken as covered), \
         `height` a light's in mm (in a dwelling a light 2500 mm up or more in the wet rooms is left out), `outdoor` an internal outlet \
         that may feed equipment outdoors. It is needed where any point is in a bath place, an outlet is in an external area or may \
         feed equipment outdoors, and, in a dwelling (the default `building`), any point is in a kitchen, laundry, service area, garage \
         or wet internal area (outside a dwelling, only their outlets). `it_continuity` marks an IT circuit that must not be cut, for \
         the safety of persons or to keep lives, which is left out (`exempt`). `residual` is the rated residual current of the device \
         the circuit has, in mA: `ok` is whether it is 30 or less, and `missing` names the circuits that need one and have none. \
         Not covered: the further rules for the volumes of a bathroom and for swimming pools (`not_covered`). It has no shortcut: \
         Revit has no tool for it. It changes nothing",
        dr_check,
    ),
    other(
        "dps_check",
        "Check Surge Protection",
        "{region: AQ1|AQ2|AQ3, overhead?, accepted_risk?, risk_to_people?, purpose?: line|direct|both, earthing?: TT|TN-S|TN-C|IT, \
         uo?, before_dr?, system?: 120/208|127/220|220/380|230/400|277/480|400/690, neutral?, phases?: 1|3, existing?, signal_lines?: [telephone|data|video|tv|antenna|interconnection]} surge protection (DPS) of \
         a power line as NBR 5410:2004 asks. It is required where the line is `overhead` (wholly or partly, or the installation has an \
         overhead line itself) and the `region` is AQ2 (more than 25 days of thunderstorms a year), or in any AQ3 region (5.4.2.1.1); \
         `accepted_risk` is the project taking a calculated risk the standard allows, never where the omission may put people at risk \
         (`risk_to_people`), and `exempt` says it was taken. When it is required the reply gives where it stands by its `purpose` \
         (line, the default: surges the line carries and switching surges, at the entry or the main board; direct: lightning on the \
         building, at the entry; both; `existing` adds the meter box of an existing individual unit), with `earthing` (and whether the \
         line has a `neutral`, yes by default) the connection scheme or schemes of Figure 13, with `phases` (3 by default) the least \
         nominal discharge current (5 kA for each mode; 20 kA three-phase or 10 kA single-phase between the neutral and the PE of \
         scheme 3) and impulse current (12.5 kA; 50 or 25 kA there), and the connecting conductors (4 mm² copper at least, 16 mm² \
         against direct lightning, 0.5 m in all). With `earthing`, `uc_min` is the least continuous operating voltage Uc of Tabela 49 for \
         each pair of points the device connects (1.1 Uo; Uo between the neutral and the PE; √3 Uo phase to PE in IT; U in IT without a \
         neutral), in volts when `uo`, the phase-to-neutral voltage, is given. `signal_lines` are the external signal lines, each protected at the entry. Not \
         covered: the overcurrent device of the DPS (6.3.5.2.5) and coordination (`not_covered`). `before_dr` is for a DPS upstream of \
         a DR, which in TT takes scheme 3 only (6.3.5.2.6 b); with `earthing` the reply also gives the follow current between neutral \
         and PE and the short-circuit the DPS must withstand (6.3.5.2.4 e). It has no shortcut: Revit has no tool for it. It changes nothing",
        dps_check,
    ),
    other(
        "demand",
        "Demand and Service Category",
        "{utility: CPFL|ENEL, building?: residential|other, system?: 127/220|220/380, area?, outlets?, lighting_points?, \
         lighting_outlets?: {use, kw, pf?}, heaters?, boilers?, appliances?, stoves?: [{w, count?}], air_conditioners?: [{btu | va and w, \
         count?}], air_use?: residential|commercial, motors?: [{cv, phases: 1|3 | kva and kw, count?}], special?: [{kind: arc_weld|\
         resistance_weld|xray, kw, count?}], hydromassage?: [{w, count?}]} the demand D (kVA) of an installation and the category of \
         service it asks of the `utility`, by that utility's own standard, since NBR 5410 gives no demand factors. `utility` is \
         required: CPFL (GED 13 version 2.24, 18/09/2020: CPFL Paulista, Piratininga, Santa Cruz and RGE) or ENEL (ET 1569 version 01, \
         29/01/2024); any other is refused until its tables are added. ENEL does not work out a demand, for its specification leaves it \
         to the designer (item 7.7.2): `{utility: ENEL, network: aerial|aerial_120_240|underground|underground_submersible, installed_kw, \
         phases?: 1|2|3, demand_kva?}` frames the supply by the installed load in kW: Anexo A (aerial, 127/220 V), C (aerial, 120/240 V), \
         D (underground, surface, 127/220 V) or E (underground, submersible): the category, the power it makes available (E prints none and \
         gives the junction box), the breaker, the connection and entry branches, the earthing conductor and the measurement; where two \
         categories hold the load (B2 and C1 of A) `candidates` names both and `phases` picks one. `network: aerial_delta` (Anexo B, 120/240 V \
         delta with a neutral) goes by `demand_current_a`, the designer's demand current in A, and gives the smallest breaker that holds it. \
         `demand_kva` is carried as the designer gives it. For CPFL: `D = a + b + c + d + e + f + g + h + i`: a lighting and outlets (in a dwelling, `area` m² \
         gives the least outlets of Tabela 2, or `outlets` above 250 m² (100 W each, and the kitchen's 3 of 600 W), and `lighting_points` of \
         100 W; in another building `lighting_outlets` declares its `use` (auditorium, shop, barbershop, club, school, office, garage, hospital, \
         hotel, church, industry, restaurant), kW and power factor, by Tabela 18), b `heaters` (showers, taps, instant heaters, irons: Tabela 4 \
         by their number in all), c `boilers` (Tabela 5), d `appliances` (dryers, ovens, dishwashers, microwaves: Tabela 6), e `stoves` \
         (Tabela 7), f `air_conditioners` (a `btu` of Tabela 8, or `va` and `w`; a factor of 1, or Tabela 9 for `air_use` commercial), g \
         `motors` (a `cv` of Tabela 14 or 15 with its `phases`, or `kva` and `kw`; Tabela 10 by rank, the biggest first), h `special` \
         equipment (Tabela 11, power factor 0.75), i `hydromassage` (Tabela 10). `w` is the power of one appliance in watts. It replies the \
         installed load (kW), each term, D, and the `category` of Tabela 1A (`system` 127/220, the default) or 1B (220/380) with its breaker, \
         cable, conduit and earthing conductor: by the installed load up to 25 kW, by D above, `null` where the table ends (75 kW; D of 76 \
         or 82 kVA). Not covered: the other columns of Tabela 1, the cables of Tabelas 1D and 1E, and the starting of motors (Tabela 13) \
         (`not_covered`). It has no shortcut: Revit has no tool for it. It changes nothing",
        demand,
    ),
    other(
        "water_size",
        "Size Cold Water Pipes",
        "{fixtures?: {wc, lav, sink, shower, laundry: count}, flow?, method?: weights|sum, dn?, length?, fittings?: {elbow90, elbow45, \
         tee_through, tee_side, gate, globe, check: count}, pressure?, rise?} a cold water pipe of PVC soldável as NBR 5626 asks. The \
         flow is `flow` (L/s), or the design flow of the `fixtures` by the `method` the project chooses: weights (the empirical \
         method, Q = 0.3 √ΣP, never more than the fixtures' own flows; the default) or sum (every fixture at once). The fixture \
         flows and weights are Tigre's (Manual Técnico, page 198), not the standard's; `wc` is the WC with a flush tank only (a flush-valve WC \
         is not in the table here). Without `dn` it replies the smallest DN chosen by speed alone (3 m/s at most; the pressure is \
         not used to choose it); with `dn`, whether that pipe keeps the speed. The head loss is Darcy-Weisbach (Colebrook, PVC at \
         20 °C) over `length` (mm) plus the equivalent length of the `fittings`; with `pressure` (kPa, dynamic, upstream) and `rise` \
         (mm, up) it replies the pressure left at the end, 10 kPa at least at a point of use, and `ok` covers both; without `pressure` \
         `ok` is the speed alone (`ok_covers` says which). It has no shortcut: Revit has no tool for it. It changes nothing",
        water_size,
    ),
    other(
        "reservoir_size",
        "Size Water Reservation",
        "{daily_l? | people?, per_person_l?, days?, fire_l?, house?, lower_percent?, compartments?, fill_flow?, pumps?, towers?, \
         smallest_l?, peak_l?} the reservation of a building as NBR 5626:2020 asks. The day's consumption is `daily_l` (litres), or \
         `people` times `per_person_l`: the standard gives no litres per person, so it is the project's own assumption and the reply \
         says so. The potable volume is `days` of it (1 by default, 24 h being the least; 3 at most unless the project shows how the \
         quality is kept) and `fire_l` is kept apart and added (the project's and the fire brigade's, not the standard's). The reply \
         has the refill flow (the day's consumption in 6 h, or 3 h with `house`, a single-family house) and checks each of one kind \
         (current requirement, historical reference, engineering practice, configurable assumption): `fill_flow` (L/s) against that \
         flow, `compartments` of an elevated reservoir (2 at least, a house excepted), `smallest_l` against `peak_l` (the greatest peak \
         over a normal maintenance), `pumps` (2 independent at least) and, with `towers` of 2 or more, the spare pump. A check whose \
         input is not given is `pending` and never stops the others; `ok` is that none failed. `lower_percent` splits the potable \
         volume between the lower and the upper reservoir, a choice of the project's; the usual split is only a suggestion. It \
         leaves out the pump's head and the recalque pipe. It changes nothing",
        reservoir_size,
    ),
    other(
        "storm_size",
        "Size a Storm Drain",
        "{kind: downpipe|horizontal, area?, rainfall?, flow?, outlet?: edge|funnel, gutter?, length?, slope?, inside?} a storm drain as \
         NBR 10844 asks. The flow is `flow` (L/min), or the rain on `area` (m², as the standard counts a roof) at `rainfall` (mm/h; \
         150 by default, which the standard allows for up to 100 m² only: above that `rainfall` is required): Q = I·A/60. A `downpipe` (condutor vertical) takes the inner diameter of the \
         standard's Figure 3 abacuses by Q, the water depth in the gutter `gutter` (50 to 100 mm), its `length` (mm, 0.3 m or more) \
         and the gutter's `outlet` (edge: sharp-edged, the default; funnel); 70 mm inside at least. A `horizontal` drain takes \
         Manning at two thirds full (n 0.011, PVC only, from the standard's Table 2) at its `slope` (a fraction, 0.005 at least): the inner \
         diameter it needs, and with `inside` (mm) whether that pipe carries the flow. Off the abacuses (more than 150 mm inside) \
         nothing is read. A key that does not apply to the `kind`, or `flow` together with `area`, is an error. It has no shortcut: \
         Revit has no tool for it. It changes nothing",
        storm_size,
    ),
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
    )
    .menu("Manage > Select by ID"),
    other(
        "ids_of",
        "IDs of Selection",
        "{ids: [id or GUID, …]} what names each element: its `id`, its `guid`, its `ifc` GlobalId, its `category` and its \
         `name` (or type, number, mark)",
        ids_of,
    )
    .menu("Manage > IDs of Selection"),
    other(
        "filter_selection",
        "Filter",
        "{ids: [id or GUID, …], by?, keep?: [value, …]} Revit's Filter on a selection: `groups` [{value, count, ids}, …], \
         the values `by` takes among the elements (by default `Category`, whose values are categories' keys: wall, door, \
         detail_line…; else a parameter as `fields` lists them, values as schedules show them, `\"\"` for none), and \
         `fields`, the parameters every one of them has, to filter by. With `keep`, `ids`: those holding one of its values, \
         the selection to keep; it selects nothing itself, a front end does",
        filter_selection,
    ),
    other(
        "elements",
        ui_msg!("Elements"),
        "{category?, level?, limit?, offset?} elements with their `id` (six digits, what commands take) and `guid` (made once, \
         never changed), fields and derived values (length, height, area_m2…); what an element points at (its level, host, \
         view…) it holds by GUID. Wherever a command takes an element, its id or its GUID will do",
        elements,
    ),
];

// ---------- parameters ----------

/// The longest an error message is, in characters: a message that repeats what it was given (a list of a hundred thousand ids)
/// is cut, and says by how much.
const MAX_MESSAGE: usize = 2000;

fn bad(cmd: &str, msg: impl std::fmt::Display) -> String {
    let text = format!("{cmd}: {msg}");
    match text.char_indices().nth(MAX_MESSAGE) {
        Some((cut, _)) => format!("{}… ({} more characters)", &text[..cut], text[cut..].chars().count()),
        None => text,
    }
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

/// `[x, y, z]` in mm, finite and within the model's limits.
fn point3(v: &Value) -> Option<[f64; 3]> {
    let [x, y, z] = v.as_array()?.as_slice() else { return None };
    let p = [x.as_f64()?, y.as_f64()?, z.as_f64()?];
    p.iter().all(|c| c.is_finite() && c.abs() <= MAX_COORD).then_some(p)
}

fn points3(c: &str, p: &Value, key: &str) -> Result<Vec<[f64; 3]>, String> {
    let a = p.get(key).and_then(Value::as_array).ok_or_else(|| bad(c, format!("`{key}` [[x, y, z], …] is required")))?;
    if a.len() > MAX_POINTS {
        return Err(bad(c, format!("`{key}` has more than {MAX_POINTS} points")));
    }
    a.iter().map(|v| point3(v).ok_or_else(|| bad(c, format!("`{key}`: {v} is not a point [x, y, z] in mm")))).collect()
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

/// How a door or a window opens, by its name; blank, not said.
fn operation(c: &str, name: &str) -> Result<Option<crate::model::Operation>, String> {
    if name.trim().is_empty() {
        return Ok(None);
    }
    crate::model::Operation::parse(name).map(Some).ok_or_else(|| {
        let names: Vec<&str> = crate::model::Operation::ALL.iter().map(|o| o.name()).collect();
        bad(c, format!("`operation` is one of {}, not `{name}`", names.join(", ")))
    })
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
        Some(Value::String(n)) if Guid::parse(n).is_none() => {
            let sheet = || m.sheets().into_iter().find(|id| m.sheet(*id).is_some_and(|sh| sh.number == *n));
            // The first by id, a view or (with `sheets`) a sheet.
            let found = match (m.view_named(n), if sheets { sheet() } else { None }) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            found.ok_or_else(|| bad(c, format!("no view named `{n}`")))
        }
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
    // `mass` and `switch` end in the plural's letter, and an accessory's plural isn't made by it.
    match s.as_str() {
        "mass" | "masses" => "mass".into(),
        "switch" | "switches" => "switch".into(),
        "pipeaccessory" | "pipeaccessories" => "pipeaccessory".into(),
        _ => s.trim_end_matches('s').to_string(),
    }
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

/// The nearest wall at `at` on any level, with the heights it spans: what a level that does not cut it should be told about.
fn wall_elsewhere(m: &Model, at: P2) -> Option<(Id, f64, f64)> {
    m.elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Wall(w) => Some((id, w, off_centre(m, w, at))),
            _ => None,
        })
        .filter(|(_, w, d)| *d <= m.thickness(w) / 2.0 + 100.0)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(id, w, _)| (*id, m.base_z(w), m.top_z(w)))
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
    let mut pieces: Vec<Vec<P2>> = geom::faces(&m.wall_lines(level), TOL).into_iter().filter(|f| geom::area(f) < -TOL * TOL).collect();
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

/// The next in the series `taken` is numbered in: one past its largest number, "1" when there is none.
/// A series is plain numbers ("1", "2") or numbers after a prefix ("P1", "P2", "J-01"); a prefix
/// counts as a series once two marks share it and they outnumber the plain numbers, so a lone "D2"
/// among numbered doors stays an exception. Zero padding is kept ("P09" → "P10"), and the result
/// is never one already taken. A mark longer than a name may be (`MAX_TEXT`) is no part of a series.
pub(crate) fn next_number<'a>(taken: impl Iterator<Item = &'a str>) -> String {
    let taken: Vec<&str> = taken.map(str::trim).collect();
    // Each series by its prefix: how many marks it has, its largest number and the width its zero-padded ones keep.
    let mut series: BTreeMap<&str, (usize, u64, usize)> = BTreeMap::new();
    for t in taken.iter().filter(|t| t.len() <= MAX_TEXT) {
        let digits = t.len() - t.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let (prefix, number) = t.split_at(t.len() - digits);
        let Ok(n) = number.parse::<u64>() else { continue };
        let s = series.entry(prefix).or_insert((0, 0, 0));
        s.0 += 1;
        s.1 = s.1.max(n);
        if number.len() > 1 && number.starts_with('0') {
            s.2 = s.2.max(digits);
        }
    }
    let plain = series.get("").map_or(0, |s| s.0);
    let (prefix, (_, last, width)) = series
        .iter()
        .filter(|(p, s)| p.is_empty() || (s.0 >= 2 && s.0 > plain))
        .max_by_key(|(p, s)| (s.0, s.1, std::cmp::Reverse(**p)))
        .map_or(("", (0, 0, 0)), |(p, s)| (*p, *s));
    // A mark longer than a name may be starts no series, so no width passes MAX_TEXT; this keeps it so.
    let width = if prefix.is_empty() { 0 } else { width.min(MAX_TEXT) };
    let mut n = last;
    loop {
        n = n.saturating_add(1);
        let next = format!("{prefix}{n:0width$}");
        if !taken.contains(&next.as_str()) || n == u64::MAX {
            return next;
        }
    }
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
    element_json_with(m, id, &crate::rooms::Rooms::new(m))
}

/// [`element_json`], a room's boundary found among `rooms`: what one look at many elements shares.
pub fn element_json_with(m: &Model, id: Id, rooms: &crate::rooms::Rooms) -> Value {
    let Some(e) = m.elements.get(id) else { return Value::Null };
    let mut extra = Map::new();
    extra.insert("id".into(), json!(id));
    extra.insert("guid".into(), json!(m.guid(id)));
    // Room Bounding, which a file leaves out while it is on.
    if let Element::Wall(Wall { room_bounding, .. })
    | Element::Column(Column { room_bounding, .. })
    | Element::StructuralColumn(Column { room_bounding, .. }) = e
    {
        extra.insert("room_bounding".into(), json!(room_bounding));
    }
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
        Element::CadImport(ci) => {
            if let Some(cad) = m.cads.get(&ci.cad) {
                extra.insert("source".into(), json!(cad.source()));
                extra.insert("entities".into(), json!(cad.dxf().entities.len()));
                extra.insert("layers".into(), json!(cad.dxf().layers.len()));
                extra.insert("unit".into(), json!(cad.dxf().unit));
                if let Some(path) = cad.path() {
                    extra.insert("linked".into(), json!(path));
                }
            }
            if let Some(b) = m.cad_box(id) {
                extra.insert("extents".into(), json!(b));
            }
        }
        Element::RoomSeparator(rs) => {
            extra.insert("length".into(), json!(hundredth(rs.start.dist(rs.end))));
        }
        Element::Rebar(rb) => {
            let length = rebar::length(&rb.path);
            extra.insert("length".into(), json!(hundredth(length)));
            extra.insert("weight_kg".into(), json!((rebar::weight_kg(rb.diameter, length) * 1000.0).round() / 1000.0));
        }
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
                if let Some(op) = t.operation {
                    extra.insert("operation".into(), json!(op.name()));
                }
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
            extra.insert("labels".into(), json!(m.dimension_labels(d)));
        }
        // Every parameter's value, worked out, and how high it reaches over its base.
        Element::Component(cp) => {
            if let Some(v) = m.families.get(&cp.family).and_then(|f| f.values(&cp.type_name, &cp.params).ok()) {
                extra.insert("values".into(), values_json(v));
            }
            // What it draws, as its family's connectors say, for its type: none for a family with none.
            if let Some(cs) = m.families.get(&cp.family).and_then(|f| f.connector_values(&cp.type_name, &cp.params).ok())
                && !cs.is_empty()
            {
                extra.insert("connectors".into(), Value::Array(cs.iter().map(connector_json).collect()));
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
            extra.insert("text".into(), json!(m.tag_text_with(t, rooms)));
            extra.insert("level".into(), json!(m.level_of(e)));
            extra.insert("at".into(), json!(m.anchor(t.host).map(|a| a + t.offset)));
        }
        // Areas less the openings and shafts through them.
        Element::Floor(f) => {
            let left: f64 = m.floor_slabs(f).iter().map(|(face, rise)| area(&m.less_shafts(face, *rise).0)).sum();
            extra.insert("area_m2".into(), json!(m2(left)));
            extra.insert("thickness".into(), json!(m.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness)));
            // What it holds, less its openings and the shafts through it: under a variable layer, less where its top falls.
            extra.insert("volume_m3".into(), json!((m.floor_volume(f) / 1e6).round() / 1e3));
            // How high its top is, at its lowest and its highest, its shape's points too.
            extra.insert("top".into(), top_range(m, f));
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
            let bound = rooms.bound(r, m.room_area);
            if let Err(Unbound::Shared(other)) = bound {
                extra.insert("shares_space_with".into(), json!(other));
            }
            let region = bound.ok();
            extra.insert("enclosed".into(), json!(region.is_some()));
            extra.insert("area_m2".into(), json!(region.as_ref().map_or(0.0, |g| m2(g.area()))));
            extra.insert("perimeter_m".into(), json!(region.as_ref().map_or(0.0, |g| (g.perimeter() / 10.0).round() / 100.0)));
            let height = m.room_height(r);
            extra.insert("unbounded_height".into(), json!(height));
            extra.insert("volume_m3".into(), json!(region.as_ref().map_or(0.0, |g| (g.area() * height / 1e7).round() / 100.0)));
        }
        Element::Text(n) => {
            extra.insert("level".into(), json!(m.level_of(e)));
            extra.insert("size".into(), json!(m.text_types.get(&n.type_name).map(|t| t.size)));
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
        Element::Spot(sp) if sp.slope => {
            let read = m.spot_slope(sp);
            extra.insert("slope".into(), json!(read.map(|(f, _)| hundredth(f))));
            extra.insert("toward".into(), json!(read.and_then(|(_, d)| d).map(|d| [hundredth(d.0), hundredth(d.1)])));
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
        // Its Computation Height, which its JSON leaves out at the usual 1200 mm.
        Element::Level(l) => {
            extra.insert("computation_height".into(), json!(l.computation_height));
        }
        Element::Dimension3D(d) => {
            extra.insert("length".into(), json!(hundredth(crate::dims3d::length(d))));
        }
        Element::Grid(_) | Element::Schedule(_) | Element::View3D(_) | Element::PlanView(_) => {}
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

/// IFC files this large or larger are refused. Reading one peaks at about 12 to 14 times its size in memory (measured on
/// Revit-like, meshed and BIMCraft-written files of 17 to 81 MB), so one just under 512 MiB stays under about 8 GB.
const MAX_IFC: u64 = 512 << 20;

/// Files of `extension` (in lower case) this large or larger are refused by `open`: an IFC file at [`MAX_IFC`], a project at
/// [`MAX_FILE`].
fn max_file(extension: &str) -> u64 {
    if extension == "ifc" { MAX_IFC } else { MAX_FILE }
}

#[cfg(test)]
mod max_file_tests {
    use super::*;

    #[test]
    fn an_ifc_file_may_be_twice_as_large_as_a_project_so_issue_264s_opens() {
        // Issue #264: an IFC file of 282,607,463 bytes was refused as too large; a project or a family keeps its 256 MiB.
        assert!(max_file("ifc") > 282_607_463);
        assert_eq!(max_file("ifc"), 512 << 20);
        for other in ["bimcraft", "json", "", "ifczip", "IFC"] {
            assert_eq!(max_file(other), MAX_FILE, "{other}");
        }
    }
}

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
    let codes = "en, pt, es or sv";
    Language::from_code(code).ok_or_else(|| bad(c, format!("`{code}` is no language here; use {codes}")))
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
    let failed = relink_cads(s);
    if !failed.is_empty() {
        reply["cads_not_reloaded"] = json!(failed);
    }
    reply
}

/// `open`, but for what its links read.
fn open_file(s: &mut Session, p: &Value) -> R {
    let c = "open";
    let path = PathBuf::from(string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is required"))?);
    let size = std::fs::metadata(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?.len();
    let most = max_file(&ext(&path));
    if size >= most {
        return Err(bad(c, format!("{} is too large ({size} bytes; `open` takes one under {most})", path.display())));
    }
    // IFC comes in as a new project: saving it asks where.
    if ext(&path) == "ifc" {
        let bytes = std::fs::read(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        let stem = path.file_stem().map_or_else(|| "Project".into(), |s| s.to_string_lossy().into_owned());
        let i = crate::import::ifc(&bytes, &stem).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for e in i.model.elements.values() {
            *counts.entry(e.category()).or_default() += 1;
        }
        // What came in as generic components, of the IFC classes BIMCraft has no element for.
        let generic = i
            .model
            .elements
            .values()
            .filter(|e| matches!(e, Element::Component(c) if i.model.families.get(&c.family).is_some_and(|f| f.remembered().is_some())))
            .count();
        let reply = json!({"path": path, "name": i.model.name, "format": i.schema, "elements": counts, "generic": generic,
            "skipped": i.skipped, "moved": i.moved});
        s.reset(i.model, None);
        return Ok(reply);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
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

fn filter_selection(s: &mut Session, p: &Value) -> R {
    let c = "filter_selection";
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let fields = crate::selection::common(m, &ids);
    let by = string(c, p, "by")?.unwrap_or_else(|| crate::selection::CATEGORY.into());
    if !fields.iter().any(|(n, _)| *n == by) {
        let names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
        return Err(bad(c, format!("`{by}` isn't a parameter every element has; they share: {}", names.join(", "))));
    }
    let groups = crate::selection::groups(m, &ids, &by);
    let mut reply = json!({
        "by": by,
        "fields": fields.iter().map(|(n, _)| n).collect::<Vec<_>>(),
        "groups": groups.iter().map(|g| json!({"value": g.value, "count": g.ids.len(), "ids": g.ids})).collect::<Vec<_>>(),
    });
    match p.get("keep") {
        v if absent(v) => {}
        Some(Value::Array(a)) if a.len() <= MAX_POINTS => {
            let keep = a
                .iter()
                .map(|v| match v {
                    Value::String(t) if t.chars().count() <= MAX_TEXT => Ok(t.clone()),
                    v => Err(bad(c, format!("`keep` holds values as text of at most {MAX_TEXT} characters, got {v}"))),
                })
                .collect::<Result<Vec<_>, _>>()?;
            reply["ids"] = json!(crate::selection::kept(m, &ids, &by, &keep));
        }
        Some(v) => return Err(bad(c, format!("`keep` must be a list of values, got {v}"))),
        None => {}
    }
    Ok(reply)
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
    let full = beside(s, path);
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
        std::fs::write(&path, crate::ifc::export(&s.model)).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        return Ok(json!({"path": path, "format": "IFC4"}));
    }
    if format == "glb" {
        let out = crate::gltf::export(&crate::link::seen(&s.model)).map_err(|e| bad(c, e))?;
        std::fs::write(&path, &out.bytes).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        return Ok(json!({"path": path, "format": "glTF 2.0", "elements": out.nodes, "triangles": out.triangles}));
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
        std::fs::write(&path, pdf::pdf(&pages)).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        return Ok(json!({"path": path, "format": "PDF", "pages": pages.len()}));
    }
    if matches!(format.as_str(), "svg" | "png") {
        let level = level_param(c, &s.model, &json!({}), "view")?;
        write_plan(&render::plan(&crate::link::seen(&s.model), level, 1600, 1000), &path)?;
        return Ok(json!({"path": path, "format": format}));
    }
    let text = s.model.to_json().map_err(|e| bad(c, e))?;
    crate::autosave::atomic_write(&path, &text).map_err(|e| bad(c, e))?;
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
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// A picture that only comes as PNG (the 3D view, a section): written to `path`, a .png, when
/// there is one, else returned in base64; `info` says what it is.
fn png_reply(c: &str, p: &Value, png: Vec<u8>, mut info: Value) -> R {
    match string(c, p, "path")? {
        Some(path) if ext(Path::new(&path)) == "png" => {
            std::fs::write(&path, png).map_err(|e| bad(c, format!("{path}: {e}")))?;
            info["path"] = json!(path);
        }
        Some(path) => return Err(bad(c, format!("only plans make SVGs; save this view as .png, not as {path}"))),
        None => info["pngBase64"] = json!(base64(&png)),
    }
    Ok(info)
}

/// A family drawn on its own: its `type` standing in a model of its own — in a wall `wall_thickness`
/// mm thick if it's a door's or a window's, on a floor of its own footprint if it isn't — and that
/// model drawn by [`render_view`]. Nothing of this project's is touched and nothing is left in it:
/// what a preview costs the project is nothing, no element and no undo step.
fn family_preview(s: &mut Session, p: &Value) -> R {
    let c = "render";
    let name = string(c, p, "family")?.ok_or_else(|| bad(c, "`family` is required: the family to draw"))?;
    let family = s.model.families.get(&name).ok_or_else(|| bad(c, format!("there's no family `{name}`; `inspect` lists them")))?.clone();
    let ty = string(c, p, "type")?.unwrap_or_else(|| family.types.keys().next().cloned().unwrap_or_default());
    if ty.is_empty() {
        return Err(bad(c, format!("family `{name}` has no type to draw")));
    }
    if !family.types.contains_key(&ty) {
        let types: Vec<&str> = family.types.keys().map(String::as_str).collect();
        return Err(bad(c, format!("family `{name}` has no type `{ty}` (there are: {})", types.join(", "))));
    }
    let mode = string(c, p, "mode")?.unwrap_or_else(|| "3d".into());
    if !matches!(mode.as_str(), "3d" | "plan" | "elevation") {
        return Err(bad(c, format!("`mode` is 3d, plan or elevation, not `{mode}`")));
    }
    let wall = num(c, p, "wall_thickness")?.unwrap_or(crate::family::DEFAULT_WALL);
    if !(0.0..MAX_COORD).contains(&wall) {
        return Err(bad(c, format!("`wall_thickness` must be mm above zero and up to {MAX_COORD}, got {wall}")));
    }
    // What its type comes to, so the host is sized to the opening it makes.
    let values = family
        .values_in(&ty, &BTreeMap::new(), crate::family::DEFAULT_WALL)
        .map_err(|e| bad(c, format!("family `{name}` type `{ty}`: {e}")))?;
    let number = |what: &str| match values.get(what) {
        Some(crate::family::Val::Number(x)) => Some(*x),
        _ => None,
    };
    // A model of its own, holding the project's materials so what's drawn looks as it would here.
    let mut scratch = Session::new();
    {
        let m = scratch.model_mut();
        m.materials = s.model.materials.clone();
        m.images = s.model.images.clone();
        m.families.insert(name.clone(), family.clone());
    }
    let made = |r: R, what: &str| r.map_err(|e| bad(c, format!("{what}: {e}")));
    if family.category.hosted() {
        // A wall as thick as asked, long and tall enough for the opening to stand whole in it.
        let width = number(crate::family::WIDTH).unwrap_or(1000.0);
        let height = number(crate::family::HEIGHT).unwrap_or(2100.0);
        let span = (width * 2.0 + 1000.0).max(4000.0);
        let tall = (height + 900.0).max(3000.0);
        made(
            scratch.execute("type", &json!({"category": "wall", "name": "Family Preview Wall", "thickness": wall})),
            "a wall of that thickness",
        )?;
        made(
            scratch.execute(
                "wall",
                &json!({"points": [[-span / 2.0, 0.0], [span / 2.0, 0.0]], "type": "Family Preview Wall", "height": tall}),
            ),
            "a wall for it to stand in",
        )?;
        let kind = if family.category == crate::family::Category::Door { "door" } else { "window" };
        made(scratch.execute(kind, &json!({"at": [0.0, 0.0], "family": name, "type": ty})), "the opening")?;
    } else {
        // A floor over its own footprint, so it stands on something and the picture has a ground.
        let mut box_ = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for shape in family.shapes(&values) {
            box_[0] = box_[0].min(shape.min[0]);
            box_[1] = box_[1].min(shape.min[1]);
            box_[2] = box_[2].max(shape.max[0]);
            box_[3] = box_[3].max(shape.max[1]);
        }
        if !box_[0].is_finite() || box_[2] < box_[0] || box_[3] < box_[1] {
            box_ = [-1000.0, -1000.0, 1000.0, 1000.0];
        }
        let pad = 500.0;
        let (x0, y0, x1, y1) = (box_[0] - pad, box_[1] - pad, box_[2] + pad, box_[3] + pad);
        made(scratch.execute("floor", &json!({"boundary": [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]})), "a floor under it")?;
        made(scratch.execute("component", &json!({"family": name, "type": ty, "at": [0.0, 0.0]})), "the component")?;
    }
    // What the picture is of, without what the preview doesn't take: `render` draws this model,
    // from where the mode asks.
    let mut q = p.clone();
    if let Some(o) = q.as_object_mut() {
        for key in ["family", "mode", "type", "wall_thickness", "ceiling", "eye_elevation"] {
            o.remove(key);
        }
        match mode.as_str() {
            "plan" => {
                for key in ["view", "azimuth", "elevation"] {
                    o.remove(key);
                }
            }
            // Front-on: as seen from the north (+y), level with the model.
            "elevation" => {
                o.insert("view".into(), json!("{3D}"));
                o.insert("azimuth".into(), json!(90.0));
                o.insert("elevation".into(), json!(0.0));
            }
            _ => {
                o.insert("view".into(), json!("{3D}"));
            }
        }
    }
    let mut reply = render_view(&mut scratch, &q)?;
    reply["view"] = json!(name);
    reply["kind"] = json!(format!("family {mode} preview"));
    reply["family"] = json!(name);
    reply["type"] = json!(ty);
    reply["mode"] = json!(mode);
    Ok(reply)
}

fn render_view(s: &mut Session, p: &Value) -> R {
    let c = "render";
    // A family on its own, in a model of its own: this project isn't touched by it. `mode` and the
    // rest say so too, for a reply that means a family's preview but forgot to name one.
    if ["family", "type", "mode", "wall_thickness"].iter().any(|k| p.get(*k).is_some_and(|v| !v.is_null())) {
        return family_preview(s, p);
    }
    // What its links build is in the picture.
    let seen = crate::link::seen(&s.model);
    let m = &*seen;
    let side = |key: &str, default: usize| count_opt(c, p, key, default).map(|n| n.clamp(16, 8192) as u32);
    let (w, h) = (side("width", 1600)?, side("height", 1000)?);
    let (w3, h3) = (w.min(view3d::MAX_SIDE), h.min(view3d::MAX_SIDE));
    let view = p.get("view");
    // A 3D view: the default `{3D}`, or any view named or id'd as one (a camera view made with `camera`). Its own camera
    // stands the picture where it looks from; the default fitted over the model when it has none.
    let three_d = match view {
        Some(Value::String(v)) if v.eq_ignore_ascii_case("{3D}") || v.eq_ignore_ascii_case("3D") => Some(m.default_3d()),
        Some(Value::String(v)) if Guid::parse(v).is_none() => {
            m.elements.iter().find_map(|(id, e)| matches!(e, Element::View3D(v3) if v3.name == *v).then_some(*id)).map(Some)
        }
        Some(v) => serde_json::from_value::<Guid>(v.clone())
            .ok()
            .and_then(|g| m.elements.id(g))
            .filter(|id| matches!(m.elements.get(id), Some(Element::View3D(_))))
            .map(Some),
        _ => None,
    };
    if let Some(three) = three_d {
        let (azimuth, elevation) = (num(c, p, "azimuth")?.unwrap_or(view3d::AZIMUTH), num(c, p, "elevation")?.unwrap_or(view3d::ELEVATION));
        let scene = view3d::Scene::new(m);
        let look = Look::of_3d(m);
        // Through the view's lens, if it's in perspective, and from as high as the eye is put.
        let held = three.and_then(|v| match m.elements.get(v) {
            Some(Element::View3D(v3)) => Some(v3.clone()),
            _ => None,
        });
        let lens = held.as_ref().and_then(View3D::lens);
        let name = held.as_ref().map_or_else(|| "{3D}".to_string(), |v| v.name.clone());
        let camera = held.as_ref().and_then(|v| v.camera);
        // A camera view is seen from its eye at its target through its lens; the default is fitted over the model.
        let fitted = match (camera, lens) {
            (Some(cam), Some(lens)) => view3d::Camera::at(cam.eye, cam.target, lens, w3, h3),
            _ => scene.fit_from(view3d::Camera { lens, ..scene.fit_in(azimuth, elevation, w3, h3, &look) }, &look),
        };
        let cam = match (num(c, p, "eye_elevation")?, lens) {
            (Some(z), Some(_)) if z.is_finite() && z.abs() <= MAX_COORD => fitted.with_eye_elevation(z),
            (Some(z), Some(_)) => return Err(bad(c, format!("`eye_elevation`: {z} mm is not a usable height"))),
            (Some(_), None) => return Err(bad(c, "a parallel 3D view has no eye to raise: turn it to perspective first (`perspective`)")),
            (None, _) => fitted,
        };
        let mut image = scene.render_in(&cam, &[], &look);
        // Its dimensions, over the picture.
        if let Some(v) = three {
            crate::dims3d::paint(&mut image, &crate::dims3d::marks(m, v, &cam, &look, crate::dims3d::SCREEN));
        }
        let png = image.png().map_err(|e| bad(c, e))?;
        return png_reply(
            c,
            p,
            png,
            json!({"width": w3, "height": h3, "view": name, "kind": "3D view", "azimuth": cam.azimuth, "elevation": cam.elevation,
                "lens": lens, "eye_elevation": cam.eye_elevation(), "eye": camera.map(|c| c.eye), "target": camera.map(|c| c.target)}),
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
            std::fs::write(&path, pdf::pdf(&[(&d, size)])).map_err(|e| bad(c, format!("{path}: {e}")))?;
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
        Some(k) if !schedule::categories().contains(&category(&k).as_str()) && category(&k) != schedule::MULTI => {
            return Err(bad(
                c,
                format!("`{k}` isn't a category schedules are of; there are: {}, {}", schedule::categories().join(", "), schedule::MULTI),
            ));
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
    // A multi-category one's categories, as elements name theirs; `[]`, every built one.
    if let Some(v) = p.get("categories").filter(|v| !v.is_null()) {
        let given: Vec<String> = read("categories", v.clone())?;
        (sc.categories, changed) = (given.iter().map(|k| category(k)).collect(), true);
    }
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
        // A workbook for a path ending .xlsx, else CSV.
        let bytes = if path.to_lowercase().ends_with(".xlsx") {
            let sheet = if sc.name.is_empty() { category(&sc.of) } else { sc.name.clone() };
            crate::schedule::to_xlsx(&t, &sheet)
        } else {
            crate::schedule::to_csv(&t).into_bytes()
        };
        std::fs::write(&path, bytes).map_err(|e| bad(c, format!("{path}: {e}")))?;
    }
    let columns: Vec<String> = t.columns.iter().map(|col| crate::schedule::heading(Units::Millimetres, col)).collect();
    let available: Vec<String> = crate::schedule::fields(m, &category(&sc.of)).into_iter().map(|(n, _)| n).collect();
    let mut out = json!({"id": id, "name": sc.name, "category": category(&sc.of), "columns": columns, "rows": t.rows, "elements": t.ids, "available": available});
    if category(&sc.of) == schedule::MULTI {
        out["categories"] = json!(schedule::listed(&sc));
    }
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
                graphics::MODEL
                    .iter()
                    .chain(&graphics::ANNOTATION)
                    .chain(&graphics::SCANS)
                    .chain(&graphics::IMPORTED)
                    .copied()
                    .chain([graphics::COMPONENT])
                    .chain(graphics::SUBCATEGORIES.iter().map(|s| s.key))
                    .collect::<Vec<_>>()
                    .join(", ")
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
    let views_own = ["hide_elements", "show_elements", "elements", "layers"];
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
            (p.get("background_grid").is_some(), Group::BackgroundGrid, "background grids"),
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
    if let Some(patch) = p.get("background_grid") {
        if patch.is_null() {
            g.background_grid = Default::default();
        } else if patch.is_object() {
            let mut now = serde_json::to_value(g.background_grid).map_err(|e| bad(c, e))?;
            merge(&mut now, patch);
            g.background_grid = serde_json::from_value(now).map_err(|e| bad(c, format!("background grid: {e}")))?;
        } else {
            return Err(bad(c, "`background_grid` is an object, or null to restore defaults"));
        }
    }
    for name in drawn_categories(c, p, "hide")? {
        let o = g.categories.entry(name).or_default();
        (o.hidden, o.shown) = (true, false);
    }
    // A subcategory off unless shown is shown by saying so.
    for name in drawn_categories(c, p, "show")? {
        let o = g.categories.entry(name.clone()).or_default();
        // Rooms are off in a 3D view unless shown there; shown, a plan draws them as it would.
        (o.hidden, o.shown) = (false, graphics::off_unless_shown(&name) || graphics::OFF_IN_3D.contains(&name.as_str()));
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
    match p.get("layers") {
        v if absent(v) => {}
        Some(Value::Object(o)) if o.len() <= crate::cad::MAX_CADS => {
            for (cad, hidden) in o {
                let Some(drawing) = m.cads.get(cad) else {
                    return Err(bad(c, format!("`layers`: the project has no CAD drawing `{cad}` (see `elements` of category cadimport)")));
                };
                let names: BTreeSet<String> = match hidden {
                    Value::Null => BTreeSet::new(),
                    Value::Array(a) if a.len() <= crate::model::MAX_HIDDEN => a
                        .iter()
                        .map(|v| {
                            v.as_str().map(str::to_string).ok_or_else(|| bad(c, format!("`layers`: `{cad}` lists layers by name, not {v}")))
                        })
                        .collect::<Result<_, _>>()?,
                    v => return Err(bad(c, format!("`layers`: `{cad}` is a list of the layers to hide, or null, not {v}"))),
                };
                if let Some(n) = names.iter().find(|n| !drawing.dxf().layers.iter().any(|l| &l.name == *n)) {
                    return Err(bad(c, format!("`layers`: `{cad}` has no layer `{n}` (see `cad_layers`)")));
                }
                if names.is_empty() {
                    g.layers.remove(cad);
                } else {
                    g.layers.insert(cad.clone(), names);
                }
            }
        }
        Some(v) => return Err(bad(c, format!("`layers` is {{drawing: [layer, …] | null}}, not {v}"))),
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
        "hidden": hidden, "categories": g.categories, "filters": g.filters, "layers": g.layers,
        "phase": g.phase, "phase_filter": g.phase_filter, "options": g.options,
        "scale": g.scale, "detail": g.detail, "visual_style": g.style,
        "background_grid": g.background_grid,
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

fn perspective(s: &mut Session, p: &Value) -> R {
    let c = "perspective";
    let m = &s.model;
    let default = absent(p.get("view"))
        || matches!(p.get("view"), Some(Value::String(n)) if n.eq_ignore_ascii_case("{3D}") || n.eq_ignore_ascii_case("3D"));
    // The default 3D view is made when it's first turned, as when its graphics first change.
    let view = if default {
        m.default_3d()
    } else {
        let v = view_param(c, m, p, false)?;
        if !matches!(m.elements.get(v), Some(Element::View3D(_))) {
            return Err(bad(c, format!("{} isn't a 3D view; only a 3D view is seen in perspective", view_title(m, v))));
        }
        Some(v)
    };
    let was = view.and_then(|v| match m.elements.get(v) {
        Some(Element::View3D(v3)) => v3.perspective,
        _ => None,
    });
    let on = match p.get("on") {
        v if absent(v) => true,
        Some(Value::Bool(b)) => *b,
        Some(v) => return Err(bad(c, format!("`on` must be true or false, got {v}"))),
        None => true,
    };
    let lens = num(c, p, "lens")?.or(was.map(|w| w.lens)).unwrap_or(view3d::LENS);
    let v = match view {
        Some(v) => v,
        None => s.model_mut().insert(Element::View3D(View3D::default_view())),
    };
    let set = Perspective { lens, on };
    if let Some(Element::View3D(v3)) = s.model_mut().elements.get_mut(v) {
        v3.perspective = Some(set);
    }
    Ok(json!({"view": v, "on": on, "lens": lens}))
}

fn graphic_display_options(s: &mut Session, p: &Value) -> R {
    let c = "graphic_display_options";
    let m = &s.model;
    let default = absent(p.get("view"))
        || matches!(p.get("view"), Some(Value::String(n)) if n.eq_ignore_ascii_case("{3D}") || n.eq_ignore_ascii_case("3D"));
    // The default 3D view is made when its options first change, as when its graphics first change.
    let view = if default {
        m.default_3d()
    } else {
        let v = view_param(c, m, p, false)?;
        if !matches!(m.elements.get(v), Some(Element::View3D(_))) {
            return Err(bad(c, format!("{} isn't a 3D view; only a 3D view has graphic display options", view_title(m, v))));
        }
        Some(v)
    };
    let was = view
        .and_then(|v| match m.elements.get(v) {
            Some(Element::View3D(v3)) => Some(v3.display),
            _ => None,
        })
        .unwrap_or_default();
    let on = |key: &str| match p.get(key) {
        v if absent(v) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(v) => Err(bad(c, format!("`{key}` must be true or false, got {v}"))),
        None => Ok(None),
    };
    let most = view3d::MAX_SILHOUETTE;
    let silhouette = match p.get("silhouette") {
        v if absent(v) => None,
        Some(v) => Some(
            v.as_f64()
                .filter(|px| px.fract() == 0.0 && (0.0..=f64::from(most)).contains(px))
                .map(|px| px as u32)
                .ok_or_else(|| bad(c, format!("`silhouette` is how many px wide the outline is, a whole number 0 to {most}, got {v}")))?,
        ),
        None => None,
    };
    let now = view3d::DisplayOptions {
        edges: on("edges")?.unwrap_or(was.edges),
        silhouette: silhouette.unwrap_or(was.silhouette),
        ambient_shadows: on("ambient_shadows")?.unwrap_or(was.ambient_shadows),
    };
    let view = match view {
        None if now != was => Some(s.model_mut().insert(Element::View3D(View3D::default_view()))),
        v => v,
    };
    if let (true, Some(v)) = (now != was, view)
        && let Some(Element::View3D(v3)) = s.model_mut().elements.get_mut(v)
    {
        v3.display = now;
    }
    Ok(json!({"view": view, "edges": now.edges, "silhouette": now.silhouette, "ambient_shadows": now.ambient_shadows}))
}

/// `[x, y, z]` in mm within the model's limits, or an error naming `key`.
fn point3_req(c: &str, p: &Value, key: &str) -> Result<[f64; 3], String> {
    match p.get(key) {
        v if absent(v) => Err(bad(c, format!("`{key}` [x, y, z] is required"))),
        Some(v) => point3(v).ok_or_else(|| bad(c, format!("`{key}` must be a point [x, y, z] in mm, got {v}"))),
        None => Err(bad(c, format!("`{key}` [x, y, z] is required"))),
    }
}

fn camera(s: &mut Session, p: &Value) -> R {
    let c = "camera";
    let m = &s.model;
    let eye_given = !absent(p.get("eye"));
    let target_given = !absent(p.get("target"));
    let view = p.get("view");
    let named_view = !absent(view);
    // The default 3D view asked for by name: it is made when it's first given a camera, as when its perspective is first
    // turned; nothing exists yet to look up.
    let default_3d = matches!(view, Some(Value::String(n)) if n.eq_ignore_ascii_case("{3D}") || n.eq_ignore_ascii_case("3D"));
    // With a view and none of eye, target or lens, this reads the camera; it needs the view to exist. Otherwise the view is
    // where the camera goes: given, a new default `{3D}`, or a new camera view made below.
    let v = match view {
        Some(_) if named_view && !default_3d => {
            let id = view_param(c, m, p, false)?;
            if !matches!(m.elements.get(id), Some(Element::View3D(_))) {
                return Err(bad(c, format!("{} isn't a 3D view; a camera goes on a 3D view", view_title(m, id))));
            }
            Some(id)
        }
        Some(_) if named_view && default_3d => m.default_3d(),
        _ => None,
    };
    let reading = named_view && !eye_given && !target_given && absent(p.get("lens"));
    // A point given must be a point, by its name; one left out stays as the view has it (below).
    let eye = if eye_given { Some(point3_req(c, p, "eye")?) } else { None };
    let target = if target_given { Some(point3_req(c, p, "target")?) } else { None };
    let lens = num(c, p, "lens")?;
    let was = v.and_then(|id| match m.elements.get(id) {
        Some(Element::View3D(v3)) => v3.perspective.filter(|p| p.on).map(|p| p.lens),
        _ => None,
    });
    // Read: the camera as the view has it, with what its perspective says. Reading makes nothing: the default `{3D}`
    // before anything made it reads as a view with no camera yet.
    if reading {
        let (cam, pers, name) = match v.and_then(|id| m.elements.get(id)) {
            Some(Element::View3D(v3)) => (v3.camera, v3.perspective.map(|p| (p.on, p.lens)), v3.name.clone()),
            _ => (None, None, "{3D}".to_string()),
        };
        let (on, lens) = pers.unwrap_or((false, lens.unwrap_or(view3d::LENS)));
        return Ok(
            json!({"view": v, "name": name, "eye": cam.map(|c| c.eye), "target": cam.map(|c| c.target), "lens": lens, "perspective": on}),
        );
    }
    // Set: the points kept from the camera the view has when they are not given, so a lens alone (or one point) changes
    // just that; a view without a camera yet needs both. The eye is never on the target.
    let kept = v.and_then(|id| match m.elements.get(id) {
        Some(Element::View3D(v3)) => v3.camera,
        _ => None,
    });
    let (Some(eye), Some(target)) = (eye.or(kept.map(|k| k.eye)), target.or(kept.map(|k| k.target))) else {
        return Err(bad(c, "`eye` and `target` [x, y, z] are both required to set a camera"));
    };
    let across = (0..3).map(|k| eye[k] - target[k]).fold(0.0, |acc, d| acc + d * d).sqrt();
    if across <= TOL {
        return Err(bad(c, "the eye is on the target; give the camera two points a little apart"));
    }
    // Set: the lens kept from the view it lands on when not given.
    let lens = lens.or(was).unwrap_or(view3d::LENS);
    if !(view3d::MIN_LENS..=view3d::MAX_LENS).contains(&lens) {
        let (lo, hi) = (view3d::MIN_LENS, view3d::MAX_LENS);
        return Err(bad(c, format!("a {lens} mm lens is no camera's; a camera's lens is {lo} to {hi} mm")));
    }
    let set = crate::model::ViewCamera { eye, target };
    let id = match v {
        Some(id) => id,
        // The default `{3D}` when asked for and not made yet keeps its own name; a new camera view is named by number.
        None if default_3d => s.model_mut().insert(Element::View3D(View3D { camera: Some(set), ..View3D::default_view() })),
        None => {
            let name = match string(c, p, "name")? {
                Some(n) => n,
                None => s.model.free_view_name(s.model.language.word("Camera"), true),
            };
            s.model_mut().insert(Element::View3D(View3D { name, camera: Some(set), ..View3D::default_view() }))
        }
    };
    if let Some(Element::View3D(v3)) = s.model_mut().elements.get_mut(id) {
        v3.camera = Some(set);
        v3.perspective = Some(Perspective { lens, on: true });
    }
    Ok(json!({"view": id, "id": id, "name": s.model.view_name(id), "eye": eye, "target": target, "lens": lens, "perspective": true}))
}

fn work_plane(s: &mut Session, p: &Value) -> R {
    let c = "work_plane";
    only(c, p, &["view", "level", "offset", "points", "surface"])?;
    let m = &s.model;
    let default = absent(p.get("view"))
        || matches!(p.get("view"), Some(Value::String(n)) if n.eq_ignore_ascii_case("{3D}") || n.eq_ignore_ascii_case("3D"));
    let view = if default {
        m.default_3d()
    } else {
        let v = view_param(c, m, p, false)?;
        if !matches!(m.elements.get(v), Some(Element::View3D(_))) {
            return Err(bad(c, format!("{} isn't a 3D view; only a 3D view has a work plane to set", view_title(m, v))));
        }
        Some(v)
    };
    let offset = num(c, p, "offset")?;
    let points = match p.get("points") {
        v if absent(v) => None,
        Some(v) => Some(
            serde_json::from_value::<[[f64; 3]; 3]>(v.clone())
                .map_err(|_| bad(c, format!("`points` are three points [[x, y, z], [x, y, z], [x, y, z]] in mm, not {v}")))?,
        ),
        None => None,
    };
    let surface = match p.get("surface") {
        v if absent(v) => None,
        Some(v) => Some(serde_json::from_value::<Surface>(v.clone()).map_err(|e| {
            bad(c, format!("`surface` is {{point: [x, y, z], normal: [x, y, z]}}, a point of it in mm and the way it faces: {e}"))
        })?),
        None => None,
    };
    let level = if absent(p.get("level")) { None } else { Some(level_param(c, m, p, "level")?) };
    let given = WorkPlane { level, offset: offset.unwrap_or(0.0), points, surface };
    let plane = if level.is_some() || points.is_some() || surface.is_some() {
        Some(given)
    } else {
        offset.map(|offset| WorkPlane { offset, ..m.work_plane(view) })
    };
    let resolved = plane.as_ref().map_or_else(|| m.work_plane(view).resolve(m), |wp| wp.resolve(m)).map_err(|e| bad(c, e))?;
    let v = match view {
        Some(v) => v,
        None => s.model_mut().insert(Element::View3D(View3D::default_view())),
    };
    if let Some(Element::View3D(v3)) = s.model_mut().elements.get_mut(v) {
        v3.work_plane = plane;
    }
    let wp = s.model.work_plane(Some(v));
    let level = wp.level.and_then(|g| s.model.elements.id(g));
    let z = resolved.over_plan().map(|p| p.z);
    Ok(json!({
        "view": v, "level": level, "offset": wp.offset, "points": wp.points, "surface": wp.surface,
        "origin": resolved.origin, "normal": resolved.normal, "z": z, "slope": resolved.slope(),
    }))
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
        "like",
        "kind",
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
        "background_grid",
    ];
    let makes = keys.iter().any(|k| !absent(p.get(k))) || p.get("template").is_some();
    if was.is_none() && !makes {
        let there: Vec<&str> = m.view_templates.keys().map(String::as_str).collect();
        return Err(bad(
            c,
            format!("there's no view template `{name}`; make one with `from` (a view), or `controls`{}", known_names(&there)),
        ));
    }
    let mut t = match (&was, name_param(c, p, "like")?) {
        (Some(t), _) => t.clone(),
        (None, Some(like)) => m.view_templates.get(&like).cloned().ok_or_else(|| {
            let there: Vec<&str> = m.view_templates.keys().map(String::as_str).collect();
            bad(c, format!("there's no view template `{like}` to start from{}", known_names(&there)))
        })?,
        (None, None) => ViewTemplate { controls: Group::ALL.into_iter().collect(), ..ViewTemplate::default() },
    };
    match p.get("kind") {
        None => {}
        Some(Value::Null) => t.kind = None,
        Some(v) => {
            t.kind = Some(
                serde_json::from_value(v.clone())
                    .map_err(|_| bad(c, format!("`kind` is plan, section or 3d, or null for any, not {v}")))?,
            );
        }
    }
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
            background_grid: resolved.background_grid,
            ..Graphics::default()
        };
        t.view_range = match m.elements.get(view) {
            Some(Element::Level(_) | Element::PlanView(_)) => Some(m.view_range_of(view)),
            _ => None,
        };
        t.kind = m.elements.get(view).and_then(graphics::ViewKind::of);
    }
    match p.get("controls") {
        v if absent(v) => {}
        Some(v) => {
            t.controls = serde_json::from_value(v.clone()).map_err(|e| {
                bad(
                    c,
                    format!(
                        "`controls` is a list of what it controls: {}: {e}",
                        Group::ALL.map(|g| serde_json::to_string(&g).unwrap_or_default()).join(", ")
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
    let name = match name_param(c, p, "rename")? {
        Some(to) if to != name => {
            let m = s.model_mut();
            if m.view_templates.contains_key(&to) {
                return Err(bad(c, format!("there's already a view template `{to}`")));
            }
            m.view_templates.remove(&name);
            m.view_templates.insert(to.clone(), t.clone());
            rename_entry(m, &LibKind::ViewTemplate, &name, &to);
            to
        }
        _ => name,
    };
    let m = &s.model;
    let mut reply = graphics_reply(&t.graphics);
    reply["name"] = json!(name);
    reply["kind"] = json!(t.kind);
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

/// A number of `key` within `lo..=hi`, or `default` when absent.
fn within(c: &str, p: &Value, key: &str, default: f64, lo: f64, hi: f64) -> Result<f64, String> {
    match p.get(key) {
        v if absent(v) => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && *x >= lo && *x <= hi)
            .ok_or_else(|| bad(c, format!("`{key}` must be a number from {lo} to {hi}, got {v}"))),
        None => Ok(default),
    }
}

/// A choice among `names`, as text; `default` when absent.
fn choice<'a>(c: &str, p: &'a Value, key: &str, names: &[&str], default: &'a str) -> Result<&'a str, String> {
    match p.get(key) {
        v if absent(v) => Ok(default),
        Some(Value::String(t)) if names.contains(&t.as_str()) => Ok(t.as_str()),
        Some(v) => Err(bad(c, format!("`{key}` must be one of {}, got {v}", names.join(", ")))),
        None => Ok(default),
    }
}

fn circuit(_: &mut Session, p: &Value) -> R {
    use crate::nbr5410::{self as e, Circuit, Insulation, Phases, Use};
    let c = "circuit";
    let o = p.as_object().ok_or_else(|| bad(c, "params must be an object"))?;
    const KEYS: &[&str] = &[
        "load",
        "apparent",
        "demand",
        "power_factor",
        "phases",
        "system",
        "use",
        "length",
        "method",
        "insulation",
        "conductor",
        "ambient",
        "grouped",
        "bundle",
        "spacing",
        "soil",
        "section",
        "breaker",
        "drop_limit",
        "reduced_neutral",
    ];
    if let Some(key) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    let load = within(c, p, "load", 0.0, 0.0, 1.0e9)?;
    if load <= 0.0 {
        return Err(bad(c, "`load` (W, or VA with `apparent`) is required"));
    }
    let systems: Vec<&str> = e::SYSTEMS.iter().map(|s| s.0).collect();
    let system = choice(c, p, "system", &systems, systems[0])?;
    let methods = e::method_keys();
    let sections_ok = |x: f64| e::SECTIONS.iter().any(|s| (s - x).abs() < 1e-9);
    let section = within(c, p, "section", 0.0, 0.0, 1000.0)?;
    if section > 0.0 && !sections_ok(section) {
        return Err(bad(
            c,
            format!(
                "`section` must be one of the standard's: {}",
                e::SECTIONS.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
            ),
        ));
    }
    let breaker = within(c, p, "breaker", 0.0, 0.0, 1000.0)?;
    if breaker > 0.0 && !e::BREAKERS.iter().any(|b| (b - breaker).abs() < 1e-9) {
        return Err(bad(
            c,
            format!("`breaker` must be one of: {}", e::BREAKERS.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", ")),
        ));
    }
    let ambient = match p.get("ambient") {
        v if absent(v) => None,
        _ => Some(within(c, p, "ambient", 30.0, -50.0, 200.0)?),
    };
    let circuit = Circuit {
        load,
        apparent: flag(c, p, "apparent")?,
        demand: within(c, p, "demand", 1.0, f64::MIN_POSITIVE, 1.0)?,
        power_factor: within(c, p, "power_factor", 1.0, f64::MIN_POSITIVE, 1.0)?,
        phases: Phases::parse(choice(c, p, "phases", &["FN", "FF", "3F", "3FN"], "FN")?).unwrap_or(Phases::PhaseNeutral),
        system: systems.iter().position(|s| *s == system).unwrap_or(0),
        use_: if choice(c, p, "use", &["power", "lighting"], "power")? == "lighting" { Use::Lighting } else { Use::Power },
        length: within(c, p, "length", 0.0, 0.0, MAX_COORD)? / 1000.0,
        method: choice(c, p, "method", &methods, "7")?.to_string(),
        insulation: Insulation::parse(choice(c, p, "insulation", &["pvc", "epr"], "pvc")?).unwrap_or(Insulation::Pvc),
        flexible: choice(c, p, "conductor", &["flexible", "rigid"], "flexible")? == "flexible",
        ambient,
        grouped: within(c, p, "grouped", 1.0, 1.0, 1000.0)?.round() as u32,
        bundle: flag(c, p, "bundle")?,
        spacing: within(c, p, "spacing", 0.0, 0.0, 100_000.0)? / 1000.0,
        soil: within(c, p, "soil", 2.5, 0.0, 100.0)?,
        section,
        breaker,
        drop_limit: within(c, p, "drop_limit", e::DROP_LIMIT, f64::MIN_POSITIVE, 100.0)?,
        reduced_neutral: flag(c, p, "reduced_neutral")?,
    };
    let d = e::design(&circuit);
    if let Some(fault) = d.fault {
        return Err(bad(c, fault.message()));
    }
    let round = |x: f64, k: f64| (x * k).round() / k + 0.0;
    let evaluation = |v: &e::Evaluation| {
        json!({"section_mm2": v.section, "breaker_a": v.breaker, "carries_a": round(v.carries, 100.0), "carries_ok": v.ampacity_ok,
               "breaker_ok": v.breaker_ok, "drop_percent": v.drop.map(|d| round(d.0, 100.0)), "drop_ok": v.drop.map(|d| d.1),
               "minimum_section_ok": v.minimum_ok})
    };
    let Some(n) = d.needed else { return Err(bad(c, e::Fault::TooBig.message())) };
    Ok(json!({
        "current_a": round(d.current, 100.0),
        "volts": d.volts,
        "minimum_section_mm2": d.minimum,
        "factors": d.factors.map(|f| json!({"temperature": f.temperature, "grouping": f.grouping, "soil": f.soil})),
        "section_mm2": n.section,
        "breaker_a": n.breaker,
        "carries_a": round(n.carries, 100.0),
        "drop_percent": n.drop.map(|d| round(d.0, 100.0)),
        "conductors": e::cable_text(circuit.phases, n.section, circuit.reduced_neutral),
        "grew": section < n.section - 1e-9 || breaker < n.breaker - 1e-9,
        "chosen": d.chosen.as_ref().map(evaluation),
    }))
}

fn conduit_fill(_: &mut Session, p: &Value) -> R {
    use crate::conduit::{self, FAMILIES};
    use crate::nbr5410::Phases;
    let c = "conduit_fill";
    let o = p.as_object().ok_or_else(|| bad(c, "params must be an object"))?;
    const KEYS: &[&str] = &["section", "phases", "circuits", "reduced_neutral", "family", "size"];
    if let Some(key) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    let section = within(c, p, "section", 0.0, 0.0, 1000.0)?;
    if section <= 0.0 {
        return Err(bad(c, "`section` (mm²) is required"));
    }
    let phases = Phases::parse(choice(c, p, "phases", &["FN", "FF", "3F", "3FN"], "FN")?).unwrap_or(Phases::PhaseNeutral);
    let circuits = within(c, p, "circuits", 1.0, 1.0, 1000.0)?.round() as usize;
    let keys: Vec<&str> = FAMILIES.iter().map(|f| f.key).collect();
    let family = conduit::family(choice(c, p, "family", &keys, "pvc-thread")?).ok_or_else(|| bad(c, "no such family"))?;
    let one = conduit::conductors(phases, section, flag(c, p, "reduced_neutral")?);
    let all: Vec<f64> = (0..circuits).flat_map(|_| one.iter().copied()).collect();
    let source = json!({"maker": family.maker, "document": family.document, "page": family.page, "url": family.url, "inside": "outside less twice the wall"});
    if let Some(s) = one.iter().find(|s| conduit::cable_diameter(**s).is_none()) {
        return Ok(
            json!({"conductors": all.len(), "pending": format!("the catalogue has no outside diameter for a cable of {s} mm²"), "source": source}),
        );
    }
    let round = |x: f64, k: f64| (x * k).round() / k + 0.0;
    let smallest = conduit::smallest(family, &all).map(|s| s.0);
    let size =
        match string(c, p, "size")? {
            None => None,
            Some(mark) => Some(family.sizes.iter().find(|s| s.0 == mark).ok_or_else(|| {
                bad(c, format!("`size` must be one of {}", family.sizes.iter().map(|s| s.0).collect::<Vec<_>>().join(", ")))
            })?),
        };
    let fill = size.and_then(|s| conduit::fill(s, &all));
    Ok(json!({
        "conductors": all.len(),
        "circuits": circuits,
        "sections_mm2": one,
        "smallest": smallest,
        "size": size.map(|s| s.0),
        "inside_mm": size.map(|s| round(conduit::inside(s), 100.0)),
        "fill_percent": fill.map(|f| round(f.ratio * 100.0, 10.0)),
        "limit_percent": fill.map(|f| round(f.limit * 100.0, 10.0)),
        "ok": fill.map(|f| f.ok),
        "source": source,
    }))
}

fn drain_size(_: &mut Session, p: &Value) -> R {
    use crate::drainage::{self, FIXTURES, Role};
    let c = "drain_size";
    let o = p.as_object().ok_or_else(|| bad(c, "params must be an object"))?;
    const KEYS: &[&str] = &["units", "fixtures", "role", "slope", "floors", "min_dn"];
    if let Some(key) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    let mut units = within(c, p, "units", 0.0, 0.0, 1.0e7)?;
    let mut minimum = within(c, p, "min_dn", 0.0, 0.0, 1000.0)?;
    let mut fixtures_dn = 0.0;
    match p.get("fixtures") {
        v if absent(v) => {}
        Some(Value::Object(f)) => {
            let mut sum = 0.0;
            for (kind, n) in f {
                let (u, dn) = drainage::fixture(kind).ok_or_else(|| {
                    bad(c, format!("`fixtures`: no fixture `{kind}`, use {}", FIXTURES.iter().map(|x| x.0).collect::<Vec<_>>().join(", ")))
                })?;
                let n = n
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 10_000.0 && n.fract() == 0.0)
                    .ok_or_else(|| bad(c, format!("`fixtures`: `{kind}` must be a whole number from 0 to 10000, got {n}")))?;
                sum += u * n;
                if n > 0.0 {
                    fixtures_dn = f64::max(fixtures_dn, dn);
                }
            }
            units += sum;
        }
        Some(v) => return Err(bad(c, format!("`fixtures` must be an object {{kind: count}}, got {v}"))),
        None => {}
    }
    if units <= 0.0 {
        return Err(bad(c, "`units` (UHC) or `fixtures` is required"));
    }
    minimum = minimum.max(fixtures_dn);
    let role = Role::parse(choice(c, p, "role", &["branch", "main", "stack"], "branch")?).unwrap_or(Role::Branch);
    let slope = within(c, p, "slope", 0.01, 0.0, 1.0)?;
    let floors = within(c, p, "floors", 1.0, 1.0, 1000.0)?.round() as u32;
    let dn = drainage::smallest(units, slope, role, floors, minimum)
        .ok_or_else(|| {
        let most = drainage::DNS.iter().map(|dn| (*dn, drainage::capacity(*dn, slope, role, floors))).fold((0.0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
        bad(
            c,
            format!(
                "{units} UHC is more than the tables of this version list for this role and slope: the most is DN {} at {} UHC (a blank in the standard's table means not allowed, not that the tables ended); split the load, or give a steeper `slope`",
                most.0, most.1
            ),
        )
    })?;
    Ok(json!({"units": units, "dn": dn, "capacity": drainage::capacity(dn, slope, role, floors), "minimum_dn": minimum}))
}

fn vent_size(_: &mut Session, p: &Value) -> R {
    use crate::drainage;
    let c = "vent_size";
    let o = p.as_object().ok_or_else(|| bad(c, "params must be an object"))?;
    const KEYS: &[&str] = &["units", "wc", "stack_dn", "length", "trap_dn"];
    if let Some(key) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    let units = within(c, p, "units", 0.0, 0.0, 1.0e7)?;
    if units <= 0.0 {
        return Err(bad(c, "`units` (UHC of what it vents) is required"));
    }
    let dns = drainage::DNS;
    let mut out = serde_json::Map::new();
    let dn = if absent(p.get("stack_dn")) {
        drainage::branch_vent(units, flag(c, p, "wc")?)
            .ok_or_else(|| bad(c, format!("{units} UHC is more than the table gives a branch vent (Tabela 8)")))?
    } else {
        let served = within(c, p, "stack_dn", 0.0, 0.0, 1000.0)?;
        if !dns.iter().any(|d| (d - served).abs() < 1e-9) {
            return Err(bad(c, format!("`stack_dn` must be one of {}", dns.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(", "))));
        }
        if absent(p.get("length")) {
            return Err(bad(c, "`length` (mm, from the foot to the open end) is required with `stack_dn`"));
        }
        let length = within(c, p, "length", 0.0, 0.0, MAX_COORD)? / 1000.0;
        let dn = drainage::vent_stack(served, units, length)
            .ok_or_else(|| bad(c, "no vent of the table (Tabela 2) is long enough for that, or the units pass its last row"))?;
        out.insert("longest_m".into(), json!(drainage::vent_stack_limit(served, units, dn)));
        dn
    };
    out.insert("dn".into(), json!(dn));
    if !absent(p.get("trap_dn")) {
        let t = within(c, p, "trap_dn", 0.0, 0.0, 1000.0)?;
        let mm = drainage::trap_distance(t).ok_or_else(|| bad(c, "`trap_dn` must be 40, 50, 75 or 100 (Tabela 1)"))?;
        out.insert("trap_to_vent_mm".into(), json!(mm));
    }
    Ok(Value::Object(out))
}

/// `demand` for Enel: ET 1569 leaves the demand to the designer (item 7.7.2), so it frames the supply by the installed load, in its
/// Anexo A (aerial) or D (underground), and carries the designer's demand if one is given.
fn demand_enel(p: &Value, utility: &str, source: &str) -> R {
    use crate::demand::{self as d, NETWORKS, Network};
    let c = "demand";
    const KEYS: &[&str] = &["utility", "network", "installed_kw", "demand_current_a", "phases", "demand_kva"];
    if let Some(k) = p.as_object().and_then(|o| o.keys().find(|k| !KEYS.contains(&k.as_str()))) {
        return Err(bad(c, format!("`{k}` isn't one of ENEL's parameters: {}", KEYS.join(", "))));
    }
    let name = choice(c, p, "network", NETWORKS, "")?;
    if name.is_empty() {
        return Err(bad(c, format!("`network` is required: {}", NETWORKS.join(", "))));
    }
    let phases = match p.get("phases").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => match v.as_u64() {
            Some(n @ 1..=3) => Some(n as u8),
            _ => return Err(bad(c, format!("`phases` is 1, 2 or 3, got {v}"))),
        },
    };
    let demand_kva = match p.get("demand_kva").filter(|v| !v.is_null()) {
        None => None,
        Some(_) => Some(within(c, p, "demand_kva", 0.0, 0.0, 1.0e6)?),
    };
    let note_demand = "ET 1569 (item 7.7.2) leaves the demand to the designer, who may use the methods the utilities recommend: it is carried as given, never worked out here";
    let not_covered = "the conduits, the poles and the type of meter box of the annexes, Anexo A to E's other columns, and the limits of supply of Tabela 2 by motors, welding machines and X-ray sets";
    if name == "aerial_delta" {
        // Anexo B goes by the current the designer's demand draws, not by the installed load.
        if p.get("installed_kw").is_some_and(|v| !v.is_null()) {
            return Err(bad(c, "Anexo B (`aerial_delta`) goes by `demand_current_a`, not by `installed_kw`"));
        }
        if absent(p.get("demand_current_a")) {
            return Err(bad(c, "`demand_current_a` (the designer's demand current, A) is required for `aerial_delta`"));
        }
        let amps = within(c, p, "demand_current_a", 0.0, 0.0, 1.0e6)?;
        let rows = d::delta_pick(amps, phases);
        let (category, note) = match &rows {
            Some(rows) => {
                let r = rows[0];
                let conn = json!({"conductors_per_phase": r.connection.0, "phase": r.connection.1, "neutral": r.connection.2});
                let cat = json!({
                    "names": r.names,
                    "breaker_a": r.breaker,
                    "connection_mm2": conn,
                    "entry_mm2": r.entry,
                    "earthing_mm2": r.earth,
                    "measurement": r.measurement,
                });
                (cat, Value::Null)
            }
            None => (Value::Null, json!("above the last breaker of Anexo B: 300 A")),
        };
        let candidates: Vec<&str> = rows.iter().flatten().flat_map(|r| r.names.iter().copied()).collect();
        return Ok(json!({
            "utility": utility,
            "source": source,
            "annex": "B",
            "network": name,
            "installed_kw": Value::Null,
            "demand_current_a": amps,
            "phases": phases,
            "category": category,
            "candidates": candidates,
            "category_note": note,
            "demand_kva": demand_kva,
            "demand_note": note_demand,
            "not_covered": not_covered,
        }));
    }
    let network = Network::parse(name).ok_or_else(|| bad(c, format!("`network` is one of {}", NETWORKS.join(", "))))?;
    if p.get("demand_current_a").is_some_and(|v| !v.is_null()) {
        return Err(bad(c, "`demand_current_a` is for `aerial_delta` (Anexo B); the other networks go by `installed_kw`"));
    }
    if absent(p.get("installed_kw")) {
        return Err(bad(c, "`installed_kw` (the installed load in kW) is required"));
    }
    let kw = within(c, p, "installed_kw", 0.0, 0.0, 1.0e6)?;
    let pick = d::enel_pick(network, kw, phases);
    let (category, candidates, note) = match &pick {
        Some(k) => {
            let r = k.rows[0];
            let conn = json!({"conductors_per_phase": r.connection.0, "phase": r.connection.1, "neutral": r.connection.2});
            let cat = json!({
                "name": r.name,
                "available_kw": if r.available_kw.is_nan() { Value::Null } else { json!(r.available_kw) },
                "breaker_a": r.breaker,
                "connection_mm2": conn,
                "entry_mm2": {"phase": r.entry.0, "neutral": r.entry.1},
                "earthing_mm2": r.earth,
                "measurement": r.measurement,
                "junction_box": r.junction_box,
            });
            (cat, json!(k.rows.iter().map(|r| r.name).collect::<Vec<_>>()), Value::Null)
        }
        None if kw > 75.0 && network != Network::Submersible => (
            Value::Null,
            json!([]),
            json!("above 75 kW the unit is served at primary voltage (item 7.3.2), which this specification does not cover"),
        ),
        None => {
            let last = match network {
                Network::Aerial => 63,
                Network::Aerial120240 => 11,
                Network::Underground => 72,
                Network::Submersible => 100,
            };
            (
                Value::Null,
                json!([]),
                json!(format!("above the last category of Anexo {}: up to {last} kW of installed load", network.annex())),
            )
        }
    };
    Ok(json!({
        "utility": utility,
        "source": source,
        "annex": network.annex(),
        "network": network.name(),
        "installed_kw": kw,
        "phases": phases,
        "category": category,
        "candidates": candidates,
        "category_note": note,
        "demand_kva": demand_kva,
        "demand_note": note_demand,
        "not_covered": not_covered,
    }))
}

fn demand(_: &mut Session, p: &Value) -> R {
    use crate::demand::{self as d, Air, Load, Motor, OtherUse, SPECIAL_KINDS, Special, SpecialKind, System, USES};
    let c = "demand";
    if !p.is_object() {
        return Err(bad(c, "params must be an object"));
    }
    only(
        c,
        p,
        &[
            "utility",
            "building",
            "system",
            "area",
            "outlets",
            "lighting_points",
            "lighting_outlets",
            "heaters",
            "boilers",
            "appliances",
            "stoves",
            "air_conditioners",
            "air_use",
            "motors",
            "special",
            "hydromassage",
            "network",
            "installed_kw",
            "demand_current_a",
            "phases",
            "demand_kva",
        ],
    )?;
    let utility = string(c, p, "utility")?.ok_or_else(|| bad(c, format!("`utility` is required: {}", d::UTILITIES.join(", "))))?;
    let Some(source) = d::source(&utility) else {
        return Err(bad(
            c,
            format!(
                "there are no tables for `{utility}` yet, only {}: they are the utility's own, and are added from its standard",
                d::UTILITIES.join(", ")
            ),
        ));
    };
    let residential = choice(c, p, "building", &["residential", "other"], "residential")? == "residential";
    if utility == "ENEL" {
        return demand_enel(p, &utility, source);
    }
    if let Some(k) = ["network", "installed_kw", "demand_current_a", "phases", "demand_kva"].iter().find(|k| p.get(**k).is_some()) {
        return Err(bad(
            c,
            format!("`{k}` is for utility ENEL: {utility}'s standard takes the loads of its examples, not a category by installed load"),
        ));
    }
    let system = System::parse(choice(c, p, "system", &["127/220", "220/380"], "127/220")?).unwrap_or(System::S127_220);
    let area = match p.get("area").filter(|v| !v.is_null()) {
        None => None,
        Some(_) => Some(within(c, p, "area", 0.0, 0.0, 1.0e6)?),
    };
    let count = |v: &Value, key: &str, hi: f64| -> Result<u32, String> {
        match v.get(key).filter(|x| !x.is_null()) {
            None => Ok(1),
            Some(x) => x
                .as_u64()
                .filter(|n| (*n as f64) <= hi)
                .map(|n| n as u32)
                .ok_or_else(|| bad(c, format!("`{key}` must be a whole number up to {hi}, got {x}"))),
        }
    };
    let whole = |key: &str, default: u32| -> Result<u32, String> {
        match p.get(key).filter(|v| !v.is_null()) {
            None => Ok(default),
            Some(x) => x
                .as_u64()
                .filter(|n| *n <= 100_000)
                .map(|n| n as u32)
                .ok_or_else(|| bad(c, format!("`{key}` must be a whole number up to 100000, got {x}"))),
        }
    };
    let outlets = match p.get("outlets").filter(|v| !v.is_null()) {
        None => None,
        Some(_) => Some(whole("outlets", 0)?),
    };
    let lighting_points = whole("lighting_points", 0)?;
    let list = |key: &str| -> Result<Vec<Value>, String> {
        match p.get(key).filter(|v| !v.is_null()) {
            None => Ok(Vec::new()),
            Some(Value::Array(a)) if a.len() <= 1000 => Ok(a.clone()),
            Some(v) => Err(bad(c, format!("`{key}` must be a list of at most 1000 items, got {v}"))),
        }
    };
    let loads = |key: &str, field: &str| -> Result<Vec<Load>, String> {
        list(key)?
            .iter()
            .map(|v| {
                let o = v.as_object().ok_or_else(|| bad(c, format!("`{key}` holds objects, got {v}")))?;
                if let Some(k) = o.keys().find(|k| ![field, "count"].contains(&k.as_str())) {
                    return Err(bad(c, format!("`{key}`: unknown parameter `{k}`")));
                }
                Ok(Load {
                    w: within(c, v, field, f64::NAN, 0.0, 1.0e8).map_err(|e| format!("{e} (in `{key}`)"))?,
                    count: count(v, "count", 100_000.0)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(|l| {
                if l.iter().any(|x| x.w.is_nan()) { Err(bad(c, format!("`{key}`: `{field}` (watts) is required for each"))) } else { Ok(l) }
            })
    };
    let other = match p.get("lighting_outlets").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => {
            let o = v.as_object().ok_or_else(|| bad(c, "`lighting_outlets` is {use, kw, pf?}"))?;
            if let Some(k) = o.keys().find(|k| !["use", "kw", "pf"].contains(&k.as_str())) {
                return Err(bad(c, format!("`lighting_outlets`: unknown parameter `{k}`")));
            }
            let kind = choice(c, v, "use", USES, "")?;
            if kind.is_empty() {
                return Err(bad(c, format!("`lighting_outlets`: `use` is one of {}", USES.join(", "))));
            }
            let kw = within(c, v, "kw", f64::NAN, 0.0, 1.0e6)?;
            if kw.is_nan() {
                return Err(bad(c, "`lighting_outlets`: `kw` is required"));
            }
            Some(OtherUse { kind: kind.to_string(), kw, pf: within(c, v, "pf", 1.0, 0.1, 1.0)? })
        }
    };
    let mut air = Vec::new();
    for v in list("air_conditioners")? {
        let o = v.as_object().ok_or_else(|| bad(c, format!("`air_conditioners` holds objects, got {v}")))?;
        if let Some(k) = o.keys().find(|k| !["btu", "va", "w", "count"].contains(&k.as_str())) {
            return Err(bad(c, format!("`air_conditioners`: unknown parameter `{k}`")));
        }
        let n = count(&v, "count", 100_000.0)?;
        if o.get("btu").is_some_and(|x| !x.is_null()) {
            let btu = within(c, &v, "btu", 0.0, 0.0, 1.0e6)?;
            let (va, w) = d::window_air(btu).ok_or_else(|| {
                bad(
                    c,
                    format!("`btu` {btu} is not in Tabela 8: 7100, 8500, 10000, 12000, 14000, 18000, 21000 or 30000; or give `va` and `w`"),
                )
            })?;
            air.push(Air { va, w, count: n });
        } else {
            let (va, w) = (within(c, &v, "va", f64::NAN, 0.0, 1.0e8)?, within(c, &v, "w", f64::NAN, 0.0, 1.0e8)?);
            if va.is_nan() || w.is_nan() {
                return Err(bad(c, "`air_conditioners`: give `btu` (Tabela 8), or `va` and `w`"));
            }
            air.push(Air { va, w, count: n });
        }
    }
    let mut motors = Vec::new();
    for v in list("motors")? {
        let o = v.as_object().ok_or_else(|| bad(c, format!("`motors` holds objects, got {v}")))?;
        if let Some(k) = o.keys().find(|k| !["cv", "phases", "kva", "kw", "count"].contains(&k.as_str())) {
            return Err(bad(c, format!("`motors`: unknown parameter `{k}`")));
        }
        let n = count(&v, "count", 100_000.0)?;
        if o.get("cv").is_some_and(|x| !x.is_null()) {
            let cv = within(c, &v, "cv", 0.0, 0.0, 1000.0)?;
            let phases = match v.get("phases").and_then(Value::as_u64) {
                Some(x @ (1 | 3)) => x as u32,
                _ => return Err(bad(c, "`motors`: `phases` is 1 or 3 with `cv`")),
            };
            let (kva, kw) = d::motor(cv, phases).ok_or_else(|| {
                bad(c, format!("no {cv} cv {}-phase motor in Tabela {}; give `kva` and `kw`", phases, if phases == 1 { 14 } else { 15 }))
            })?;
            motors.push(Motor { kva, kw, count: n });
        } else {
            let (kva, kw) = (within(c, &v, "kva", f64::NAN, 0.0, 1.0e6)?, within(c, &v, "kw", f64::NAN, 0.0, 1.0e6)?);
            if kva.is_nan() || kw.is_nan() {
                return Err(bad(c, "`motors`: give `cv` and `phases` (Tabela 14 or 15), or `kva` and `kw`"));
            }
            motors.push(Motor { kva, kw, count: n });
        }
    }
    let mut special = Vec::new();
    for v in list("special")? {
        let o = v.as_object().ok_or_else(|| bad(c, format!("`special` holds objects, got {v}")))?;
        if let Some(k) = o.keys().find(|k| !["kind", "kw", "count"].contains(&k.as_str())) {
            return Err(bad(c, format!("`special`: unknown parameter `{k}`")));
        }
        let kind = SpecialKind::parse(choice(c, &v, "kind", SPECIAL_KINDS, "")?)
            .ok_or_else(|| bad(c, format!("`special`: `kind` is one of {}", SPECIAL_KINDS.join(", "))))?;
        let kw = within(c, &v, "kw", f64::NAN, 0.0, 1.0e6)?;
        if kw.is_nan() {
            return Err(bad(c, "`special`: `kw` is required"));
        }
        special.push(Special { kind, kw, count: count(&v, "count", 100_000.0)? });
    }
    let input = d::Input {
        residential,
        area,
        outlets,
        lighting_points,
        other,
        heaters: loads("heaters", "w")?,
        boilers: loads("boilers", "w")?,
        appliances: loads("appliances", "w")?,
        stoves: loads("stoves", "w")?,
        air,
        motors,
        special,
        hydro: loads("hydromassage", "w")?,
        commercial_air: choice(c, p, "air_use", &["residential", "commercial"], "residential")? == "commercial",
    };
    let o = d::compute(&input).map_err(|e| bad(c, e))?;
    let r6 = |x: f64| (x * 1.0e6).round() / 1.0e6 + 0.0;
    let cat = d::category(system, o.installed_kw, o.demand_kva);
    let (category, note) = match cat {
        Some(k) => (
            json!({"name": k.name, "breaker_a": k.breaker, "cable_mm2": k.cable, "conduit_mm": k.conduit, "earthing_mm2": k.earth, "phases": k.phases}),
            Value::Null,
        ),
        None if o.installed_kw > 75.0 => {
            (Value::Null, json!("above 75 kW the utility serves at primary voltage, which this standard does not cover"))
        }
        None => (Value::Null, json!("the demand is above the last category of the table")),
    };
    let t = &o.terms;
    Ok(json!({
        "utility": utility,
        "source": source,
        "installed_kw": r6(o.installed_kw),
        "installed_kw_rounded_up": (o.installed_kw - 1.0e-9).ceil(),
        "lighting_outlets_w": r6(o.lighting_outlets_w),
        "terms_kva": {"a": r6(t.a), "b": r6(t.b), "c": r6(t.c), "d": r6(t.d), "e": r6(t.e), "f": r6(t.f), "g": r6(t.g), "h": r6(t.h), "i": r6(t.i)},
        "demand_kva": r6(o.demand_kva),
        "demand_kva_rounded_up": (o.demand_kva - 1.0e-9).ceil(),
        "category": category,
        "category_note": note,
        "system": if system == System::S127_220 { "127/220" } else { "220/380" },
        "not_covered": "the other columns of Tabela 1 (poles, pontalete, ramal de ligação), the cables of Tabelas 1D and 1E, the starting of motors (Tabela 13) and the DPS and other requirements of the entry",
    }))
}

fn dps_check(_: &mut Session, p: &Value) -> R {
    use crate::dps::{self, EARTHINGS, Earthing, PURPOSES, Purpose, REGIONS, Region, SIGNALS};
    let c = "dps_check";
    if !p.is_object() {
        return Err(bad(c, "params must be an object"));
    }
    only(
        c,
        p,
        &[
            "region",
            "overhead",
            "accepted_risk",
            "risk_to_people",
            "purpose",
            "earthing",
            "neutral",
            "phases",
            "existing",
            "signal_lines",
            "uo",
            "system",
            "before_dr",
        ],
    )?;
    let Some(region) = Region::parse(choice(c, p, "region", REGIONS, "")?) else {
        return Err(bad(c, format!("`region` is required: {}", REGIONS.join(", "))));
    };
    let line = dps::Line {
        overhead: flag(c, p, "overhead")?,
        region,
        accepted_risk: flag(c, p, "accepted_risk")?,
        risk_to_people: flag(c, p, "risk_to_people")?,
    };
    let purpose = Purpose::parse(choice(c, p, "purpose", PURPOSES, "line")?).unwrap_or(Purpose::Line);
    let earthing = match choice(c, p, "earthing", EARTHINGS, "")? {
        "" => None,
        name => Earthing::parse(name),
    };
    let neutral = if absent(p.get("neutral")) { true } else { flag(c, p, "neutral")? };
    let phases = match p.get("phases").filter(|v| !v.is_null()) {
        None => 3,
        Some(v) => match v.as_u64() {
            Some(n @ (1 | 3)) => n as u32,
            _ => return Err(bad(c, format!("`phases` is 1 or 3, got {v}"))),
        },
    };
    let existing = flag(c, p, "existing")?;
    let before_dr = flag(c, p, "before_dr")?;
    let system = match p.get("system").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => match v.as_str().filter(|x| dps::withstand(x).is_some()) {
            Some(x) => Some(x.to_string()),
            None => return Err(bad(c, format!("`system` is one of {}, got {v}", dps::SYSTEMS.join(", ")))),
        },
    };
    let uo = match p.get("uo").filter(|v| !v.is_null()) {
        None => None,
        Some(_) => Some(within(c, p, "uo", 0.0, 1.0, 1.0e5)?),
    };
    let mut signals = Vec::new();
    match p.get("signal_lines").filter(|v| !v.is_null()) {
        None => {}
        Some(Value::Array(a)) if a.len() <= 1000 => {
            for v in a {
                let kind = v.as_str().filter(|k| dps::signal_required(k).is_some());
                let Some(kind) = kind else {
                    return Err(bad(c, format!("`signal_lines` holds {}, got {v}", SIGNALS.join(", "))));
                };
                signals.push(json!({
                    "kind": kind,
                    "required": true,
                    "why": dps::signal_required(kind),
                    "where": "at the building's entry or exit",
                    "min_impulse_withstand_kv": dps::SIGNAL_ENTRY_WITHSTAND_KV,
                }));
            }
        }
        Some(v) => return Err(bad(c, format!("`signal_lines` is a list of {}, got {v}", SIGNALS.join(", ")))),
    }
    let v = dps::required(&line);
    let mut power = Map::new();
    power.insert("required".into(), json!(v.required));
    power.insert("exempt".into(), json!(v.exempt));
    power.insert("reasons".into(), json!(v.reasons));
    if v.required {
        power.insert("purpose".into(), json!(PURPOSES[purpose as usize]));
        power.insert("where".into(), json!(dps::place(purpose, existing)));
        power.insert(
            "short_circuit".into(),
            json!("the DPS withstands the short-circuit current presumed where it is installed, with the overcurrent device that goes with it, and a DPS with spark gaps interrupts the follow current of that point (6.3.5.2.4 e)"),
        );
        if let Some(w) = system.as_deref().and_then(|x| dps::withstand(x).map(|w| (x, w))) {
            power.insert(
                "up".into(),
                json!({
                    "system": w.0,
                    "withstand_kv": {"IV": w.1[0], "III": w.1[1], "II": w.1[2], "I": w.1[3]},
                    "max_up_kv": w.1[2],
                    "why": "the protection level Up must stay under what the equipment of use (category II of Tabela 31) withstands",
                }),
            );
        }
        let mut three = false;
        if let Some(e) = earthing {
            let list = if before_dr { dps::schemes_before_dr(e, neutral) } else { dps::schemes(e, neutral) };
            three = list.contains(&3);
            let text: Vec<Value> = list.iter().map(|s| json!({"scheme": s, "connects": dps::scheme_text(*s, e)})).collect();
            let modes: Vec<Value> = dps::uc_min(e, neutral, uo.unwrap_or(1.0))
                .iter()
                .map(|u| match uo {
                    Some(_) => json!({"between": u.between, "formula": u.formula, "volts": (u.volts * 100.0).round() / 100.0}),
                    None => json!({"between": u.between, "formula": u.formula}),
                })
                .collect();
            power.insert("uc_min".into(), json!({"uo_v": uo, "modes": modes}));
            match dps::subsequent_current(e) {
                Some(dps::Subsequent::AtLeastA(a)) => {
                    power.insert("subsequent_current".into(), json!({"neutral_pe_min_a": a}));
                }
                Some(dps::Subsequent::LikePhaseNeutral) => {
                    power.insert("subsequent_current".into(), json!({"neutral_pe": "the same as the DPS between phase and neutral"}));
                }
                None => {}
            }
            power.insert(
                "connection".into(),
                json!({"earthing": EARTHINGS[e as usize], "neutral": neutral, "schemes": list, "connects": text}),
            );
        }
        let extra = |each: f64, special: f64| {
            let mut o = Map::new();
            o.insert("each_mode".into(), json!(each));
            if three {
                o.insert("neutral_to_pe_in_scheme_3".into(), json!(special));
            }
            Value::Object(o)
        };
        if purpose != Purpose::Direct {
            power.insert("discharge_ka".into(), extra(dps::discharge(purpose, false, phases), dps::discharge(purpose, true, phases)));
        }
        if purpose != Purpose::Line {
            power.insert("impulse_ka".into(), extra(dps::impulse(false, phases), dps::impulse(true, phases)));
        }
        power.insert("conductor".into(), json!({"section_mm2": dps::conductor_section(purpose), "max_length_m": dps::CONDUCTOR_LENGTH}));
    }
    Ok(json!({
        "power": power,
        "signal": signals,
        "not_covered": "the overcurrent device of the DPS and where it stands (6.3.5.2.5), and the coordination between DPS",
    }))
}

fn load_points(s: &mut Session, p: &Value) -> R {
    use crate::loads::{self, KINDS, Kind};
    let c = "load_points";
    if !p.is_object() {
        return Err(bad(c, "params must be an object"));
    }
    only(c, p, &["rooms", "alternative"])?;
    let alternative = flag(c, p, "alternative")?;
    let round = |x: f64| (x * 100.0).round() / 100.0 + 0.0;
    // (name, kind, area m2, perimeter m, points, kind given)
    let mut input: Vec<(String, Kind, f64, f64, Option<u32>, bool)> = Vec::new();
    let mut skipped = Vec::new();
    match p.get("rooms").filter(|v| !absent(Some(v))) {
        None => {
            let m = &s.model;
            for (id, e) in m.elements.iter() {
                let Element::Room(r) = e else { continue };
                match m.room_region_at(r, RoomArea::Finish) {
                    Some(g) => input.push((r.name.clone(), loads::kind_of(&r.name), g.area() / 1.0e6, g.perimeter() / 1000.0, None, false)),
                    None => skipped.push(json!({"id": id, "name": r.name, "why": "not enclosed by walls"})),
                }
            }
        }
        Some(Value::Array(a)) if a.len() <= 5000 => {
            for (i, r) in a.iter().enumerate() {
                let at = |e: String| format!("rooms[{i}]: {e}");
                let o = r.as_object().ok_or_else(|| bad(c, format!("rooms[{i}] must be an object")))?;
                if let Some(k) = o.keys().find(|k| !["name", "area", "perimeter", "kind", "points"].contains(&k.as_str())) {
                    return Err(bad(c, format!("rooms[{i}]: unknown parameter `{k}`")));
                }
                let name = string(c, r, "name").map_err(at)?.unwrap_or_default();
                if name.chars().count() > MAX_TEXT {
                    return Err(bad(c, format!("rooms[{i}]: `name` is longer than {MAX_TEXT} characters")));
                }
                let area = within(c, r, "area", f64::NAN, 0.0, 1.0e6).map_err(at)?;
                let perimeter = within(c, r, "perimeter", f64::NAN, 0.0, MAX_COORD).map_err(at)?;
                if area.is_nan() || perimeter.is_nan() {
                    return Err(bad(c, format!("rooms[{i}]: `area` (m²) and `perimeter` (mm) are required")));
                }
                let given = choice(c, r, "kind", KINDS, "").map_err(at)?;
                let points = match r.get("points") {
                    v if absent(v) => None,
                    Some(v) => Some(
                        v.as_u64()
                            .filter(|n| *n <= 1000)
                            .ok_or_else(|| bad(c, format!("rooms[{i}]: `points` must be a whole number from 0 to 1000, got {v}")))?
                            as u32,
                    ),
                    None => None,
                };
                let kind = Kind::parse(given).unwrap_or_else(|| loads::kind_of(&name));
                input.push((name, kind, area, perimeter / 1000.0, points, !given.is_empty()));
            }
        }
        Some(v) => return Err(bad(c, format!("`rooms` must be a list of at most 5000 rooms, got {v}"))),
    }
    let rooms: Vec<loads::Room> =
        input.iter().map(|r| loads::Room { name: &r.0, kind: r.1, area: r.2, perimeter: r.3, provided: r.4 }).collect();
    let t = loads::total(&rooms, alternative);
    let mut list = Vec::new();
    for (r, sh) in input.iter().zip(&t.rooms) {
        let mut o = Map::new();
        o.insert("name".into(), json!(r.0));
        o.insert("kind".into(), json!(r.1.key()));
        o.insert("area_m2".into(), json!(round(r.2)));
        o.insert("perimeter_m".into(), json!(round(r.3)));
        o.insert("lighting_points".into(), json!(1));
        o.insert("lighting_va".into(), json!(sh.lighting_va));
        o.insert("tug_minimum".into(), json!(sh.minimum));
        o.insert("tug_points".into(), json!(sh.points));
        o.insert("tug_va".into(), json!(sh.tug_va));
        if sh.counter_outlets > 0 {
            o.insert("counter_outlets".into(), json!(sh.counter_outlets));
        }
        o.insert("ok".into(), json!(sh.points >= sh.minimum));
        if r.1 == Kind::Other && !r.5 {
            o.insert("note".into(), json!("the name says no kind of room: treated as `other`; give `kind`"));
        }
        list.push(Value::Object(o));
    }
    Ok(json!({
        "rooms": list,
        "skipped": skipped,
        "lighting_va": t.lighting_va,
        "tug_va": t.tug_va,
        "total_va": t.lighting_va + t.tug_va,
        "wet_points": t.wet_points,
        "alternative_possible": t.alternative_possible,
        "alternative_applied": t.alternative_applied,
        "ok": t.rooms.iter().all(|sh| sh.points >= sh.minimum),
        "not_covered": "the exceptions to the lighting point and to a veranda's outlet, the placement of the outlets (near the lavatory, over a counter, up to 0.80 m outside the door of a room of 2.25 m² or less) and the TUE",
    }))
}

fn dr_check(_: &mut Session, p: &Value) -> R {
    use crate::dr::{self, KINDS, PLACES, Place};
    let c = "dr_check";
    if !p.is_object() {
        return Err(bad(c, "params must be an object"));
    }
    only(c, p, &["circuits", "building"])?;
    let residential = choice(c, p, "building", &["residential", "other"], "residential")? == "residential";
    let list = match p.get("circuits") {
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= 5000 => a,
        _ => return Err(bad(c, "`circuits` is a list of 1 to 5000 circuits, each {name, points: [...]}")),
    };
    let mut out = Vec::new();
    let mut missing = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let at = |e: String| format!("circuits[{i}]: {e}");
        let o = item.as_object().ok_or_else(|| bad(c, format!("circuits[{i}] must be an object")))?;
        if let Some(k) = o.keys().find(|k| !["name", "points", "residual", "it_continuity"].contains(&k.as_str())) {
            return Err(bad(c, format!("circuits[{i}]: unknown parameter `{k}`")));
        }
        let name = string(c, item, "name").map_err(at)?.unwrap_or_else(|| format!("#{}", i + 1));
        if name.chars().count() > MAX_TEXT {
            return Err(bad(c, format!("circuits[{i}]: `name` is longer than {MAX_TEXT} characters")));
        }
        let pts = match item.get("points") {
            Some(Value::Array(a)) if !a.is_empty() && a.len() <= 1000 => a,
            _ => return Err(bad(c, format!("circuits[{i}]: `points` is a list of 1 to 1000 points"))),
        };
        let mut points = Vec::new();
        for (j, pt) in pts.iter().enumerate() {
            let at = |e: String| format!("circuits[{i}].points[{j}]: {e}");
            let po = pt.as_object().ok_or_else(|| bad(c, at("it must be an object".into())))?;
            if let Some(k) = po.keys().find(|k| !["place", "kind", "rating", "height", "outdoor"].contains(&k.as_str())) {
                return Err(bad(c, at(format!("unknown parameter `{k}`"))));
            }
            let place = Place::parse(choice(c, pt, "place", PLACES, "").map_err(at)?);
            let kind = dr::Kind::parse(choice(c, pt, "kind", KINDS, "").map_err(at)?);
            let (Some(place), Some(kind)) = (place, kind) else {
                return Err(bad(c, at(format!("`place` ({}) and `kind` ({}) are required", PLACES.join(", "), KINDS.join(", ")))));
            };
            let optional = |key: &str, hi: f64| -> Result<Option<f64>, String> {
                match pt.get(key).filter(|v| !v.is_null()) {
                    None => Ok(None),
                    Some(_) => within(c, pt, key, 0.0, 0.0, hi).map(Some).map_err(at),
                }
            };
            points.push(dr::Point {
                place,
                kind,
                rating: optional("rating", 1.0e6)?,
                height: optional("height", MAX_COORD)?,
                outdoor: flag(c, pt, "outdoor").map_err(at)?,
            });
        }
        let residual = match item.get("residual").filter(|v| !v.is_null()) {
            None => None,
            Some(_) => Some(within(c, item, "residual", 0.0, 0.0, 1.0e6).map_err(at)?),
        };
        let circuit = dr::Circuit { points, residual, it_continuity: flag(c, item, "it_continuity").map_err(at)? };
        let v = dr::check(&circuit, residential);
        if v.ok == Some(false) {
            missing.push(name.clone());
        }
        out.push(json!({
            "name": name,
            "required": v.required,
            "exempt": v.exempt,
            "reasons": v.reasons,
            "residual_ma": residual,
            "ok": v.ok,
        }));
    }
    Ok(json!({
        "circuits": out,
        "missing": missing,
        "max_residual_ma": dr::MAX_RESIDUAL,
        "building": if residential { "residential" } else { "other" },
        "not_covered": "the further rules for the volumes of a bathroom and for swimming pools, and the other protections against shock, which the device does not replace",
    }))
}

fn hydrant_size(_: &mut Session, p: &Value) -> R {
    use crate::hydrant::{self, MATERIALS, MAX_NOZZLE_HEAD, MAX_SPEED, TYPE_KEYS};
    let c = "hydrant_size";
    let o = p.as_object().ok_or_else(|| bad(c, "params must be an object"))?;
    const KEYS: &[&str] = &["region", "column", "area", "type", "hydrants", "inside", "nominal", "material", "length", "rise"];
    if let Some(key) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    if string(c, p, "region")?.is_some_and(|r| r != "SP") {
        return Err(bad(
            c,
            "`region` must be SP: only IT 22/2025 of the Corpo de Bombeiros of São Paulo is covered, no other state's rule",
        ));
    }
    let round = |x: f64, k: f64| (x * k).round() / k + 0.0;
    let (has_column, has_area) = (!absent(p.get("column")), !absent(p.get("area")));
    let mut system: Option<(u8, f64)> = None;
    if has_column || has_area {
        if !has_column {
            return Err(bad(c, "`column` (1 to 5, of IT 22's Tabela 3) is required with `area`"));
        }
        if !has_area {
            return Err(bad(c, "`area` (m², built) is required with `column`"));
        }
        let column = within(c, p, "column", 1.0, 1.0, 5.0)?;
        if column.fract() != 0.0 {
            return Err(bad(c, "`column` must be a whole number from 1 to 5"));
        }
        let area = within(c, p, "area", 0.0, f64::MIN_POSITIVE, 1.0e8)?;
        system = hydrant::system_for(column as u8, area);
    }
    let mut out = serde_json::Map::new();
    if let Some((t, rti)) = system {
        out.insert("system".into(), json!(t.to_string()));
        out.insert("reserve_m3".into(), json!(rti));
    }
    let key = if absent(p.get("type")) {
        match system {
            Some((4, _)) if !absent(p.get("inside")) => {
                return Err(bad(c, "the system is type 4: give `type` as 4-40 or 4-65, by the nozzle"));
            }
            Some((t, _)) if t != 4 => Some(t.to_string()),
            Some(_) => None,
            None => return Err(bad(c, "`type` (1, 2, 3, 4-40, 4-65 or 5) or `column` and `area` is required")),
        }
    } else {
        let key = choice(c, p, "type", TYPE_KEYS, "")?;
        if let (Some((t, _)), Some(row)) = (system, hydrant::kind(key))
            && row.1 != t
        {
            return Err(bad(
                c,
                format!("`type` {key} contradicts `column` {} and `area`, which give type {t}: give one, or make them agree", p["column"]),
            ));
        }
        Some(key.to_string())
    };
    let Some(key) = key else { return Ok(Value::Object(out)) };
    let hydrants = within(c, p, "hydrants", 2.0, 1.0, 1000.0)?;
    if hydrants.fract() != 0.0 {
        return Err(bad(c, "`hydrants` must be a whole number from 1 to 1000"));
    }
    let Some(t) = hydrant::kind(&key) else { return Err(bad(c, format!("no system type `{key}`"))) };
    let flow = hydrant::design_flow(&key, hydrants as u32);
    out.insert("type".into(), json!(key));
    out.insert("flow_lpm".into(), json!(flow));
    out.insert("nozzle_head_mca".into(), json!(t.6));
    out.insert("hose_dn_mm".into(), json!(t.3));
    out.insert("hose_length_m".into(), json!(t.4));
    let mut ok = t.6 <= MAX_NOZZLE_HEAD;
    if absent(p.get("inside")) {
        if !absent(p.get("nominal")) {
            return Err(bad(c, "`nominal` needs the pipe's `inside` diameter (mm)"));
        }
        return Ok(Value::Object(out));
    }
    let inside = within(c, p, "inside", 0.0, f64::MIN_POSITIVE, 5000.0)?;
    let material = choice(c, p, "material", &MATERIALS.iter().map(|m| m.0).collect::<Vec<_>>(), "steel")?;
    let cf = hydrant::hazen_c(material).unwrap_or(120.0);
    let length = within(c, p, "length", 0.0, 0.0, MAX_COORD)? / 1000.0;
    let rise = within(c, p, "rise", 0.0, -MAX_COORD, MAX_COORD)? / 1000.0;
    let (v, j) = (hydrant::speed(flow, inside), hydrant::hazen_j(flow, cf, inside));
    let loss = j * length;
    let speed_ok = v <= MAX_SPEED + 1e-9;
    ok = ok && speed_ok;
    out.insert("speed_ms".into(), json!(round(v, 100.0)));
    out.insert("speed_ok".into(), json!(speed_ok));
    out.insert("head_loss_m_per_m".into(), json!(round(j, 1.0e5)));
    out.insert("loss_mca".into(), json!(round(loss, 100.0)));
    out.insert("need_mca".into(), json!(round(t.6 + loss + rise, 100.0)));
    out.insert(
        "need_covers".into(),
        json!("nozzle head + head loss of the straight pipe + rise: not the hose, the fittings or the pressure at the source"),
    );
    if !absent(p.get("nominal")) {
        let nominal = within(c, p, "nominal", 0.0, f64::MIN_POSITIVE, 5000.0)?;
        let size_ok = nominal + 1e-9 >= hydrant::min_nominal(&key);
        ok = ok && size_ok;
        out.insert("size_ok".into(), json!(size_ok));
    }
    out.insert("ok".into(), json!(ok));
    out.insert(
        "ok_covers".into(),
        json!("nozzle head within 100 mca, speed within 5 m/s and the pipe's size only: it is not a verdict on the design"),
    );
    Ok(Value::Object(out))
}

fn water_size(_: &mut Session, p: &Value) -> R {
    use crate::water::{self, FITTINGS, FIXTURES, MAX_SPEED, MIN_USE_KPA, Method};
    let c = "water_size";
    let o = p.as_object().ok_or_else(|| bad(c, "params must be an object"))?;
    const KEYS: &[&str] = &["fixtures", "flow", "method", "dn", "length", "fittings", "pressure", "rise"];
    if let Some(key) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    let whole = |key: &str, kind: &str, n: &Value| {
        n.as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 10_000.0 && n.fract() == 0.0)
            .ok_or_else(|| bad(c, format!("`{key}`: `{kind}` must be a whole number from 0 to 10000, got {n}")))
    };
    let method = Method::parse(choice(c, p, "method", &["weights", "sum"], "weights")?).unwrap_or(Method::Weights);
    let flow = match (absent(p.get("flow")), p.get("fixtures")) {
        (false, f) if !absent(f) => return Err(bad(c, "give `flow` or `fixtures`, not both")),
        (false, _) => within(c, p, "flow", 0.0, f64::MIN_POSITIVE, 1000.0)?,
        (true, Some(Value::Object(f))) => {
            let mut list: Vec<(&str, f64)> = Vec::new();
            for (kind, n) in f {
                if water::fixture(kind).is_none() {
                    return Err(bad(
                        c,
                        format!("`fixtures`: no fixture `{kind}`, use {}", FIXTURES.iter().map(|x| x.0).collect::<Vec<_>>().join(", ")),
                    ));
                }
                let n = whole("fixtures", kind, n)?;
                list.push((kind.as_str(), n));
            }
            water::design_flow(method, &list)
        }
        (true, Some(v)) if !absent(Some(v)) => return Err(bad(c, format!("`fixtures` must be an object {{kind: count}}, got {v}"))),
        _ => 0.0,
    };
    if flow <= 0.0 {
        return Err(bad(c, "`flow` (L/s) or `fixtures` is required"));
    }
    let dn = if absent(p.get("dn")) {
        water::smallest_dn(flow).ok_or_else(|| bad(c, "no DN of PVC soldável (20 to 110) keeps the speed within 3 m/s at that flow"))?
    } else {
        let dn = within(c, p, "dn", 0.0, 0.0, 1000.0)?;
        water::inside(dn).map(|_| dn).ok_or_else(|| bad(c, "`dn` must be one of 20, 25, 32, 40, 50, 60, 75, 85, 110 (PVC soldável)"))?
    };
    let inside = water::inside(dn).ok_or_else(|| bad(c, "`dn` is not a PVC soldável size"))?;
    let mut length = within(c, p, "length", 0.0, 0.0, MAX_COORD)? / 1000.0;
    match p.get("fittings") {
        v if absent(v) => {}
        Some(Value::Object(f)) => {
            for (kind, n) in f {
                let le = water::equivalent_length(dn, kind)
                    .ok_or_else(|| bad(c, format!("`fittings`: no fitting `{kind}`, use {}", FITTINGS.join(", "))))?;
                length += le * whole("fittings", kind, n)?;
            }
        }
        Some(v) => return Err(bad(c, format!("`fittings` must be an object {{fitting: count}}, got {v}"))),
        None => {}
    }
    let (speed, j) = water::head_loss(flow, inside, water::EPS_PVC);
    let loss = j * length * water::KPA_PER_M;
    let round = |x: f64, k: f64| (x * k).round() / k + 0.0;
    let mut ok = speed <= MAX_SPEED + 1e-9;
    let end = if absent(p.get("pressure")) {
        None
    } else {
        let pressure = within(c, p, "pressure", 0.0, 0.0, 1.0e5)?;
        let rise = within(c, p, "rise", 0.0, -MAX_COORD, MAX_COORD)? / 1000.0;
        let end = pressure - loss - rise * water::KPA_PER_M;
        ok = ok && end >= MIN_USE_KPA;
        Some(end)
    };
    Ok(json!({
        "flow_lps": round(flow, 1000.0),
        "dn": dn,
        "inside_mm": round(inside, 10.0),
        "speed_ms": round(speed, 100.0),
        "head_loss_m_per_m": round(j, 1.0e5),
        "equivalent_length_m": round(length, 100.0),
        "loss_kpa": round(loss, 100.0),
        "end_kpa": end.map(|e| round(e, 100.0)),
        "ok": ok,
        "ok_covers": if end.is_some() { "the speed and the pressure left at the end" } else { "the speed alone: no pressure was given" },
    }))
}

fn reservoir_size(_: &mut Session, p: &Value) -> R {
    use crate::reservoir::{self, Input, Kind, Status};
    let c = "reservoir_size";
    let o = p.as_object().ok_or_else(|| bad(c, "params must be an object"))?;
    const KEYS: &[&str] = &[
        "daily_l",
        "people",
        "per_person_l",
        "days",
        "fire_l",
        "house",
        "lower_percent",
        "compartments",
        "fill_flow",
        "pumps",
        "towers",
        "smallest_l",
        "peak_l",
    ];
    if let Some(key) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    let whole = |key: &str, lo: f64, hi: f64| -> Result<Option<u32>, String> {
        match p.get(key) {
            v if absent(v) => Ok(None),
            Some(v) => v
                .as_f64()
                .filter(|n| n.is_finite() && *n >= lo && *n <= hi && n.fract() == 0.0)
                .map(|n| Some(n as u32))
                .ok_or_else(|| bad(c, format!("`{key}` must be a whole number from {lo} to {hi}, got {v}"))),
            None => Ok(None),
        }
    };
    let optional = |key: &str, hi: f64| -> Result<Option<f64>, String> {
        if absent(p.get(key)) { Ok(None) } else { within(c, p, key, 0.0, f64::MIN_POSITIVE, hi).map(Some) }
    };
    let people = whole("people", 1.0, 1.0e6)?;
    let per_person = optional("per_person_l", 10_000.0)?;
    let (daily_l, source) = match (optional("daily_l", 1.0e9)?, people, per_person) {
        (Some(_), Some(_), _) => return Err(bad(c, "`people` and `daily_l` are two ways to give the consumption: give one")),
        (Some(_), None, Some(_)) => return Err(bad(c, "`per_person_l` goes with `people`, not with `daily_l`")),
        (Some(d), None, None) => (d, "given as `daily_l`"),
        (None, Some(n), Some(l)) => (f64::from(n) * l, "`people` times `per_person_l`"),
        (None, Some(_), None) => {
            return Err(bad(c, "`people` needs `per_person_l` (litres per person per day): the standard gives none, so it is yours"));
        }
        (None, None, Some(_)) => return Err(bad(c, "`per_person_l` needs `people`")),
        (None, None, None) => return Err(bad(c, "`daily_l` (litres a day), or `people` and `per_person_l`, is required")),
    };
    let days = within(c, p, "days", 1.0, 0.01, 365.0)?;
    let fire_l = within(c, p, "fire_l", 0.0, 0.0, 1.0e9)?;
    let lower_percent = if absent(p.get("lower_percent")) { None } else { Some(within(c, p, "lower_percent", 0.0, 0.0, 100.0)?) };
    let input = Input {
        daily_l,
        days,
        fire_l,
        house: flag(c, p, "house")?,
        lower_percent,
        compartments: whole("compartments", 0.0, 1000.0)?,
        fill_flow_lps: optional("fill_flow", 1.0e6)?,
        pumps: whole("pumps", 0.0, 1000.0)?,
        towers: whole("towers", 1.0, 1000.0)?.unwrap_or(1),
        smallest_l: optional("smallest_l", 1.0e9)?,
        peak_l: optional("peak_l", 1.0e9)?,
    };
    let r = reservoir::size(&input);
    let round = |x: f64, k: f64| (x * k).round() / k + 0.0;
    let pending: Vec<&str> = r.checks.iter().filter(|k| k.status == Status::Pending).map(|k| k.id).collect();
    Ok(json!({
        "daily_consumption_l": round(daily_l, 100.0),
        "consumption": {
            "source": source,
            "kind": Kind::Assumption.name(),
            "note": "the standard gives no litres per person per day: the value is the project's, from a source it names",
        },
        "potable_volume_l": round(r.potable_l, 100.0),
        "fire_reserve_l": round(fire_l, 100.0),
        "total_volume_l": round(r.total_l, 100.0),
        "calc": [
            {"what": "potable volume", "formula": "days x daily consumption", "value": round(r.potable_l, 100.0), "unit": "L"},
            {"what": "total volume", "formula": "potable volume + fire reserve", "value": round(r.total_l, 100.0), "unit": "L"},
            {"what": "least supply flow", "formula": "daily consumption / (refill hours x 3600)", "value": round(r.min_fill_flow_lps, 1000.0), "unit": "L/s"},
        ],
        "refill": {"hours": r.refill_hours, "min_flow_lps": round(r.min_fill_flow_lps, 1000.0)},
        "split": r.split.map(|s| json!({
            "kind": Kind::Assumption.name(),
            "lower_percent": s.lower_percent,
            "lower_l": round(s.lower_l, 100.0),
            "upper_l": round(s.upper_l, 100.0),
            "note": "the standard fixes no share between the lower and the upper reservoir: this is the project's",
        })),
        "suggested_split": {
            "kind": Kind::Practice.name(),
            "lower_percent": r.suggested_lower_percent,
            "note": "the usual share, a common practice and not a requirement; give `lower_percent` to choose",
        },
        "checks": r.checks.iter().map(|k| json!({
            "id": k.id,
            "kind": k.kind.name(),
            "status": k.status.name(),
            "text": k.text,
            "source": k.source,
        })).collect::<Vec<_>>(),
        "pending": pending,
        "ok": r.checks.iter().all(|k| k.status != Status::Fail),
        "ok_covers": "the checks that could be made: none failed; `pending` lists what an input not given left open",
        "not_covered": "the pump's head and power, the recalque pipe (Forchheimer is not the standard's and is not offered here), the fire reserve's own rules and the consumption per person",
    }))
}

fn storm_size(_: &mut Session, p: &Value) -> R {
    use crate::storm::{self, DEFAULT_RAIN, MIN_SLOPE, N_PVC, Off, Outlet};
    let c = "storm_size";
    let o = p.as_object().ok_or_else(|| bad(c, "params must be an object"))?;
    const KEYS: &[&str] = &["kind", "area", "rainfall", "flow", "outlet", "gutter", "length", "slope", "inside"];
    if let Some(key) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    let kind = choice(c, p, "kind", &["downpipe", "horizontal"], "")?;
    if kind.is_empty() {
        return Err(bad(c, "`kind` (downpipe or horizontal) is required"));
    }
    let mine: &[&str] = if kind == "downpipe" { &["outlet", "gutter", "length"] } else { &["slope", "inside"] };
    if let Some(key) = ["outlet", "gutter", "length", "slope", "inside"].into_iter().find(|k| o.contains_key(*k) && !mine.contains(k)) {
        return Err(bad(c, format!("`{key}` does not apply to a {kind}")));
    }
    if !absent(p.get("flow"))
        && let Some(key) = ["area", "rainfall"].into_iter().find(|k| o.contains_key(*k))
    {
        return Err(bad(c, format!("`{key}` and `flow` are two ways to give the flow: give one")));
    }
    let outlet = Outlet::parse(choice(c, p, "outlet", &["edge", "funnel"], "edge")?).unwrap_or(Outlet::Edge);
    let flow = if absent(p.get("flow")) {
        let area = within(c, p, "area", 0.0, 0.0, 1.0e7)?;
        if area <= 0.0 {
            return Err(bad(c, "`area` (m², the roof that drains) or `flow` (L/min) is required"));
        }
        if area > storm::DEFAULT_RAIN_MAX_AREA && absent(p.get("rainfall")) {
            return Err(bad(
                c,
                format!(
                    "`rainfall` (mm/h, from the standard's table for the place and return period) is required above {} m²: the default {DEFAULT_RAIN} mm/h is for smaller roofs",
                    storm::DEFAULT_RAIN_MAX_AREA
                ),
            ));
        }
        storm::design_flow(within(c, p, "rainfall", DEFAULT_RAIN, f64::MIN_POSITIVE, 1000.0)?, area)
    } else {
        within(c, p, "flow", 0.0, f64::MIN_POSITIVE, 1.0e9)?
    };
    let round = |x: f64, k: f64| (x * k).round() / k + 0.0;
    if kind == "downpipe" {
        if absent(p.get("gutter")) {
            return Err(bad(c, "`gutter` (the water depth in the gutter, 50 to 100 mm) is required"));
        }
        if absent(p.get("length")) {
            return Err(bad(c, "`length` (the downpipe's, mm) is required"));
        }
        let (gutter, length) = (within(c, p, "gutter", 0.0, 0.0, 10_000.0)?, within(c, p, "length", 0.0, 0.0, MAX_COORD)? / 1000.0);
        let d = storm::downpipe_diameter(outlet, flow, gutter, length).map_err(|off| {
            bad(
                c,
                match off {
                    Off::Depth => "the water depth in the gutter must be 50 to 100 mm: the abacuses stop there",
                    Off::Length => "a downpipe shorter than 0.3 m is off the abacuses",
                    Off::Diameter => "more than 150 mm inside is off the abacuses of NBR 10844's Figure 3",
                },
            )
        })?;
        return Ok(
            json!({"flow_lpm": round(flow, 100.0), "inside_mm": round(d.diameter, 10.0), "by_depth_mm": round(d.by_depth, 10.0), "by_length_mm": round(d.by_length, 10.0)}),
        );
    }
    if absent(p.get("slope")) {
        return Err(bad(c, "`slope` (a fraction, 0.005 at least) is required"));
    }
    let slope = within(c, p, "slope", 0.0, 0.0, 1.0)?;
    if slope + 1e-9 < MIN_SLOPE {
        return Err(bad(c, "`slope` must be 0.5 % (0.005) at least, NBR 10844"));
    }
    let need = storm::horizontal_diameter(flow, slope, N_PVC);
    let inside = within(c, p, "inside", 0.0, 0.0, 10_000.0)?;
    let carries = (inside > 0.0).then(|| storm::manning_flow(inside, slope, N_PVC));
    Ok(json!({
        "flow_lpm": round(flow, 100.0),
        "inside_mm": round(need, 10.0),
        "carries_lpm": carries.map(|q| round(q, 10.0)),
        "ok": carries.map(|q| q + 1e-9 >= flow),
    }))
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

fn tower(s: &mut Session, p: &Value) -> R {
    let c = "tower";
    let params = p.as_object().ok_or_else(|| bad(c, "params must be an object"))?;
    if let Some(key) = params.keys().find(|k| !["storeys", "plate", "annotation", "sheets"].contains(&k.as_str())) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    let plate = match p.get("plate") {
        v if absent(v) => crate::sample::tower::Plate::REFERENCE,
        Some(Value::String(s)) if s == "reference" => crate::sample::tower::Plate::REFERENCE,
        Some(Value::String(s)) if s == "small" => crate::sample::tower::Plate::SMALL,
        _ => return Err(bad(c, "`plate` must be `reference` or `small`")),
    };
    let max = crate::sample::tower::MAX_STOREYS;
    let storeys = count_opt(c, p, "storeys", max as usize)?;
    let storeys = u32::try_from(storeys)
        .ok()
        .filter(|n| (1..=max).contains(n))
        .ok_or_else(|| bad(c, format!("`storeys` must be between 1 and {max}")))?;
    let drawings = crate::sample::tower::Drawings { annotation: flag(c, p, "annotation")?, sheets: flag(c, p, "sheets")? };
    let model = crate::sample::tower::drawn(plate, storeys, drawings)?;
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
                let len = std::fs::metadata(path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?.len();
                if len > cloud::MAX_BYTES as u64 {
                    return Err(bad(c, format!("{} is too large ({len} bytes; up to {})", path.display(), cloud::MAX_BYTES)));
                }
                let bytes = std::fs::read(path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
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

// ---------- CAD drawings ----------

/// The file `path` names, a relative one beside the project's file: where a link or a linked drawing is read from.
fn beside(s: &Session, path: &str) -> PathBuf {
    let at = PathBuf::from(path);
    match (&s.path, at.is_relative()) {
        (Some(project), true) => project.parent().map_or(at.clone(), |dir| dir.join(&at)),
        _ => at,
    }
}

/// The bytes of the DXF file `path` (a DWG's, read by the session's DWG reader, as the DXF it becomes) and notes on what a
/// DWG's read left out, or why it can't be brought in.
fn read_dxf_file(s: &Session, path: &str) -> Result<(Vec<u8>, Vec<String>), String> {
    let full = beside(s, path);
    match ext(&full).as_str() {
        "dxf" => {}
        "dwg" => {
            let Some(read) = &s.dwg else {
                return Err(format!(
                    "{}: this BIMCraft has no DWG reader built in; save it as DXF (any version, ASCII) from the program that \
                     made it, or convert it with a DWG to DXF converter, and bring that in",
                    full.display()
                ));
            };
            return read(&full).map_err(|e| format!("{}: {e}", full.display()));
        }
        other => return Err(format!("{}: a .{other} file isn't a CAD drawing BIMCraft reads; it reads DXF", full.display())),
    }
    let len = std::fs::metadata(&full).map_err(|e| format!("{}: {e}", full.display()))?.len();
    if len > crate::dxf::MAX_BYTES as u64 {
        return Err(format!("{} is too large ({len} bytes; up to {})", full.display(), crate::dxf::MAX_BYTES));
    }
    std::fs::read(&full).map(|bytes| (bytes, Vec::new())).map_err(|e| format!("{}: {e}", full.display()))
}

/// The CAD import `id` names, by id or GUID.
fn cad_import<'m>(c: &str, m: &'m Model, p: &Value) -> Result<(Id, &'m CadImport), String> {
    let v = p.get("id").filter(|v| !v.is_null()).ok_or_else(|| bad(c, "`id`, a CAD import's id or GUID, is required"))?;
    let id = element_id(c, m, v)?;
    match m.elements.get(id) {
        Some(Element::CadImport(ci)) => Ok((id, ci)),
        Some(e) => Err(bad(c, format!("{} {id} isn't a CAD import", e.category()))),
        None => Err(bad(c, format!("no element {id}"))),
    }
}

/// `key`, a list of a drawing's layer names, each one it has.
fn layer_list(c: &str, p: &Value, key: &str, cad: &Cad) -> Result<Vec<String>, String> {
    match p.get(key) {
        v if absent(v) => Ok(Vec::new()),
        Some(Value::Array(a)) if a.len() <= crate::model::MAX_HIDDEN => a
            .iter()
            .map(|v| {
                let n = v.as_str().ok_or_else(|| bad(c, format!("`{key}` lists layers by name, not {v}")))?;
                if cad.dxf().layer(n).is_none() {
                    let names: Vec<&str> = cad.dxf().layers.iter().take(20).map(|l| l.name.as_str()).collect();
                    let more = if cad.dxf().layers.len() > 20 { "…" } else { "" };
                    return Err(bad(c, format!("the drawing has no layer `{n}` (it has: {}{more})", names.join(", "))));
                }
                Ok(n.to_string())
            })
            .collect(),
        Some(v) => Err(bad(c, format!("`{key}` is a list of layer names, of up to {}, not {v}", crate::model::MAX_HIDDEN))),
        None => Ok(Vec::new()),
    }
}

/// A drawing's layers as a reply lists them: each with its colour, its linetype, and whether it shows.
fn layers_json(cad: &Cad, hidden: &BTreeSet<String>, in_view: Option<&BTreeSet<String>>) -> Value {
    let rows: Vec<Value> = cad
        .dxf()
        .layers
        .iter()
        .map(|l| {
            let color = l.color.rgb().map(|c| json!([c.0, c.1, c.2]));
            let mut row = json!({"name": l.name, "color": color, "linetype": l.linetype, "shown": !hidden.contains(&l.name)});
            if let Some(v) = in_view {
                row["shown_in_view"] = json!(!hidden.contains(&l.name) && !v.contains(&l.name));
            }
            row
        })
        .collect();
    json!(rows)
}

/// The layers a drawing's file has off or frozen: those an import of only its visible layers hides.
fn off_layers(cad: &Cad) -> BTreeSet<String> {
    cad.dxf().layers.iter().filter(|l| l.off || l.frozen).map(|l| l.name.clone()).collect()
}

/// A DXF drawing brought in, or one the project holds placed again, as an element: Revit's Import CAD (and Link CAD).
fn import_cad(s: &mut Session, p: &Value) -> R {
    let c = "import_cad";
    let m = &s.model;
    let (level, view) = if absent(p.get("view")) {
        (level_param(c, m, p, "level")?, None)
    } else {
        let (level, plan) = plan_param(c, m, p, "view")?;
        if !absent(p.get("level")) && level_param(c, m, p, "level")? != level {
            return Err(bad(c, "`view` is a plan of another level than `level`; give the one, or a plan of the other"));
        }
        (level, Some(plan.unwrap_or(level)))
    };
    let link = flag(c, p, "link")?;
    let mut notes = Vec::new();
    let (name, cad, new) = match (string(c, p, "path")?, string(c, p, "cad")?) {
        (Some(_), Some(_)) => return Err(bad(c, "give `path`, a DXF file to bring in, or `cad`, a drawing the project holds; not both")),
        (None, None) => return Err(bad(c, "give `path`, a DXF file to bring in, or `cad`, the name of a drawing the project holds")),
        (None, Some(n)) => {
            let cad = m.cads.get(&n).cloned().ok_or_else(|| {
                let names: Vec<&str> = m.cads.keys().map(String::as_str).collect();
                let held = if names.is_empty() { "none".into() } else { names.join(", ") };
                bad(c, format!("the project holds no CAD drawing named `{n}` (it holds: {held})"))
            })?;
            (n, cad, false)
        }
        (Some(path), None) => {
            let source = Path::new(&path).file_name().map_or_else(|| path.clone(), |f| f.to_string_lossy().into_owned());
            let stem =
                Path::new(&path).file_stem().map_or_else(|| "Drawing".into(), |f| f.to_string_lossy().chars().take(MAX_TEXT).collect());
            let name = string(c, p, "name")?.unwrap_or(stem);
            if name.trim().is_empty() {
                return Err(bad(c, "a CAD drawing needs a `name`"));
            }
            if m.cads.contains_key(&name) {
                return Err(bad(
                    c,
                    format!(
                        "the project already holds a CAD drawing named `{name}`: place it again with `cad`, or bring this file in under another `name`"
                    ),
                ));
            }
            if m.cads.len() >= crate::cad::MAX_CADS {
                return Err(bad(
                    c,
                    format!("the project holds {} CAD drawings already; delete the imports of one first", crate::cad::MAX_CADS),
                ));
            }
            let (bytes, read_notes) = read_dxf_file(s, &path).map_err(|e| bad(c, e))?;
            notes = read_notes;
            let held: usize = m.cads.values().map(Cad::len).sum();
            if held + bytes.len() > crate::cad::MAX_HELD {
                return Err(bad(
                    c,
                    format!("the project's CAD drawings would hold {} bytes; it holds up to {}", held + bytes.len(), crate::cad::MAX_HELD),
                ));
            }
            let cad = Cad::new(&source, link.then_some(path.clone()), bytes).map_err(|e| bad(c, format!("{path}: {e}")))?;
            (name, cad, true)
        }
    };
    if link && !new {
        return Err(bad(c, "`link` is for a file brought in by `path`; a drawing the project holds stays as it's held"));
    }
    let dxf = cad.dxf();
    let scale = match (string(c, p, "units")?.as_deref(), num(c, p, "scale")?) {
        (Some(_), Some(_)) => return Err(bad(c, "give `units` or `scale`, not both")),
        (_, Some(k)) if crate::model::CAD_SCALE.contains(&k) => k,
        (_, Some(k)) => {
            let (lo, hi) = (crate::model::CAD_SCALE.start(), crate::model::CAD_SCALE.end());
            return Err(bad(c, format!("`scale` is mm to each of the drawing's units, from {lo} to {hi}, not {k}")));
        }
        (None | Some("auto"), None) => dxf.scale.unwrap_or(1.0),
        (Some(u), None) => cloud::unit(u).ok_or_else(|| bad(c, format!("`units` is auto, mm, cm, m, in or ft, not {u}")))?,
    };
    let bulk = cad.bulk().unwrap_or_default();
    let origin = match string(c, p, "position")?.as_deref() {
        None | Some("center" | "centre") => (bulk[0] + bulk[1]) * 0.5,
        Some("origin") => P2(0.0, 0.0),
        Some(o) => {
            return Err(bad(
                c,
                format!("`position` is center (the middle of the drawing on `at`) or origin (its origin on `at`), not {o}"),
            ));
        }
    };
    let at = point_opt(c, p, "at")?.unwrap_or(P2(0.0, 0.0));
    let angle = num(c, p, "angle")?.unwrap_or(0.0);
    let hidden: BTreeSet<String> = match p.get("layers") {
        None | Some(Value::Null) => off_layers(&cad),
        Some(Value::String(w)) if w == "visible" => off_layers(&cad),
        Some(Value::String(w)) if w == "all" => BTreeSet::new(),
        Some(Value::Array(_)) => {
            let keep: BTreeSet<String> = layer_list(c, p, "layers", &cad)?.into_iter().collect();
            dxf.layers.iter().filter(|l| !keep.contains(&l.name)).map(|l| l.name.clone()).collect()
        }
        Some(v) => {
            return Err(bad(
                c,
                format!("`layers` is visible (those the file has on and thawed), all, or a list of the layers to show, not {v}"),
            ));
        }
    };
    let colors = match string(c, p, "colors")? {
        None => Colors::Preserve,
        Some(n) => Colors::from_name(&n).ok_or_else(|| bad(c, format!("`colors` is preserve, invert or black, not {n}")))?,
    };
    // The unit it was read in: the one given, else its file's.
    let unit = match string(c, p, "units")?.filter(|u| u != "auto") {
        Some(u) => format!("{u} (its file says {})", dxf.unit),
        None if p.get("scale").is_some_and(|v| !v.is_null()) => format!("a scale of its own (its file says {})", dxf.unit),
        None if dxf.scale.is_some() => dxf.unit.clone(),
        None => format!("{} (read as mm unless `units` says)", dxf.unit),
    };
    let (entities, layers, far, left_out) = (dxf.entities.len(), dxf.layers.len(), cad.far(), dxf.left_out.clone());
    let source = cad.source().to_string();
    let m = s.model_mut();
    if new {
        m.cads.insert(name.clone(), cad);
    }
    let id = m.insert(Element::CadImport(CadImport { cad: name.clone(), level, at, origin, scale, angle, view, hidden, colors }));
    let extents = m.cad_box(id);
    let mut reply = json!({
        "id": id, "cad": name, "source": source, "unit": unit, "scale": scale, "entities": entities, "layers": layers,
        "extents": extents, "far": far, "left_out": left_out,
    });
    if !notes.is_empty() {
        reply["notes"] = json!(notes);
    }
    if far > 0 {
        reply["warning"] = json!(format!(
            "{far} of its entities lie far from the rest of the drawing: they're drawn where they are, and left out of its extents. \
             Delete them in the program that made the drawing, or hide their layers"
        ));
    }
    Ok(reply)
}

/// An import's layers, and which it shows: in every view, or in one (its Visibility/Graphics' Imported Categories).
fn cad_layers(s: &mut Session, p: &Value) -> R {
    let c = "cad_layers";
    let m = &s.model;
    let (id, ci) = cad_import(c, m, p)?;
    let cad = m.cads.get(&ci.cad).ok_or_else(|| bad(c, format!("the project holds no CAD drawing named `{}`", ci.cad)))?.clone();
    let (hide, show) = (layer_list(c, p, "hide", &cad)?, layer_list(c, p, "show", &cad)?);
    if let Some(both) = hide.iter().find(|h| show.contains(h)) {
        return Err(bad(c, format!("layer `{both}` is in `hide` and in `show`; give it in one")));
    }
    let name = ci.cad.clone();
    let mut hidden = ci.hidden.clone();
    let view = if absent(p.get("view")) { None } else { Some(view_param(c, m, p, false)?) };
    let in_view = match view {
        None => {
            hidden.extend(hide);
            for l in &show {
                hidden.remove(l);
            }
            if hidden != ci.hidden
                && let Some(Element::CadImport(ci)) = s.model_mut().elements.get_mut(id)
            {
                ci.hidden = hidden.clone();
            }
            None
        }
        Some(v) => {
            let mut g = graphics::graphics_of(m, v)
                .cloned()
                .ok_or_else(|| bad(c, format!("{} isn't a plan, a section, an elevation or a 3D view", view_title(m, v))))?;
            let was = g.layers.get(&name).cloned().unwrap_or_default();
            let mut now = was.clone();
            now.extend(hide);
            for l in &show {
                now.remove(l);
            }
            if now != was {
                not_dependent(c, m, v, "graphics")?;
                if now.is_empty() {
                    g.layers.remove(&name);
                } else {
                    g.layers.insert(name.clone(), now.clone());
                }
                g.check(m).map_err(|e| bad(c, e))?;
                set_graphics(s.model_mut(), v, g);
            }
            Some(now)
        }
    };
    Ok(json!({"id": id, "cad": name, "layers": layers_json(&cad, &hidden, in_view.as_ref())}))
}

/// A linked drawing read again from its file.
fn reload_cad(s: &mut Session, p: &Value) -> R {
    let c = "reload_cad";
    let name = string(c, p, "cad")?.ok_or_else(|| bad(c, "`cad`, the name of a linked CAD drawing, is required"))?;
    let cad = s.model.cads.get(&name).cloned().ok_or_else(|| bad(c, format!("the project holds no CAD drawing named `{name}`")))?;
    let path = cad
        .path()
        .map(str::to_string)
        .ok_or_else(|| bad(c, format!("CAD drawing `{name}` isn't linked; bring it in again with `import_cad` and `link`")))?;
    let (bytes, _) = read_dxf_file(s, &path).map_err(|e| bad(c, e))?;
    if cad.same_bytes(&bytes) {
        return Ok(json!({"cad": name, "changed": false}));
    }
    let new = Cad::new(cad.source(), Some(path.clone()), bytes).map_err(|e| bad(c, format!("{path}: {e}")))?;
    let entities = new.dxf().entities.len();
    s.model_mut().cads.insert(name.clone(), new);
    Ok(json!({"cad": name, "changed": true, "entities": entities}))
}

/// Every linked drawing read again, as opening a project does; those that couldn't be, as `{cad, error}`. A drawing whose file
/// is gone keeps what the project holds of it.
fn relink_cads(s: &mut Session) -> Vec<Value> {
    let linked: Vec<(String, String)> = s.model.cads.iter().filter_map(|(n, cad)| Some((n.clone(), cad.path()?.to_string()))).collect();
    let mut failed = Vec::new();
    for (name, path) in linked {
        let read = read_dxf_file(s, &path).and_then(|(bytes, _)| {
            let held = s.model.cads.get(&name);
            if held.is_some_and(|h| h.same_bytes(&bytes)) {
                return Ok(None);
            }
            let source = held.map_or(name.as_str(), |h| h.source());
            Cad::new(source, Some(path.clone()), bytes).map(Some)
        });
        match read {
            Ok(Some(cad)) => {
                Arc::make_mut(&mut s.model).cads.insert(name, cad);
            }
            Ok(None) => {}
            Err(error) => failed.push(json!({"cad": name, "error": error})),
        }
    }
    failed
}

/// What of an import lies nearest a point: Revit's Query.
fn cad_query(s: &mut Session, p: &Value) -> R {
    let c = "cad_query";
    let m = &s.model;
    let (id, ci) = cad_import(c, m, p)?;
    let at = point_req(c, p, "at")?;
    let reach = num(c, p, "reach")?.unwrap_or(100.0);
    if reach <= 0.0 {
        return Err(bad(c, format!("`reach` is how far from `at` to look, mm, above 0, not {reach}")));
    }
    let cad = m.cads.get(&ci.cad).ok_or_else(|| bad(c, format!("the project holds no CAD drawing named `{}`", ci.cad)))?;
    let Some((e, d)) = crate::cad::nearest(cad, ci, at, reach) else { return Ok(json!({"id": id, "found": false})) };
    let color = cad.color(e, Colors::Preserve).map(|c| json!([c.0, c.1, c.2]));
    let linetype = e.linetype.clone().or_else(|| cad.dxf().layer(&e.layer).map(|l| l.linetype.clone()));
    Ok(json!({
        "id": id, "found": true, "kind": crate::cad::kind(&e.shape), "layer": e.layer, "color": color,
        "linetype": linetype, "weight": e.weight, "distance": hundredth(d),
    }))
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

fn room_separator(s: &mut Session, p: &Value) -> R {
    let c = "room_separator";
    let mut pts = points(c, p, "points")?;
    if pts.len() < 2 {
        return Err(bad(c, "`points` needs at least 2 points"));
    }
    if flag(c, p, "closed")?
        && let Some(&first) = pts.first()
    {
        pts.push(first);
    }
    let level = level_param(c, &s.model, p, "level")?;
    let m = s.model_mut();
    let ids: Vec<Id> = pts
        .windows(2)
        .filter_map(|seg| match seg {
            [a, b] => Some(m.insert(Element::RoomSeparator(RoomSeparator { level, start: *a, end: *b }))),
            _ => None,
        })
        .collect();
    Ok(json!({"ids": ids}))
}

fn rebar_bars(s: &mut Session, p: &Value) -> R {
    let c = "rebar";
    let mut path = points3(c, p, "points")?;
    if path.len() < 2 {
        return Err(bad(c, "`points` needs at least 2 points"));
    }
    if flag(c, p, "closed")?
        && let Some(&first) = path.first()
    {
        path.push(first);
    }
    if rebar::length(&path) <= TOL {
        return Err(bad(c, "the bar is too short: its points are all in one place"));
    }
    let diameter = num(c, p, "diameter")?.ok_or_else(|| bad(c, "`diameter` in mm is required"))?;
    let (thin, thick) = rebar::DIAMETERS;
    if !(thin..=thick).contains(&diameter) {
        return Err(bad(c, format!("`diameter` {diameter} mm isn't between {thin} and {thick} mm")));
    }
    let grade = string(c, p, "grade")?.unwrap_or_else(|| rebar::GRADE.into());
    if grade.trim().is_empty() {
        return Err(bad(c, "`grade` can't be blank"));
    }
    let layer = string(c, p, "layer")?.unwrap_or_default();
    let count = count_opt(c, p, "count", 1)?;
    if !(1..=rebar::MAX_BARS).contains(&count) || count.saturating_mul(path.len()) > MAX_POINTS {
        return Err(bad(c, format!("`count` is 1 to {} bars, and the bars' points together up to {MAX_POINTS}", rebar::MAX_BARS)));
    }
    let step = match p.get("step") {
        Some(v) if !v.is_null() => point3(v).ok_or_else(|| bad(c, format!("`step` must be [dx, dy, dz] in mm, got {v}")))?,
        _ if count > 1 => return Err(bad(c, "`step` [dx, dy, dz] is required for more than one bar")),
        _ => [0.0; 3],
    };
    let m = &s.model;
    let host = match id_opt(c, p, "host")? {
        Some(h) => Some(m.elements.id(h).ok_or_else(|| bad(c, format!("no element {h}")))?),
        None => None,
    };
    // Its host's level, unless told another.
    let hosted_at = host.and_then(|h| m.elements.get(h)).and_then(|e| m.level_of(e));
    let level = match hosted_at {
        Some(l) if absent(p.get("level")) => l,
        _ => level_param(c, m, p, "level")?,
    };
    let host = host.map(|h| m.guid(h));
    let m = s.model_mut();
    let ids: Vec<Id> = (0..count)
        .map(|k| {
            let k = k as f64;
            let path = path.iter().map(|q| [q[0] + k * step[0], q[1] + k * step[1], q[2] + k * step[2]]).collect();
            m.insert(Element::Rebar(Rebar { level, host, path, diameter, grade: grade.clone(), layer: layer.clone() }))
        })
        .collect();
    Ok(json!({"ids": ids}))
}

fn reinforce_host(s: &mut Session, p: &Value) -> R {
    let c = "reinforce";
    let m = &s.model;
    // A preset's parameters, which what is given with it replaces.
    let preset = string(c, p, "preset")?;
    let merged;
    let (p, kind) = match &preset {
        None => (p, None),
        Some(name) => {
            let pre = m.reinforcement.get(name).ok_or_else(|| bad(c, format!("there's no reinforcement preset `{name}`")))?;
            let mut all = pre.params.clone();
            all.extend(p.as_object().into_iter().flatten().filter(|(k, _)| k.as_str() != "preset").map(|(k, v)| (k.clone(), v.clone())));
            merged = Value::Object(all);
            (&merged, Some((name, pre.kind.as_str())))
        }
    };
    // One `host`, or a list of `hosts`, each armed alike; each once.
    let hosts: Vec<Id> = match (p.get("host"), p.get("hosts")) {
        (Some(h), Some(l)) if !absent(Some(h)) && !absent(Some(l)) => return Err(bad(c, "give `host` or `hosts`, not both")),
        (v, Some(Value::Array(a))) if absent(v) && (1..=rebar::MAX_HOSTS).contains(&a.len()) => {
            let mut seen = BTreeSet::new();
            a.iter().map(|v| element_id(c, m, v)).collect::<Result<Vec<Id>, String>>()?.into_iter().filter(|id| seen.insert(*id)).collect()
        }
        (v, Some(l)) if absent(v) && !absent(Some(l)) => {
            return Err(bad(c, format!("`hosts` is a list of 1 to {} elements", rebar::MAX_HOSTS)));
        }
        _ => {
            let host = id_opt(c, p, "host")?.ok_or_else(|| bad(c, "`host` (an element) or `hosts` (a list of them) is required"))?;
            vec![m.elements.id(host).ok_or_else(|| bad(c, format!("no element {host}")))?]
        }
    };
    let several = hosts.len() > 1;
    let mut made = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut total = 0;
    for id in &hosts {
        let element = m.elements.get(*id).ok_or_else(|| bad(c, format!("no element {id}")))?;
        let level = m.level_of(element).ok_or_else(|| bad(c, "that element has no level"))?;
        if let Some((name, kind)) = kind.filter(|(_, k)| crate::reinforce::kind_of(element) != Some(*k)) {
            return Err(bad(c, format!("preset `{name}` is for a {kind}, and {id} is a {}", element.category())));
        }
        let crate::reinforce::Made { bars, notes: its } = crate::reinforce::bars(m, element, p)
            .map_err(|e| if several { format!("{e} (for the {} {id})", element.category()) } else { e })?;
        notes.extend(its.into_iter().map(|n| if several { format!("{n} (for the {} {id})", element.category()) } else { n }));
        total += bars.len();
        if total > rebar::MAX_BARS_ALL {
            return Err(bad(
                c,
                format!(
                    "that is more than the {} bars a command makes: arm fewer elements at once, or widen a spacing",
                    rebar::MAX_BARS_ALL
                ),
            ));
        }
        made.push((*id, m.guid(*id), level, bars));
    }
    let hosted: BTreeSet<Guid> = made.iter().map(|(_, g, _, _)| *g).collect();
    // The bars each made before, which this replaces; a bar with no layer was drawn by hand.
    let old: Vec<Id> = m
        .elements
        .iter()
        .filter(|(_, e)| matches!(e, Element::Rebar(r) if r.host.is_some_and(|h| hosted.contains(&h)) && !r.layer.is_empty()))
        .map(|(i, _)| *i)
        .collect();
    let weight: f64 = made.iter().flat_map(|(_, _, _, bars)| bars).map(|b| rebar::weight_kg(b.diameter, rebar::length(&b.path))).sum();
    let m = s.model_mut();
    for i in &old {
        m.elements.remove(*i);
    }
    let mut ids: Vec<Id> = Vec::new();
    for (_, guid, level, bars) in made {
        for b in bars {
            ids.push(m.insert(Element::Rebar(Rebar {
                level,
                host: Some(guid),
                path: b.path,
                diameter: b.diameter,
                grade: b.grade,
                layer: b.layer.into(),
            })));
        }
    }
    notes.truncate(rebar::MAX_NOTES);
    Ok(
        json!({"ids": ids, "count": ids.len(), "hosts": hosts.len(), "removed": old.len(), "weight_kg": (weight * 1000.0).round() / 1000.0, "notes": notes}),
    )
}

fn reinforcement_preset(s: &mut Session, p: &Value) -> R {
    let c = "reinforcement_preset";
    only(c, p, &["name", "kind", "params", "rename", "delete"])?;
    let Some(name) = name_param(c, p, "name")? else {
        return Ok(json!({"presets": s.model.reinforcement}));
    };
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if m.reinforcement.remove(&name).is_none() {
            return Err(bad(c, format!("there's no reinforcement preset `{name}`")));
        }
        return Ok(json!({"deleted": name}));
    }
    let was = m.reinforcement.get(&name).cloned();
    let kind = string(c, p, "kind")?
        .or_else(|| was.as_ref().map(|w| w.kind.clone()))
        .ok_or_else(|| bad(c, format!("`kind` ({}) is required for a new preset", crate::reinforce::KINDS.join(", "))))?;
    let params = match p.get("params") {
        Some(Value::Object(o)) => o.clone(),
        None | Some(Value::Null) => was.map(|w| w.params).ok_or_else(|| bad(c, "`params` {…} is required for a new preset"))?,
        Some(v) => return Err(bad(c, format!("`params` must be an object of reinforce's parameters, got {v}"))),
    };
    let name = match name_param(c, p, "rename")? {
        Some(to) if to != name => {
            if m.reinforcement.contains_key(&to) {
                return Err(bad(c, format!("there's already a reinforcement preset `{to}`")));
            }
            m.reinforcement.remove(&name);
            rename_entry(m, &LibKind::ReinforcementPreset, &name, &to);
            to
        }
        _ => name,
    };
    let preset = crate::reinforce::Preset { kind, params };
    preset.check(&name).map_err(|e| bad(c, e))?;
    if !m.reinforcement.contains_key(&name) && m.reinforcement.len() >= crate::reinforce::MAX_PRESETS {
        return Err(bad(c, format!("the project holds up to {} presets", crate::reinforce::MAX_PRESETS)));
    }
    m.reinforcement.insert(name.clone(), preset.clone());
    Ok(json!({"name": name, "preset": preset}))
}

/// A number of `unit` under `key`, finite and of any size: the setting's own check says how large it may be.
fn amount(c: &str, p: &Value, key: &str, unit: &str) -> Result<Option<f64>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => {
            v.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| bad(c, format!("`{key}` must be a number of {unit}, got {v}")))
        }
        None => Ok(None),
    }
}

/// A whole number under `key`, from 0 to 255.
fn small(c: &str, p: &Value, key: &str) -> Result<Option<u8>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| bad(c, format!("`{key}` must be a whole number from 0 to 255, got {v}"))),
        None => Ok(None),
    }
}

/// The text under `key` to set: none when it isn't given (what it was stays), none inside when it is "" (taken away).
fn clearable(c: &str, p: &Value, key: &str) -> Result<Option<Option<String>>, String> {
    Ok(string(c, p, key)?.map(|t| Some(t.trim().to_string()).filter(|t| !t.is_empty())))
}

fn voltage(s: &mut Session, p: &Value) -> R {
    use crate::electrical::{MAX_VOLTAGES, Voltage};
    let c = "voltage";
    only(c, p, &["name", "nominal", "min", "max", "rename", "delete"])?;
    let Some(name) = name_param(c, p, "name")? else {
        return Ok(json!({"voltages": s.model.voltages}));
    };
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if !m.voltages.contains_key(&name) {
            return Err(bad(c, format!("there's no voltage `{name}`")));
        }
        if let Some((system, _)) = m.distribution_systems.iter().find(|(_, d)| d.voltage_names().any(|v| *v == name)) {
            return Err(bad(c, format!("voltage `{name}` is one distribution system `{system}` supplies: change or delete that first")));
        }
        m.voltages.remove(&name);
        return Ok(json!({"deleted": name}));
    }
    let was = m.voltages.get(&name).cloned();
    let rename = name_param(c, p, "rename")?;
    if was.is_none() && rename.is_some() {
        return Err(bad(c, format!("there's no voltage `{name}` to rename")));
    }
    let nominal = match (amount(c, p, "nominal", "volts")?, &was) {
        (Some(n), _) => n,
        (None, Some(w)) => w.nominal,
        (None, None) => return Err(bad(c, "`nominal` (volts) is required for a new voltage")),
    };
    let band = Voltage::around(nominal);
    let v = Voltage {
        nominal,
        min: amount(c, p, "min", "volts")?.or(was.as_ref().map(|w| w.min)).unwrap_or(band.min),
        max: amount(c, p, "max", "volts")?.or(was.as_ref().map(|w| w.max)).unwrap_or(band.max),
    };
    let name = match rename {
        Some(to) if to != name => {
            if m.voltages.contains_key(&to) {
                return Err(bad(c, format!("there's already a voltage `{to}`")));
            }
            m.voltages.remove(&name);
            rename_entry(m, &LibKind::Voltage, &name, &to);
            to
        }
        _ => name,
    };
    v.check(&name).map_err(|e| bad(c, e))?;
    if !m.voltages.contains_key(&name) && m.voltages.len() >= MAX_VOLTAGES {
        return Err(bad(c, format!("the project holds up to {MAX_VOLTAGES} voltages")));
    }
    m.voltages.insert(name.clone(), v.clone());
    Ok(json!({"name": name, "voltage": v}))
}

fn distribution_system(s: &mut Session, p: &Value) -> R {
    use crate::electrical::{DistributionSystem, MAX_SYSTEMS};
    let c = "distribution_system";
    only(c, p, &["name", "phases", "wires", "l_l", "l_n", "l_g", "ground", "rename", "delete"])?;
    let Some(name) = name_param(c, p, "name")? else {
        return Ok(json!({"distribution_systems": s.model.distribution_systems}));
    };
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if m.distribution_systems.remove(&name).is_none() {
            return Err(bad(c, format!("there's no distribution system `{name}`")));
        }
        return Ok(json!({"deleted": name}));
    }
    let was = m.distribution_systems.get(&name).cloned();
    let rename = name_param(c, p, "rename")?;
    if was.is_none() && rename.is_some() {
        return Err(bad(c, format!("there's no distribution system `{name}` to rename")));
    }
    let phases = small(c, p, "phases")?
        .or(was.as_ref().map(|w| w.phases))
        .ok_or_else(|| bad(c, "`phases` (1, 2 or 3) is required for a new distribution system"))?;
    let wires = small(c, p, "wires")?
        .or(was.as_ref().map(|w| w.wires))
        .ok_or_else(|| bad(c, "`wires` (the line conductors and the neutral) is required for a new distribution system"))?;
    let (l_l, l_n, l_g, ground) = was.map(|w| (w.l_l, w.l_n, w.l_g, w.ground)).unwrap_or_default();
    let d = DistributionSystem {
        phases,
        wires,
        l_l: clearable(c, p, "l_l")?.unwrap_or(l_l),
        l_n: clearable(c, p, "l_n")?.unwrap_or(l_n),
        l_g: clearable(c, p, "l_g")?.unwrap_or(l_g),
        ground: clearable(c, p, "ground")?.unwrap_or(ground),
    };
    let name = match rename {
        Some(to) if to != name => {
            if m.distribution_systems.contains_key(&to) {
                return Err(bad(c, format!("there's already a distribution system `{to}`")));
            }
            m.distribution_systems.remove(&name);
            rename_entry(m, &LibKind::DistributionSystem, &name, &to);
            to
        }
        _ => name,
    };
    d.check(&name, &m.voltages).map_err(|e| bad(c, e))?;
    if !m.distribution_systems.contains_key(&name) && m.distribution_systems.len() >= MAX_SYSTEMS {
        return Err(bad(c, format!("the project holds up to {MAX_SYSTEMS} distribution systems")));
    }
    m.distribution_systems.insert(name.clone(), d.clone());
    Ok(json!({"name": name, "distribution_system": d}))
}

fn wire_type(s: &mut Session, p: &Value) -> R {
    use crate::electrical::{MATERIALS, MAX_SIZES, MAX_WIRE_TYPES, WireSize, WireType};
    let c = "wire_type";
    only(c, p, &["name", "material", "temperature", "insulation", "note", "sizes", "rename", "delete"])?;
    let Some(name) = name_param(c, p, "name")? else {
        return Ok(json!({"wire_types": s.model.wire_types}));
    };
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if m.wire_types.remove(&name).is_none() {
            return Err(bad(c, format!("there's no wire type `{name}`")));
        }
        return Ok(json!({"deleted": name}));
    }
    let was = m.wire_types.get(&name).cloned();
    let rename = name_param(c, p, "rename")?;
    if was.is_none() && rename.is_some() {
        return Err(bad(c, format!("there's no wire type `{name}` to rename")));
    }
    let material = string(c, p, "material")?
        .or(was.as_ref().map(|w| w.material.clone()))
        .ok_or_else(|| bad(c, format!("`material` ({}) is required for a new wire type", MATERIALS.join(", "))))?;
    let temperature = amount(c, p, "temperature", "°C")?
        .or(was.as_ref().map(|w| w.temperature))
        .ok_or_else(|| bad(c, "`temperature` (°C) is required for a new wire type"))?;
    let insulation = string(c, p, "insulation")?
        .or(was.as_ref().map(|w| w.insulation.clone()))
        .ok_or_else(|| bad(c, "`insulation` is required for a new wire type"))?;
    let note = string(c, p, "note")?.or(was.as_ref().map(|w| w.note.clone())).unwrap_or_default();
    let sizes: Vec<WireSize> = match p.get("sizes") {
        Some(v) if !absent(Some(v)) => {
            if v.as_array().is_some_and(|rows| rows.len() > MAX_SIZES) {
                return Err(bad(c, format!("`sizes` holds up to {MAX_SIZES} rows")));
            }
            serde_json::from_value(v.clone())
                .map_err(|e| bad(c, format!("`sizes` must be a list of {{section, ampacity, label?}}: {e}")))?
        }
        _ => was.map(|w| w.sizes).ok_or_else(|| bad(c, "`sizes` [{section, ampacity}, …] is required for a new wire type"))?,
    };
    let w = WireType { material, temperature, insulation, note, sizes };
    let name = match rename {
        Some(to) if to != name => {
            if m.wire_types.contains_key(&to) {
                return Err(bad(c, format!("there's already a wire type `{to}`")));
            }
            m.wire_types.remove(&name);
            rename_entry(m, &LibKind::WireType, &name, &to);
            to
        }
        _ => name,
    };
    w.check(&name).map_err(|e| bad(c, e))?;
    if !m.wire_types.contains_key(&name) && m.wire_types.len() >= MAX_WIRE_TYPES {
        return Err(bad(c, format!("the project holds up to {MAX_WIRE_TYPES} wire types")));
    }
    m.wire_types.insert(name.clone(), w.clone());
    Ok(json!({"name": name, "wire_type": w}))
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
    let top_offset = num(c, p, "top_offset")?.unwrap_or(0.0);
    let height = height.unwrap_or(DEFAULT_HEIGHT);
    let arc = num(c, p, "arc")?.unwrap_or(0.0);
    let location = match string(c, p, "location")? {
        Some(n) => Location::from_name(&n).ok_or_else(|| bad(c, location_names(&n)))?,
        None => Location::Centre,
    };
    let (attach_top, attach_base) = (id_opt(c, p, "attach_top")?, id_opt(c, p, "attach_base")?);
    let room_bounding = match p.get("room_bounding") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(c, "`room_bounding` is true or false")),
    };
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
                top_offset,
                arc,
                location,
                attach_top,
                attach_base,
                room_bounding,
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
                // A flipped wall runs the other way, and across it in to out is out to in: an end swaps too.
                if lengths.contains_key(&r.id) {
                    r.face = r.face.flipped();
                    r.point = r.point.map(End::other);
                } else if hosts.get(&r.id).is_some_and(|h| lengths.contains_key(h)) {
                    r.face = r.face.mirrored();
                    r.point = r.point.map(End::other);
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
            wall_at(m, level, at).ok_or_else(|| {
                let none = format!("no wall at [{}, {}] on {}", at.0, at.1, level_name(m, level));
                match wall_elsewhere(m, at) {
                    Some((id, bottom, top)) => bad(
                        c,
                        format!(
                            "{none}; wall {id} stands there but spans {bottom} to {top} mm and {} cuts at {} mm: give the wall that level, or a view range that cuts it",
                            level_name(m, level),
                            m.plan_range(level).cut
                        ),
                    ),
                    None => bad(c, none),
                }
            })?
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
        shape: Default::default(),
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
    let mut r = Room::new(level, point, name, number.clone());
    r.frozen = flag(c, p, "frozen")?;
    r.base_offset = num(c, p, "base_offset")?.unwrap_or(0.0);
    let limit_offset = num(c, p, "limit_offset")?;
    // Unless told, up to 300 mm under the level above; with none above, not bounded, 2500 mm tall.
    (r.top_level, r.limit_offset) = match (p.get("top_level"), limit_offset) {
        (v, None) if absent(v) => match m.level_above(level) {
            Some(above) => (Some(above), crate::model::ROOM_BELOW_ABOVE),
            None => (None, crate::model::ROOM_LIMIT_OFFSET),
        },
        (v, offset) if absent(v) => (None, offset.unwrap_or(crate::model::ROOM_LIMIT_OFFSET)),
        (_, offset) => (Some(level_param(c, m, p, "top_level")?), offset.unwrap_or(0.0)),
    };
    if let Some(wall) = m.wall_holding(&r, m.room_area) {
        return Err(bad(
            c,
            format!(
                "[{}, {}] lies inside wall {}; place a room in the space between walls, not in a wall",
                point.0,
                point.1,
                m.shown(wall)
            ),
        ));
    }
    // Unbounded, it is placed all the same, as Revit's, with a warning that says why.
    let (area, warning) = match m.room_bound(&r, m.room_area) {
        Ok(region) => (m2(region.area()), None),
        Err(Unbound::NotEnclosed) => (
            0.0,
            Some(format!(
                "[{}, {}] isn't enclosed by walls or room separation lines on {}: the room is placed unbounded, a {} mm square, \
                 until they close round it",
                point.0,
                point.1,
                level_name(m, level),
                crate::model::UNBOUNDED_ROOM
            )),
        ),
        Err(Unbound::Shared(other)) => (
            0.0,
            Some(format!(
                "room {} is already in this space; two rooms can't share one, so this one is placed unbounded: move it, delete \
                 one of them, or part them with a room separation line",
                m.shown(other)
            )),
        ),
    };
    let id = s.model_mut().insert(Element::Room(r));
    let mut out = json!({"id": id, "number": number, "area_m2": area});
    if let Some(w) = warning {
        out["warning"] = json!(w);
    }
    Ok(out)
}

/// The elements whose reference lines may come within `reach` mm of `p` in a plan of `level`, by id: the walls near it
/// that span the plan's cut height (a wall's faces and caps stay within twice the thickest wall's thickness of its location
/// line: a cap runs on no farther than the thickest wall meeting it, [`walls::cap`]), the doors and windows in them, and
/// the grids.
fn near_point(m: &Model, level: Guid, p: P2, reach: f64) -> BTreeSet<Id> {
    let look = m.lookup();
    let thickest = m.types.wall.values().map(|t| t.thickness).filter(|t| t.is_finite()).fold(0.0, f64::max);
    // Of the walls the plan cuts: those that span its cut height.
    let cut = m.plan_range(level).cut;
    let walls = look.walls_near_at(&[([p.0, p.1, p.0, p.1], cut, cut)], reach + 2.0 * thickest + 1.0);
    let mut out = BTreeSet::new();
    for &w in &walls {
        let g = m.guid(w);
        out.extend(
            look.referrers(&g).filter(|id| matches!(m.elements.get(*id), Some(Element::Door(o) | Element::Window(o)) if o.host == g)),
        );
    }
    out.extend(walls);
    out.extend(look.grids());
    out
}

/// Of the lines a plan of `level` shows that `keep` takes (walls' faces, centre lines and ends,
/// doors' and windows' centres and jambs, grids), the nearest `p`, within 100 mm.
pub(crate) fn reference_near(m: &Model, level: impl Key, p: P2, keep: impl Fn(Line) -> bool) -> Option<Reference> {
    let level = m.guid(level);
    nearest_reference(m, level, p, keep, near_point(m, level, p, 100.0).into_iter())
}

/// [`reference_near`] among every element: its twin, which tests hold it to.
#[cfg(test)]
pub(crate) fn reference_near_everywhere(m: &Model, level: impl Key, p: P2, keep: impl Fn(Line) -> bool) -> Option<Reference> {
    let level = m.guid(level);
    nearest_reference(m, level, p, keep, m.elements.iter().map(|(id, _)| *id))
}

fn nearest_reference(m: &Model, level: Guid, p: P2, keep: impl Fn(Line) -> bool, ids: impl Iterator<Item = Id>) -> Option<Reference> {
    let cut = |w: Option<&Wall>| w.is_some_and(|w| m.cut_by(w, level));
    ids.filter(|id| match m.elements.get(*id) {
        Some(Element::Wall(w)) => cut(Some(w)),
        Some(Element::Door(o) | Element::Window(o)) => cut(m.wall(o.host)),
        Some(Element::Grid(_)) => true,
        _ => false,
    })
    .filter_map(|id| m.reference_at(id, p, &keep))
    .filter(|(_, d)| *d <= 100.0)
    .min_by(|a, b| a.1.total_cmp(&b.1))
    .map(|(r, _)| r)
}

/// Of the ends of the lines a plan of `level` (`view`, when not the level's own) shows (walls' centre lines and faces,
/// doors' and windows' jambs, grids, its detail lines), the nearest `p`, within 100 mm.
pub(crate) fn endpoint_near(m: &Model, level: Guid, view: Option<Guid>, p: P2) -> Option<Reference> {
    let plan = view.unwrap_or(level);
    let mut ids = near_point(m, level, p, 100.0);
    // The view's detail lines: what points at it.
    ids.extend(m.lookup().referrers(&m.guid(plan)).filter(|id| matches!(m.elements.get(*id), Some(Element::DetailLine(_)))));
    nearest_end(m, level, view, p, ids.into_iter())
}

/// [`endpoint_near`] among every element: its twin, which tests hold it to.
#[cfg(test)]
pub(crate) fn endpoint_near_everywhere(m: &Model, level: Guid, view: Option<Guid>, p: P2) -> Option<Reference> {
    nearest_end(m, level, view, p, m.elements.iter().map(|(id, _)| *id))
}

fn nearest_end(m: &Model, level: Guid, view: Option<Guid>, p: P2, ids: impl Iterator<Item = Id>) -> Option<Reference> {
    let cut = |w: Option<&Wall>| w.is_some_and(|w| m.cut_by(w, level));
    let plan = view.unwrap_or(level);
    ids.filter(|id| match m.elements.get(*id) {
        Some(Element::Wall(w)) => cut(Some(w)),
        Some(Element::Door(o) | Element::Window(o)) => cut(m.wall(o.host)),
        Some(Element::Grid(_)) => true,
        Some(Element::DetailLine(l)) => m.same(l.view, plan),
        _ => false,
    })
    .filter_map(|id| m.endpoint_at(id, p))
    .filter(|(_, d)| *d <= 100.0)
    .min_by(|a, b| a.1.total_cmp(&b.1))
    .map(|(r, _)| r)
}

/// Of the arcs a plan of `level` (`view`, when not the level's own) shows (its curved walls' centre lines and faces, its
/// detail lines that are arcs), the one passing nearest `p`, within 100 mm.
fn sweep_near(m: &Model, level: Guid, view: Option<Guid>, p: P2) -> Option<Reference> {
    let plan = view.unwrap_or(level);
    m.elements
        .with_guids()
        .flat_map(|(_, id, e)| {
            match e {
                Element::Wall(w) if w.arc != 0.0 && m.cut_by(w, level) => vec![Face::Centre, Face::Left, Face::Right],
                Element::DetailLine(l) if m.same(l.view, plan) => vec![Face::Centre],
                _ => Vec::new(),
            }
            .into_iter()
            .map(move |face| Reference::line(id, face))
        })
        .filter_map(|r| {
            let (c, radius, a0, sweep) = m.reference_sweep(&r).ok()?;
            // How far round from its start `p` lies, the way it sweeps: within it, the distance off it; else from its ends.
            let t = (sweep.signum() * ((p.1 - c.1).atan2(p.0 - c.0) - a0)).rem_euclid(std::f64::consts::TAU);
            let gap = if t <= sweep.abs() {
                (p.dist(c) - radius).abs()
            } else {
                let end = |a: f64| c + P2(a.cos(), a.sin()) * radius;
                p.dist(end(a0)).min(p.dist(end(a0 + sweep)))
            };
            Some((r, gap))
        })
        .filter(|(_, d)| *d <= 100.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

/// Of what a plan of `level` (`view`, when not the level's own) shows round (its round columns, its curved walls' centre
/// lines and faces, its detail lines that are circles or arcs), the one whose edge passes nearest `p`, within 100 mm.
fn arc_near(m: &Model, level: Guid, view: Option<Guid>, p: P2) -> Option<Reference> {
    let plan = view.unwrap_or(level);
    m.elements
        .with_guids()
        .flat_map(|(_, id, e)| {
            match e {
                e if e.column().is_some_and(|col| col.level == level) => vec![Face::Centre],
                Element::Wall(w) if w.arc != 0.0 && m.cut_by(w, level) => vec![Face::Centre, Face::Left, Face::Right],
                Element::DetailLine(l) if m.same(l.view, plan) => vec![Face::Centre],
                _ => Vec::new(),
            }
            .into_iter()
            .map(move |face| Reference::line(id, face))
        })
        .filter_map(|r| {
            let (c, radius) = m.reference_arc(&r).ok()?;
            Some((r, (p.dist(c) - radius).abs()))
        })
        .filter(|(_, d)| *d <= 100.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

/// A dimension in a 3D view: see its params doc.
fn dimension_3d(s: &mut Session, p: &Value) -> R {
    let c = "dimension_3d";
    let m = &s.model;
    let default = absent(p.get("view"))
        || matches!(p.get("view"), Some(Value::String(n)) if n.eq_ignore_ascii_case("{3D}") || n.eq_ignore_ascii_case("3D"));
    let view = if default {
        m.default_3d()
    } else {
        let v = view_param(c, m, p, false)?;
        if !matches!(m.elements.get(v), Some(Element::View3D(_))) {
            return Err(bad(
                c,
                format!("{} isn't a 3D view; a dimension of points and edges in space goes in a 3D view", view_title(m, v)),
            ));
        }
        Some(v)
    };
    let refs = match p.get("refs") {
        Some(Value::Array(a)) if a.len() == 2 => a,
        _ => return Err(bad(c, "`refs` is two references [{at: [x, y, z], edge?: [[x, y, z], [x, y, z]]}, …]")),
    };
    let one = |v: &Value| -> Result<Ref3, String> {
        let r = serde_json::from_value::<Ref3>(v.clone()).map_err(|e| bad(c, format!("a ref is {{at: [x, y, z], edge?}}: {e}")))?;
        let fine = |q: [f64; 3]| q.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD);
        if !fine(r.at) || r.edge.is_some_and(|[a, b]| !fine(a) || !fine(b)) {
            return Err(bad(c, format!("a ref's points must be [x, y, z] in mm within ±{MAX_COORD}, got {v}")));
        }
        Ok(r)
    };
    let refs = [one(&refs[0])?, one(&refs[1])?];
    let at = point3_req(c, p, "at")?;
    let type_name = match string(c, p, "type")? {
        Some(t) => t,
        None => crate::dims::default_type(&m.dimension_types, DimKind::Aligned).cloned().unwrap_or_default(),
    };
    let view = match view {
        Some(v) => v,
        None => s.model_mut().insert(Element::View3D(View3D::default_view())),
    };
    let d = Dimension3D { view: s.model.guid(view), refs, at, type_name };
    let length = crate::dims3d::length(&d);
    let id = s.model_mut().insert(Element::Dimension3D(d));
    Ok(json!({"id": id, "view": view, "length": hundredth(length)}))
}

fn dimension(s: &mut Session, p: &Value) -> R {
    let c = "dimension";
    let m = &s.model;
    let (level, view) = plan_param(c, m, p, "level")?;
    let at = point_req(c, p, "at")?;
    let kind = match string(c, p, "kind")? {
        None => DimKind::Aligned,
        Some(k) => serde_json::from_value(json!(k))
            .map_err(|_| bad(c, format!("`kind` is aligned, linear, angular, radial, diameter or arc_length, not `{k}`")))?,
    };
    let mut orient: Option<Orient> = match string(c, p, "orient")? {
        None => None,
        Some(o) => Some(serde_json::from_value(json!(o)).map_err(|_| bad(c, format!("`orient` is horizontal or vertical, not `{o}`")))?),
    };
    let given = p.get("refs").and_then(Value::as_array).ok_or_else(|| bad(c, "`refs` [ref, …] is required"))?;
    if given.len() > MAX_POINTS {
        return Err(bad(c, format!("`refs` has more than {MAX_POINTS} references")));
    }
    let mut refs: Vec<Reference> = Vec::new();
    for v in given {
        let near = |q: P2, what: &str| bad(c, format!("no {what} at [{}, {}] on {}", q.0, q.1, level_name(m, level)));
        let endpoint = v.get("endpoint").filter(|_| v.as_object().is_some_and(|o| o.len() == 1));
        if let Some(e) = endpoint {
            let q = point(e).ok_or_else(|| bad(c, format!("`endpoint` is a point [x, y], not {e}")))?;
            refs.push(
                endpoint_near(m, level, view, q)
                    .ok_or_else(|| near(q, "end of a wall, a wall face, a door, a window, a grid or a detail line"))?,
            );
            continue;
        }
        let r = match point(v) {
            Some(q) if matches!(kind, DimKind::Radial | DimKind::Diameter) => {
                arc_near(m, level, view, q).ok_or_else(|| near(q, "round column's, curved wall's, circle's or arc's edge"))?
            }
            Some(q) if kind == DimKind::ArcLength => sweep_near(m, level, view, q).ok_or_else(|| near(q, "curved wall or arc"))?,
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
    if kind == DimKind::Linear && orient.is_none() {
        // East or west of everything it measures, vertical; else horizontal.
        let xs: Vec<f64> = refs.iter().filter_map(|r| m.reference_geometry(r).ok()).flat_map(|(a, b)| [a.0, b.0]).collect();
        let beside = xs.iter().all(|&x| x < at.0) || xs.iter().all(|&x| x > at.0);
        orient = Some(if beside && !xs.is_empty() { Orient::Vertical } else { Orient::Horizontal });
    }
    let type_name = match name_param(c, p, "type")? {
        Some(t) => t,
        None => crate::dims::default_type(&m.dimension_types, kind).cloned().unwrap_or_default(),
    };
    let text = |key: &str| string(c, p, key).map(Option::unwrap_or_default);
    let d = Dimension {
        level,
        view,
        kind,
        orient,
        refs,
        at,
        type_name,
        prefix: text("prefix")?,
        suffix: text("suffix")?,
        above: text("above")?,
        below: text("below")?,
        replace: text("replace")?,
        equality: flag(c, p, "equality")?,
        texts: Vec::new(),
        equal: false,
        locked: Vec::new(),
        alignment: false,
    };
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
    if p.get("size").is_some() {
        return Err(bad(c, "a note's size is its text type's: give `type` (`text_type` lists them, and makes one of a size)"));
    }
    only(c, p, &["text", "at", "view", "type", "angle", "leader", "override_format", "runs"])?;
    let m = &s.model;
    let text = note_text(c, p)?.ok_or_else(|| bad(c, "`text` is required"))?;
    let at = point_req(c, p, "at")?;
    let view = view_param(c, m, p, true)?;
    let type_name = match name_param(c, p, "type")? {
        Some(n) => n,
        None => crate::text::default_type(&m.text_types).cloned().ok_or_else(|| bad(c, "the project has no text type; make one first"))?,
    };
    let runs = match p.get("runs") {
        v if absent(v) => Vec::new(),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad(c, format!("`runs`: {e}")))?,
        None => Vec::new(),
    };
    let n = Note {
        view: m.guid(view),
        at,
        text,
        type_name,
        size: None,
        angle: num(c, p, "angle")?.unwrap_or(0.0),
        leader: point_opt(c, p, "leader")?,
        override_format: flag(c, p, "override_format")?,
        runs,
    };
    let id = s.model_mut().insert(Element::Text(n));
    Ok(json!({"id": id}))
}

/// A note's `text`: up to [`crate::text::MAX_NOTE`] characters.
fn note_text(c: &str, p: &Value) -> Result<Option<String>, String> {
    match p.get("text") {
        v if absent(v) => Ok(None),
        Some(Value::String(t)) if t.chars().count() <= crate::text::MAX_NOTE => Ok(Some(t.clone())),
        Some(v) => Err(bad(c, format!("`text` must be text of at most {} characters, got {v}", crate::text::MAX_NOTE))),
        None => Ok(None),
    }
}

fn text_type(s: &mut Session, p: &Value) -> R {
    let c = "text_type";
    let keys = ["font", "size", "bold", "italic", "underline", "width_factor", "color", "border", "border_offset", "border_pen"];
    only(c, p, &[&["name", "like", "rename", "delete"][..], &keys[..]].concat())?;
    let Some(name) = name_param(c, p, "name")? else {
        // What letters each type here: its font, or what stands in for it.
        let drawn: serde_json::Map<String, Value> =
            s.model.text_types.iter().map(|(n, t)| (n.clone(), json!(crate::text::resolve(&t.font, false, false).family()))).collect();
        return Ok(json!({"types": s.model.text_types, "fonts": crate::text::families(), "drawn_with": drawn}));
    };
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if m.text_types.remove(&name).is_none() {
            return Err(bad(c, format!("there's no text type `{name}`")));
        }
        if let Some((id, _)) = m.elements.iter().find(|(_, e)| matches!(e, Element::Text(n) if n.type_name == name)) {
            return Err(bad(c, format!("note {id} is of `{name}`; give it another type first")));
        }
        return Ok(json!({"deleted": name}));
    }
    let mut t = match (m.text_types.get(&name), name_param(c, p, "like")?) {
        (Some(t), _) => t.clone(),
        (None, Some(like)) => {
            m.text_types.get(&like).cloned().ok_or_else(|| bad(c, format!("there's no text type `{like}` to start from")))?
        }
        (None, None) => crate::text::TextType::default(),
    };
    if let Some(f) = name_param(c, p, "font")? {
        // A font the type has already stays, though this system hasn't it (a project from another).
        if !f.trim().eq_ignore_ascii_case(t.font.trim()) {
            t.font = crate::text::known(&f).ok_or_else(|| {
                let all = crate::text::families();
                let word = f.split_whitespace().next().unwrap_or_default().to_lowercase();
                let like: Vec<&str> = all.iter().map(String::as_str).filter(|n| n.to_lowercase().contains(&word)).take(12).collect();
                let some: Vec<&str> = if like.is_empty() { all.iter().map(String::as_str).take(12).collect() } else { like };
                let n = all.len();
                bad(
                    c,
                    format!(
                        "this system has no font `{f}` (it has {n}, such as: {}); `text_type` without `name` lists them",
                        some.join(", ")
                    ),
                )
            })?;
        }
    }
    for (key, slot) in [("size", &mut t.size), ("width_factor", &mut t.width_factor), ("border_offset", &mut t.border_offset)] {
        if let Some(x) = num(c, p, key)? {
            *slot = x;
        }
    }
    for (key, slot) in [("bold", &mut t.bold), ("italic", &mut t.italic), ("underline", &mut t.underline), ("border", &mut t.border)] {
        if !absent(p.get(key)) {
            *slot = flag(c, p, key)?;
        }
    }
    if let Some(v) = p.get("color").filter(|v| !v.is_null()) {
        t.color = colour(v).ok_or_else(|| bad(c, format!("`color` is \"#rrggbb\" or [r, g, b], each 0 to 255, not {v}")))?;
    }
    if let Some(v) = p.get("border_pen").filter(|v| !v.is_null()) {
        t.border_pen = serde_json::from_value(v.clone()).map_err(|e| bad(c, format!("`border_pen`: {e}")))?;
    }
    let name = match name_param(c, p, "rename")? {
        Some(to) if to != name => {
            if m.text_types.contains_key(&to) {
                return Err(bad(c, format!("there's already a text type `{to}`")));
            }
            m.text_types.remove(&name);
            rename_entry(m, &LibKind::TextType, &name, &to);
            to
        }
        _ => name,
    };
    t.check(&name).map_err(|e| bad(c, e))?;
    if m.text_types.get(&name) != Some(&t) {
        m.text_types.insert(name.clone(), t.clone());
    }
    Ok(json!({"name": name, "type": t}))
}

fn dimension_constraint(s: &mut Session, p: &Value) -> R {
    use crate::model::Lock;
    let c = "dimension_constraint";
    only(c, p, &["id", "equal", "locked"])?;
    let id = element_id(c, &s.model, p.get("id").unwrap_or(&Value::Null))?;
    let Some(Element::Dimension(d)) = s.model.elements.get(id) else {
        return Err(bad(c, format!("{} {id} isn't a dimension", s.model.elements.get(id).map_or("nothing", |e| e.category()))));
    };
    let mut d = d.clone();
    let values = s.model.dimension_values(&d);
    if !absent(p.get("equal")) {
        d.equal = flag(c, p, "equal")?;
        // As Revit's EQ shows its segments as EQ.
        d.equality = d.equal;
    }
    if let Some(v) = p.get("locked").filter(|v| !v.is_null()) {
        let list = v.as_array().ok_or_else(|| bad(c, format!("`locked` is a list of segments, not {v}")))?;
        let mut locked = Vec::new();
        for x in list {
            let i = x
                .as_u64()
                .map(|i| i as usize)
                .filter(|&i| i < values.len())
                .ok_or_else(|| bad(c, format!("dimension {id} has segments 0 to {}, not {x}", values.len().saturating_sub(1))))?;
            // A segment locked already keeps its length; a new one, the length it has now.
            let length = d.locked.iter().find(|l| l.segment == i).map_or(values.get(i).copied().unwrap_or_default(), |l| l.length);
            if !locked.iter().any(|l: &Lock| l.segment == i) {
                locked.push(Lock { segment: i, length });
            }
        }
        locked.sort_by_key(|l| l.segment);
        d.locked = locked;
    }
    s.model_mut().elements.insert(id, Element::Dimension(d.clone()));
    Ok(json!({"id": id, "equal": d.equal, "locked": d.locked.iter().map(|l| l.segment).collect::<Vec<_>>()}))
}

fn dimension_text(s: &mut Session, p: &Value) -> R {
    use crate::model::SegmentText;
    let c = "dimension_text";
    only(c, p, &["id", "segment", "prefix", "suffix", "above", "below", "replace"])?;
    let id = element_id(c, &s.model, p.get("id").unwrap_or(&Value::Null))?;
    let Some(Element::Dimension(d)) = s.model.elements.get(id) else {
        return Err(bad(c, format!("{} {id} isn't a dimension", s.model.elements.get(id).map_or("nothing", |e| e.category()))));
    };
    let mut d = d.clone();
    let count = s.model.dimension_labels(&d).len();
    let given = |key: &str| string(c, p, key);
    match num(c, p, "segment")? {
        None => {
            for (key, slot) in [
                ("prefix", &mut d.prefix),
                ("suffix", &mut d.suffix),
                ("above", &mut d.above),
                ("below", &mut d.below),
                ("replace", &mut d.replace),
            ] {
                if let Some(t) = given(key)? {
                    *slot = t;
                }
            }
        }
        Some(i) if i.fract() == 0.0 && i >= 0.0 && (i as usize) < count => {
            let i = i as usize;
            let mut st = d.texts.iter().find(|t| t.segment == i).cloned().unwrap_or(SegmentText { segment: i, ..SegmentText::default() });
            for (key, slot) in [
                ("prefix", &mut st.prefix),
                ("suffix", &mut st.suffix),
                ("above", &mut st.above),
                ("below", &mut st.below),
                ("replace", &mut st.replace),
            ] {
                if let Some(t) = given(key)? {
                    *slot = t;
                }
            }
            d.texts.retain(|t| t.segment != i);
            if !st.is_empty() {
                d.texts.push(st);
                d.texts.sort_by_key(|t| t.segment);
            }
        }
        Some(i) => return Err(bad(c, format!("dimension {id} has segments 0 to {}, not {i}", count.saturating_sub(1)))),
    }
    let labels = s.model.dimension_labels(&d);
    s.model_mut().elements.insert(id, Element::Dimension(d));
    Ok(json!({"id": id, "labels": labels}))
}

fn dimension_type(s: &mut Session, p: &Value) -> R {
    use crate::dims::DimensionType;
    let c = "dimension_type";
    let Some(name) = name_param(c, p, "name")? else {
        return Ok(json!({"types": s.model.dimension_types, "fonts": crate::text::families()}));
    };
    let m = s.model_mut();
    if flag(c, p, "delete")? {
        if let Some((id, _)) = m.elements.iter().find(|(_, e)| matches!(e, Element::Dimension(d) if d.type_name == name)) {
            return Err(bad(c, format!("dimension {id} is of `{name}`; give it another type first")));
        }
        if m.dimension_types.remove(&name).is_none() {
            return Err(bad(c, format!("there's no dimension type `{name}`")));
        }
        return Ok(json!({"deleted": name}));
    }
    let t = match (m.dimension_types.get(&name), name_param(c, p, "like")?) {
        (Some(t), _) => t.clone(),
        (None, Some(like)) => {
            m.dimension_types.get(&like).cloned().ok_or_else(|| bad(c, format!("there's no dimension type `{like}` to start from")))?
        }
        (None, None) => DimensionType::default(),
    };
    // What it is, with the values given over it, read as a file reads one: a key it hasn't is an error.
    let mut v = serde_json::to_value(&t).map_err(|e| bad(c, e.to_string()))?;
    let mut font = None;
    if let Some(o) = v.as_object_mut() {
        for (key, value) in p.as_object().into_iter().flatten() {
            match key.as_str() {
                "name" | "like" | "rename" | "delete" => {}
                "font" => font = name_param(c, p, "font")?,
                "color" if !value.is_null() => {
                    let rgb =
                        colour(value).ok_or_else(|| bad(c, format!("`color` is \"#rrggbb\" or [r, g, b], each 0 to 255, not {value}")))?;
                    o.insert(key.clone(), json!(rgb));
                }
                _ => {
                    o.insert(key.clone(), value.clone());
                }
            }
        }
    }
    let mut t: DimensionType = serde_json::from_value(v).map_err(|e| bad(c, e.to_string()))?;
    if let Some(f) = font.filter(|f| !f.trim().eq_ignore_ascii_case(t.font.trim())) {
        t.font = crate::text::known(&f)
            .ok_or_else(|| bad(c, format!("this system has no font `{f}`; `dimension_type` without `name` lists them")))?;
    }
    let name = match name_param(c, p, "rename")? {
        Some(to) if to != name => {
            if m.dimension_types.contains_key(&to) {
                return Err(bad(c, format!("there's already a dimension type `{to}`")));
            }
            m.dimension_types.remove(&name);
            rename_entry(m, &LibKind::DimensionType, &name, &to);
            to
        }
        _ => name,
    };
    t.check(&name).map_err(|e| bad(c, e))?;
    if m.dimension_types.get(&name) != Some(&t) {
        m.dimension_types.insert(name.clone(), t.clone());
    }
    Ok(json!({"name": name, "type": t}))
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
    let sp = Spot { view: m.guid(view), at, host, slope: false };
    let elevation = m.spot_value(&sp).map(hundredth);
    let host = host.and_then(|h| m.elements.id(h));
    let id = s.model_mut().insert(Element::Spot(sp));
    Ok(json!({"id": id, "elevation": elevation, "host": host}))
}

/// A spot slope: in a plan, how steeply the top of the floor or roof at `at` (or `host`) falls there, and which way.
fn spot_slope(s: &mut Session, p: &Value) -> R {
    let c = "spot_slope";
    let m = &s.model;
    let at = point_req(c, p, "at")?;
    let view = view_param(c, m, p, false)?;
    let level = m.plan_level(view).ok_or_else(|| bad(c, "a spot slope reads a floor's or a roof's top: place it in a plan"))?;
    let tops = |id: &Id| matches!(m.elements.get(*id), Some(Element::Floor(_) | Element::Roof(_)));
    let host = match id_opt(c, p, "host")? {
        Some(h) => m.elements.id(h),
        None => m.spot_host(level, at),
    };
    let host = host.filter(tops).ok_or_else(|| {
        let place = format!("[{}, {}] on {}", at.0, at.1, level_name(m, level));
        bad(c, format!("no slope to read at {place}; place it on a floor or a roof"))
    })?;
    let sp = Spot { view: m.guid(view), at, host: Some(m.guid(host)), slope: true };
    let read = m.spot_slope(&sp);
    let id = s.model_mut().insert(Element::Spot(sp));
    Ok(
        json!({"id": id, "host": host, "slope": read.map(|(f, _)| hundredth(f)), "toward": read.and_then(|(_, d)| d).map(|d| [hundredth(d.0), hundredth(d.1)])}),
    )
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
            room_bounding: true,
        };
        return Ok(Element::Footing(col));
    }
    let (top_level, height) = top_param(c, m, p, level)?;
    let col = Column {
        room_bounding: true,
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

fn duct(s: &mut Session, p: &Value) -> R {
    chain(s, p, "duct")
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
        "duct" => "duct",
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
    let lines = drawn.as_ref().map(|(_, lines)| lines.clone()).unwrap_or_default();
    let sketch = unslope(&lines);
    let given = !absent(p.get("boundary")) || drawn.is_some();
    let overhang = num(c, p, "overhang")?.unwrap_or(if given || extrusion.is_some() { 0.0 } else { 500.0 });
    let (boundary, found) = match (drawn, given, &extrusion) {
        (Some((r, _)), ..) => (r.outer, r.holes),
        (None, true, _) => (ring(points(c, p, "boundary")?), Vec::new()),
        (None, false, Some(_)) => (Vec::new(), Vec::new()),
        (None, false, None) => roof_footprint(m, level, overhang).map_err(|e| bad(c, e))?,
    };
    let mut r = Roof {
        params: Default::default(),
        type_name,
        level,
        boundary,
        courtyards: rings(c, p, "courtyards")?.unwrap_or(found),
        slope: if extrusion.is_some() { 0.0 } else { num(c, p, "slope")?.unwrap_or(30.0) },
        overhang,
        gable: flag(c, p, "gable")?,
        gables: shaped(c, p, "gables", "a list of edge numbers")?.unwrap_or_default(),
        slopes: shaped(c, p, "slopes", "a list of degrees, one for each edge")?.unwrap_or_default(),
        offset: num(c, p, "offset")?.unwrap_or(0.0),
        openings: rings(c, p, "openings")?.unwrap_or_default(),
        join: id_opt(c, p, "join")?,
        extrusion,
        sketch,
        shape: Default::default(),
    };
    // Its sketch lines' slopes are its edges', unless it is given `slopes`.
    if absent(p.get("slopes")) {
        r.slopes = slopes_of_edges(&r, &lines, &[]);
    }
    let id = s.model_mut().insert(Element::Roof(r));
    Ok(json!({"id": id}))
}

/// `lines` as a roof keeps them: none holds a `slope`, which the roof keeps as its `slopes`.
fn unslope(lines: &[SketchLine]) -> Vec<SketchLine> {
    lines.iter().map(|l| SketchLine { slope: None, ..l.clone() }).collect()
}

/// The `slopes` of roof `r`'s footprint edges, as the sketch `lines` that say a `slope` give them: an edge along such a line has
/// its slope, 0 meaning the roof's; any other edge the one it had along an edge of `was` (an edited roof's edges, each with its
/// slope), else 0. Empty when none has a slope, and for a footprint too big to pitch, whose slopes nothing reads.
fn slopes_of_edges(r: &Roof, lines: &[SketchLine], was: &[((P2, P2), f64)]) -> Vec<f64> {
    let said: Vec<(Vec<P2>, f64)> = lines.iter().filter_map(|l| Some((l.points(), l.slope?))).collect();
    if r.edges().count() > crate::model::MAX_ROOF_POINTS || (said.is_empty() && was.iter().all(|(_, s)| *s == 0.0)) {
        return Vec::new();
    }
    let within = |(a, b): (P2, P2), (c, d): (P2, P2)| geom::dist_to_segment(c, a, b) <= TOL && geom::dist_to_segment(d, a, b) <= TOL;
    let slopes: Vec<f64> = r
        .edges()
        .map(|e| {
            let own = said.iter().find(|(drawn, _)| sketch::along(drawn, e.0, e.1)).map(|(_, s)| *s);
            own.or_else(|| was.iter().find(|(old, _)| within(*old, e)).map(|(_, s)| *s)).unwrap_or(0.0)
        })
        .collect();
    if slopes.iter().all(|s| *s == 0.0) { Vec::new() } else { slopes }
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
        slopes: Vec::new(),
        offset: eave - model.elevation(level),
        openings: Vec::new(),
        join: Some(host),
        extrusion: None,
        sketch: Vec::new(),
        shape: Default::default(),
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
            top_offset: 0.0,
            arc: 0.0,
            location: Location::Centre,
            attach_top: Some(roof_guid),
            attach_base: Some(host),
            room_bounding: true,
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
                let mut l = SketchLine { style: k.style.clone(), invisible: k.invisible, slope: k.slope, ..SketchLine::from(curve) };
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
    let mut lines = kept.unwrap_or_else(|| sketch::lines_of(&outline, &holes));
    // A roof's lines say the slope their edges share, when they have one of their own.
    if let Element::Roof(r) = e
        && !r.slopes.is_empty()
        && r.edges().count() <= crate::model::MAX_ROOF_POINTS
    {
        for l in &mut lines {
            let drawn = l.points();
            let mine: Vec<f64> = r
                .edges()
                .enumerate()
                .filter(|(_, (a, b))| sketch::along(&drawn, *a, *b))
                .map(|(i, _)| r.slopes.get(i).copied().unwrap_or(0.0))
                .collect();
            l.slope = mine.first().copied().filter(|first| mine.iter().all(|s| s == first));
        }
    }
    Some(lines)
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
    let mut said = serde_json::Map::new();
    let e = match e {
        Element::Floor(f) => {
            let shape;
            (shape, said) = shape_within(m, &f, &r.outer, &r.holes);
            Element::Floor(Floor { boundary: r.outer, openings: r.holes, sketch: lines, shape, ..f })
        }
        Element::Ceiling(x) => Element::Ceiling(Ceiling { boundary: r.outer, openings: r.holes, sketch: lines, ..x }),
        Element::Shaft(_) if !r.holes.is_empty() => return Err(bad(c, "a shaft is one outline with no holes in it")),
        Element::Shaft(sh) => Element::Shaft(Shaft { boundary: r.outer, sketch: lines, ..sh }),
        Element::Roof(old) => {
            let mut new = Roof { boundary: r.outer, courtyards: r.holes, gables: Vec::new(), sketch: unslope(&lines), ..old.clone() };
            // A slope stays with its edge too, where the edge stays or is cut in two; the lines may say it instead.
            let was: Vec<((P2, P2), f64)> = old.edges().enumerate().map(|(i, e)| (e, old.slopes.get(i).copied().unwrap_or(0.0))).collect();
            new.slopes = slopes_of_edges(&new, &lines, &was);
            // A flat one's shape keeps what stays on its top, as a floor's does.
            let holes: Vec<Vec<P2>> = new.courtyards.iter().chain(&new.openings).cloned().collect();
            (new.shape, said) = shape_within(m, &m.roof_as_floor(&old), &new.boundary, &holes);
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
    let mut reply = json!({"id": id, "area_m2": area});
    if let Some(o) = reply.as_object_mut() {
        o.extend(said);
    }
    Ok(reply)
}

/// What of a floor's (or a flat roof's, seen as one) shape a new outline and openings keep, so its top round them stays
/// as it was: the points inside the outline (or on it) and in no opening; each split line cut where it leaves the outline
/// or enters an opening, the pieces inside kept; and at each end of a piece, and each new corner of the outline or an
/// opening, a point at the height the top had there, unless one is there or it was 0. And what changed, as `dropped`
/// (`{points: [[x, y], …], lines: [[[x, y], [x, y]], …]}`, what went), `cut` (the lines cut) and `added` (the points made).
fn shape_within(m: &Model, old: &Floor, outline: &[P2], holes: &[Vec<P2>]) -> (crate::model::Shape, serde_json::Map<String, Value>) {
    let sh = &old.shape;
    let mut said = serde_json::Map::new();
    if sh.is_empty() {
        return (sh.clone(), said);
    }
    let was = |q: P2| (m.shape_height(old, q) * 1000.0).round() / 1000.0;
    let near = |r: &[P2], q: P2| geom::ring_edges(r).any(|(a, b)| geom::dist_to_segment(q, a, b) <= TOL);
    let inside = |q: P2| (geom::contains(outline, q) || near(outline, q)) && !holes.iter().any(|h| geom::contains(h, q) && !near(h, q));
    let (mut points, gone_points): (Vec<_>, Vec<_>) = sh.points.iter().cloned().partition(|x| inside(x.at));
    let corners: Vec<P2> = outline.iter().chain(holes.iter().flatten()).copied().collect();
    let old_corners: Vec<P2> = old.boundary.iter().chain(old.openings.iter().flatten()).copied().collect();
    let edges: Vec<(P2, P2)> = std::iter::once(outline).chain(holes.iter().map(Vec::as_slice)).flat_map(geom::ring_edges).collect();
    let mut added: Vec<P2> = Vec::new();
    // A point at `q` at the height the top had there, unless one is there, it's a corner the top already meets, or it was 0.
    let mut keep_height = |points: &mut Vec<crate::model::ShapePoint>, q: P2, corner_ok: bool| {
        let known = points.iter().any(|x| x.at.dist(q) <= TOL) || (corner_ok && corners.iter().any(|c| c.dist(q) <= TOL));
        let z = was(q);
        if !known && z.abs() > 0.01 {
            points.push(crate::model::ShapePoint { at: q, z, ..Default::default() });
            added.push(q);
        }
    };
    // Each new corner where the top wasn't at 0.
    for &c in &corners {
        if !old_corners.iter().any(|o| o.dist(c) <= TOL) {
            keep_height(&mut points, c, false);
        }
    }
    let (mut lines, mut gone_lines, mut cut) = (Vec::new(), Vec::new(), Vec::new());
    for l in &sh.lines {
        // Where it crosses an edge, or passes a corner: the pieces between, those inside kept.
        let mut ts = vec![0.0, 1.0];
        ts.extend(edges.iter().filter_map(|&(a, b)| geom::crossing(l.a, l.b, a, b)));
        ts.extend(corners.iter().filter(|c| geom::dist_to_segment(**c, l.a, l.b) <= TOL).map(|c| geom::param_on_segment(*c, l.a, l.b)));
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        let at = |t: f64| l.a + (l.b - l.a) * t;
        let mut pieces: Vec<(f64, f64)> = Vec::new();
        for w in ts.windows(2) {
            if let [t0, t1] = *w
                && inside(at((t0 + t1) / 2.0))
            {
                match pieces.last_mut() {
                    Some(last) if (last.1 - t0).abs() < 1e-9 => last.1 = t1,
                    _ => pieces.push((t0, t1)),
                }
            }
        }
        let pieces: Vec<(P2, P2)> = pieces.into_iter().map(|(t0, t1)| (at(t0), at(t1))).filter(|(a, b)| a.dist(*b) > TOL).collect();
        match pieces.as_slice() {
            [] => gone_lines.push(l.clone()),
            [(a, b)] if a.dist(l.a) <= TOL && b.dist(l.b) <= TOL => lines.push(l.clone()),
            _ => {
                cut.push([l.a, l.b]);
                lines.extend(pieces.iter().map(|&(a, b)| crate::model::SplitLine { a, b, ..Default::default() }));
            }
        }
    }
    // Each end of a line kept a point or a corner, at the height the top had there.
    for end in lines.iter().flat_map(|l| [l.a, l.b]).collect::<Vec<_>>() {
        keep_height(&mut points, end, true);
    }
    if !gone_points.is_empty() || !gone_lines.is_empty() {
        said.insert(
            "dropped".into(),
            json!({
                "points": gone_points.iter().map(|x| x.at).collect::<Vec<_>>(),
                "lines": gone_lines.iter().map(|l| [l.a, l.b]).collect::<Vec<_>>(),
            }),
        );
    }
    if !cut.is_empty() {
        said.insert("cut".into(), json!(cut));
    }
    if !added.is_empty() {
        said.insert("added".into(), json!(added));
    }
    (crate::model::Shape { points, lines }, said)
}

/// A shape's point as a command takes it; `id` keeps the point it names, and a `guid` sent back with it is let be.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PointIn {
    #[serde(default)]
    id: Option<Id>,
    #[serde(default, rename = "guid")]
    _guid: Option<Guid>,
    at: P2,
    #[serde(default)]
    z: Option<f64>,
}

/// A split line as a command takes it.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LineIn {
    #[serde(default)]
    id: Option<Id>,
    #[serde(default, rename = "guid")]
    _guid: Option<Guid>,
    a: P2,
    b: P2,
}

/// The floor or flat roof `id` names, by its id, with params that are only `keys`: as shape editing sees it
/// (`Model::shapeable`), a roof's top as a floor's.
fn shaped_floor(c: &str, s: &Session, p: &Value, keys: &[&str]) -> Result<(Id, Floor), String> {
    if let Some(key) = p.as_object().and_then(|o| o.keys().find(|k| !keys.contains(&k.as_str()))) {
        return Err(bad(c, format!("unknown parameter `{key}`")));
    }
    let m = &s.model;
    let key = id_opt(c, p, "id")?.ok_or_else(|| bad(c, "`id`, the floor or flat roof to shape, is required"))?;
    let id = m.elements.id(key).ok_or_else(|| bad(c, format!("no element {key}")))?;
    m.shapeable(id).map(|f| (id, f)).map_err(|e| bad(c, format!("{id}: {e}")))
}

/// Element `id`'s shape given anew: a floor's or a roof's.
fn put_shape(s: &mut Session, id: Id, shape: crate::model::Shape) {
    if let Some(sh) = s.model_mut().elements.get_mut(id).and_then(Element::shape_mut) {
        *sh = shape;
    }
}

/// A height as a command takes it, mm: `z`, finite and in range.
fn height(c: &str, z: f64) -> Result<f64, String> {
    (z.is_finite() && z.abs() <= MAX_COORD).then_some(z).ok_or_else(|| bad(c, format!("`z` must be a number in mm, got {z}")))
}

/// Each end of `lines` that is none of `points` and no corner of `f`'s outline or openings, made a point at the height
/// `f`'s top has there with `points` and no lines.
fn loose_ends(m: &Model, f: &Floor, points: &mut Vec<crate::model::ShapePoint>, lines: &[crate::model::SplitLine]) {
    let corners: Vec<P2> = f.boundary.iter().chain(f.openings.iter().flatten()).copied().collect();
    let base = Floor { shape: crate::model::Shape { points: points.clone(), lines: Vec::new() }, ..f.clone() };
    for end in lines.iter().flat_map(|l| [l.a, l.b]) {
        let known = |q: &P2| q.dist(end) <= TOL;
        if !points.iter().any(|x| known(&x.at)) && !corners.iter().any(known) {
            points.push(crate::model::ShapePoint { at: end, z: m.shape_height(&base, end), ..Default::default() });
        }
    }
}

/// What `edit_shape` says of a floor's shape: its points and split lines, its corners at their heights, how many planes
/// its top is, and its lowest and highest.
fn shape_reply(m: &Model, id: Id, f: &Floor) -> Value {
    let points: Vec<Value> = f.shape.points.iter().map(|x| json!({"id": x.id, "at": x.at, "z": x.z})).collect();
    let lines: Vec<Value> = f.shape.lines.iter().map(|l| json!({"id": l.id, "a": l.a, "b": l.b})).collect();
    let corners: Vec<Value> =
        f.boundary.iter().chain(f.openings.iter().flatten()).map(|q| json!({"at": q, "z": hundredth(m.shape_height(f, *q))})).collect();
    json!({"id": id, "points": points, "lines": lines, "corners": corners, "faces": shape_faces(m, f), "top": top_range(m, f)})
}

/// How many planes a floor's top is.
fn shape_faces(m: &Model, f: &Floor) -> usize {
    if f.shape.is_empty() { 1 } else { m.shape_planes(f).map_or(1, |planes| planes.len()) }
}

/// A floor's top at its lowest and its highest, mm above the project's zero, to the hundredth.
fn top_range(m: &Model, f: &Floor) -> Value {
    let (lo, hi) = m
        .floor_faces(f)
        .iter()
        .flat_map(|(ring, plane)| ring.iter().map(move |q| plane.at(*q)))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)));
    json!([hundredth(lo), hundredth(hi)])
}

/// The established tools' shape editing, all at once: a floor's points and split lines given anew, as `edit_boundary`
/// gives an outline. Asked with neither, says what the shape is.
fn edit_shape(s: &mut Session, p: &Value) -> R {
    let c = "edit_shape";
    let (id, f) = shaped_floor(c, s, p, &["id", "points", "lines", "absolute"])?;
    let m = &s.model;
    if absent(p.get("points")) && absent(p.get("lines")) {
        return Ok(shape_reply(m, id, &f));
    }
    let absolute = flag(c, p, "absolute")?;
    let given: Vec<PointIn> = shaped(c, p, "points", "a list of {at: [x, y], z, id?}")?.unwrap_or_default();
    let lines_in: Vec<LineIn> = shaped(c, p, "lines", "a list of {a: [x, y], b: [x, y], id?}")?.unwrap_or_default();
    let top = m.floor_top(&f);
    let old = &f.shape;
    let mut points = Vec::with_capacity(given.len());
    for x in given {
        let z = height(c, x.z.ok_or_else(|| bad(c, format!("the point at {:?} needs its height `z`", x.at)))?)?;
        let was = x.id.and_then(|i| old.points.iter().find(|o| o.id == i));
        let (pid, guid) = was.map_or((0, Guid::default()), |o| (o.id, o.guid));
        points.push(crate::model::ShapePoint { id: pid, guid, at: x.at, z: if absolute { z - top.at(x.at) } else { z } });
    }
    let lines: Vec<crate::model::SplitLine> = lines_in
        .into_iter()
        .map(|l| {
            let was = l.id.and_then(|i| old.lines.iter().find(|o| o.id == i));
            let (lid, guid) = was.map_or((0, Guid::default()), |o| (o.id, o.guid));
            crate::model::SplitLine { id: lid, guid, a: l.a, b: l.b }
        })
        .collect();
    loose_ends(m, &f, &mut points, &lines);
    let f = Floor { shape: crate::model::Shape { points, lines }, ..f };
    let reply = json!({"id": id, "faces": shape_faces(m, &f), "top": top_range(m, &f)});
    put_shape(s, id, f.shape);
    Ok(reply)
}

/// Add Point: a point on a floor's top at `at`, `z` mm above where the top would be without its shape (with `absolute`,
/// at that elevation); with no `z`, at the height the top has there now. Within a millimetre of a point it has, that one.
fn add_point(s: &mut Session, p: &Value) -> R {
    let c = "add_point";
    let (id, mut f) = shaped_floor(c, s, p, &["id", "at", "z", "absolute", "offset"])?;
    let m = &s.model;
    let at = point_req(c, p, "at")?;
    let z = match (num(c, p, "z")?, num(c, p, "offset")?) {
        (Some(_), Some(_)) => return Err(bad(c, "give `z` or `offset`, not both")),
        (None, Some(by)) => m.shape_height(&f, at) + by,
        (Some(z), None) if flag(c, p, "absolute")? => z - m.floor_top(&f).at(at),
        (Some(z), None) => z,
        (None, None) => m.shape_height(&f, at),
    };
    match f.shape.points.iter_mut().find(|x| x.at.dist(at) <= TOL) {
        Some(x) => x.z = z,
        None => f.shape.points.push(crate::model::ShapePoint { at, z, ..Default::default() }),
    }
    let reply = json!({"id": id, "faces": shape_faces(m, &f), "top": top_range(m, &f)});
    put_shape(s, id, f.shape);
    Ok(reply)
}

/// Add Split Line: a line from `a` to `b` a floor's top folds along; an end that is no point of its and no corner becomes
/// a point, at the height the top has there now.
fn add_split_line(s: &mut Session, p: &Value) -> R {
    let c = "add_split_line";
    let (id, mut f) = shaped_floor(c, s, p, &["id", "a", "b"])?;
    let m = &s.model;
    let line = crate::model::SplitLine { a: point_req(c, p, "a")?, b: point_req(c, p, "b")?, ..Default::default() };
    let mut points = f.shape.points.clone();
    loose_ends(m, &f, &mut points, std::slice::from_ref(&line));
    f.shape.points = points;
    f.shape.lines.push(line);
    let reply = json!({"id": id, "faces": shape_faces(m, &f), "top": top_range(m, &f)});
    put_shape(s, id, f.shape);
    Ok(reply)
}

/// Falls: a floor's or flat roof's shape worked out from its drains and the least fall (`falls::falls`), given anew.
fn falls(s: &mut Session, p: &Value) -> R {
    let c = "falls";
    let (id, f) = shaped_floor(c, s, p, &["id", "drains", "fall", "ridges"])?;
    let drains = points(c, p, "drains")?;
    let fall = num(c, p, "fall")?.ok_or_else(|| bad(c, "`fall`, the least fall in %, is required"))?;
    let ridges: Vec<[P2; 2]> = shaped(c, p, "ridges", "a list of lines [[x, y], [x, y]]")?.unwrap_or_default();
    if let Some(q) = ridges.iter().flatten().find(|q| !(q.0.is_finite() && q.1.is_finite() && q.0.abs().max(q.1.abs()) <= MAX_COORD)) {
        return Err(bad(c, format!("a ridge's end {q:?} is not a point in mm")));
    }
    let ridges: Vec<(P2, P2)> = ridges.into_iter().map(|[a, b]| (a, b)).collect();
    let out = crate::falls::falls(&f.boundary, &f.openings, &drains, fall, &ridges).map_err(|e| bad(c, e))?;
    let points: Vec<crate::model::ShapePoint> =
        out.drains.iter().chain(&out.points).map(|&(at, z)| crate::model::ShapePoint { at, z, ..Default::default() }).collect();
    let lines = out.lines.iter().map(|&(a, b)| crate::model::SplitLine { a, b, ..Default::default() }).collect();
    let f = Floor { shape: crate::model::Shape { points, lines }, ..f };
    let m = &s.model;
    // Every plane at least as steep as asked, its lowest corner a drain or an opening's corner a valley runs on from.
    let planes = m.shape_planes(&f).map_err(|e| bad(c, format!("the falls don't fold into planes: {e}")))?;
    let low = |q: P2| out.drains.iter().chain(&out.points).any(|(r, z)| r.dist(q) <= TOL && *z < 0.0);
    for (loops, plane) in &planes {
        let ring = loops.first().map(Vec::as_slice).unwrap_or_default();
        let steep = plane.g.length() * 100.0 >= fall - 1e-6;
        let lowest = ring.iter().copied().min_by(|a, b| plane.at(*a).total_cmp(&plane.at(*b)));
        if !steep || !lowest.is_some_and(low) {
            let q = geom::centroid(ring);
            return Err(bad(
                c,
                format!(
                    "round ({:.0}, {:.0}) the top can't fall {fall} % to a drain: add a drain there, or a ridge to part it off",
                    q.0, q.1
                ),
            ));
        }
    }
    let drains: Vec<Value> = out.drains.iter().map(|(at, z)| json!({"at": at, "z": z})).collect();
    let reply = json!({"id": id, "drains": drains, "faces": shape_faces(m, &f), "top": top_range(m, &f)});
    put_shape(s, id, f.shape);
    Ok(reply)
}

/// Reset Shape: a floor's points and split lines all taken off; it's flat at its offset again.
fn reset_shape(s: &mut Session, p: &Value) -> R {
    let c = "reset_shape";
    let (id, mut f) = shaped_floor(c, s, p, &["id"])?;
    f.shape = Default::default();
    put_shape(s, id, f.shape);
    Ok(json!({"id": id}))
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
            let word = m.language.word(if mode == "dependent" { msg!("Dependent") } else { "Copy" });
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
    let name = string(c, p, "name")?.unwrap_or_else(|| m.language.word(msg!("Unnamed")).into());
    let size = string(c, p, "size")?.map_or_else(|| "A3".into(), |z| z.trim().to_ascii_uppercase());
    let id = s.model_mut().insert(Element::Sheet(Sheet { number: number.clone(), name, size }));
    Ok(json!({"id": id, "number": number}))
}

fn place(s: &mut Session, p: &Value) -> R {
    let c = "place";
    let m = &s.model;
    let sheet = match p.get("sheet") {
        Some(Value::String(n)) => m.sheet_numbered(n).ok_or_else(|| bad(c, format!("no sheet numbered `{n}`")))?,
        Some(v) => v.as_u64().filter(|id| m.sheet(*id).is_some()).ok_or_else(|| bad(c, format!("`sheet`: {v} is not a sheet")))?,
        None => return Err(bad(c, "`sheet`, a sheet's number or id, is required")),
    };
    let scale = num(c, p, "scale")?;
    let at = point_opt(c, p, "at")?;
    let view = match p.get("view") {
        Some(Value::String(n)) => match m.view_named(n) {
            Some(id) => id,
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
    if let Some(v) = p.get("transparency").filter(|v| !v.is_null()) {
        mat.transparency = v
            .as_u64()
            .filter(|t| *t <= 100)
            .and_then(|t| u8::try_from(t).ok())
            .ok_or_else(|| bad(c, format!("`transparency` is a whole percent, 0 to 100, not {v}")))?;
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
            let len = std::fs::metadata(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?.len();
            if len > texture::MAX_BYTES as u64 {
                return Err(bad(c, format!("{} is too large ({len} bytes)", path.display())));
            }
            let bytes = std::fs::read(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
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

/// What `form`, `symbol`, `delete_form` and `delete_symbol` did, for `dry_run`'s `changes`.
#[derive(Default)]
struct Patches {
    forms_replaced: usize,
    forms_patched: Vec<Id>,
    forms_deleted: Vec<Id>,
    symbols_replaced: usize,
    symbols_patched: Vec<Id>,
    symbols_deleted: Vec<Id>,
}

/// Explicit reloads retain part identities and project-only types. Instance overrides stay on their elements.
fn family_reload(mut incoming: crate::family::Family, old: &crate::family::Family, keep_values: bool) -> crate::family::Family {
    let compatible = |p: &crate::family::Parameter| {
        old.parameters
            .iter()
            .find(|q| q.name == p.name && q.kind == p.kind && q.instance == p.instance && q.formula.is_none() && p.formula.is_none())
    };
    // Keep incoming defaults on new types before replacing the family's defaults.
    if keep_values {
        for (name, values) in &mut incoming.types {
            if !old.types.contains_key(name) {
                for p in incoming.parameters.iter().filter(|p| !p.instance && p.formula.is_none()) {
                    values.entry(p.name.clone()).or_insert_with(|| p.value.clone());
                }
            }
        }
    }
    for (name, values) in &old.types {
        let only_in_project = !incoming.types.contains_key(name);
        let target = incoming.types.entry(name.clone()).or_default();
        if keep_values || only_in_project {
            for p in incoming.parameters.iter().filter(|p| !p.instance) {
                if let Some(q) = compatible(p) {
                    target.insert(p.name.clone(), values.get(&p.name).unwrap_or(&q.value).clone());
                }
            }
        }
    }
    if keep_values {
        for p in &mut incoming.parameters {
            if let Some(q) = compatible(p) {
                p.value.clone_from(&q.value);
            }
        }
    }
    let mut old = old.clone();
    crate::library::keep_parts(&mut old.forms, &mut incoming.forms);
    crate::library::keep_parts(&mut old.symbols, &mut incoming.symbols);
    crate::library::keep_parts(&mut old.connectors, &mut incoming.connectors);
    incoming
}

/// A project's families: read one (`family {name}` alone costs no undo step), change one, make one
/// or take one away. The whole change is worked out and checked before the model is touched, so
/// reading and `dry_run` leave the project as they found it, and a family [`Model::check`] would
/// refuse is never put in it: every error it has comes back at once, its warnings with them.
fn family(s: &mut Session, p: &Value) -> R {
    let c = "family";
    let overwrite = match p.get("overwrite") {
        None => None,
        Some(Value::String(v)) if matches!(v.as_str(), "no" | "version" | "version_and_values") => Some(v.clone()),
        Some(_) => return Err(bad(c, "`overwrite` must be no, version, or version_and_values")),
    };
    if overwrite.is_some() && string(c, p, "path")?.is_none() {
        return Err(bad(c, "`overwrite` is only for loading a family with `path`"));
    }
    // From a file: its fields under the name given, else its own, else the file's.
    let mut p = p.clone();
    if let Some(path) = string(c, &p, "path")? {
        let path = PathBuf::from(path);
        let size = std::fs::metadata(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?.len();
        if size >= MAX_FILE {
            return Err(bad(c, format!("{} is too large ({size} bytes)", path.display())));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| bad(c, format!("{}: {e}", path.display())))?;
        let loaded: Value = serde_json::from_str(&text).map_err(|e| bad(c, format!("{} is not a family: {e}", path.display())))?;
        let Value::Object(mut fields) = loaded else {
            return Err(bad(c, format!("{} is not a family: it isn't an object", path.display())));
        };
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let name = p.get("name").filter(|v| !v.is_null()).cloned().or_else(|| fields.remove("name")).unwrap_or(json!(stem));
        // Only the family it holds: what a file says to rename or delete isn't for it to say.
        let mut family: Map<String, Value> =
            ["category", "kind", "ifc", "parameters", "types", "forms", "symbols", "operation", "connectors"]
                .into_iter()
                .filter_map(|k| fields.remove(k).map(|v| (k.to_string(), v)))
                .collect();
        if let Some(policy) = &overwrite {
            let name =
                name.as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(c, "`name` must be a nonempty family name"))?;
            if let Some(old) = s.model.families.get(name) {
                let incoming =
                    if policy == "no" { old.clone() } else { serde_json::from_value(Value::Object(family)).map_err(|e| bad(c, e))? };
                if policy == "no" || incoming == *old {
                    let (_, warnings) = old.problems(&|m| s.model.materials.contains_key(m));
                    let mut reply = family_reply(name, old, &warnings);
                    reply["overwritten"] = json!(false);
                    if flag(c, &p, "dry_run")? {
                        reply["dry_run"] = json!(true);
                        reply["ok"] = json!(true);
                        reply["changes"] = family_changes(Some(old), old, &Patches::default(), None);
                    }
                    return Ok(reply);
                }
                let reloaded = family_reload(incoming, old, policy == "version");
                family = serde_json::from_value(serde_json::to_value(reloaded).map_err(|e| bad(c, e))?).map_err(|e| bad(c, e))?;
            }
        }
        family.insert("name".into(), name);
        // `dry_run` is the caller's, not the file's: a dry run of a file loads nothing.
        if let Some(dry) = p.get("dry_run") {
            family.insert("dry_run".into(), dry.clone());
        }
        p = Value::Object(family);
    }
    // The family to read, change or delete; without a name, the project's families are listed.
    let name = string(c, &p, "name")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    // Saved to a file a family of the project's is: the file `path` loads it back from.
    if let Some(file) = string(c, &p, "save")? {
        let name = name.ok_or_else(|| bad(c, "`name` is required: the family to save"))?;
        let f = s.model.families.get(&name).ok_or_else(|| bad(c, format!("there's no family `{name}` to save")))?;
        if flag(c, &p, "dry_run")? {
            return Ok(json!({"name": name, "dry_run": true, "ok": true, "path": file, "saved": false}));
        }
        let mut v = serde_json::to_value(f).map_err(|e| bad(c, e))?;
        v["name"] = json!(name);
        let text = serde_json::to_string_pretty(&v).map_err(|e| bad(c, e))?;
        std::fs::write(&file, text).map_err(|e| bad(c, format!("{file}: {e}")))?;
        return Ok(json!({"saved": name, "path": file}));
    }
    let users = |m: &Model, name: &str| -> Vec<Id> {
        m.elements.iter().filter(|(_, e)| e.family().is_some_and(|(f, _)| f == name)).map(|(id, _)| *id).collect()
    };
    let Some(name) = name else {
        if flag(c, &p, "delete")? || p.get("rename").is_some_and(|v| !v.is_null()) {
            return Err(bad(c, "`name` is required: the family's"));
        }
        let families: Vec<Value> = s
            .model
            .families
            .iter()
            .map(|(n, f)| {
                json!({"name": n, "category": f.category, "types": f.types.len(), "forms": f.forms.len(), "symbols": f.symbols.len()})
            })
            .collect();
        return Ok(json!({"families": families}));
    };
    let dry = flag(c, &p, "dry_run")?;
    if flag(c, &p, "delete")? {
        if !s.model.families.contains_key(&name) {
            return Err(bad(c, format!("there's no family `{name}`")));
        }
        let used = users(&s.model, &name);
        if !used.is_empty() {
            let ids: Vec<String> = used.iter().map(Id::to_string).collect();
            return Err(bad(c, format!("elements {} are of `{name}`; delete them first", ids.join(", "))));
        }
        if dry {
            let mut changes = family_changes(None, &Default::default(), &Patches::default(), None);
            changes["deleted"] = json!(true);
            return Ok(json!({"name": name, "dry_run": true, "ok": true, "warnings": [], "changes": changes}));
        }
        s.model_mut().families.remove(&name);
        return Ok(json!({"deleted": name, "warnings": []}));
    }
    // Read it, which changes nothing and costs no undo step: what it is, each of its types' values
    // worked out, and what's worth knowing. `dry_run` reads it the same way and says it changed nothing.
    let keys = [
        "category",
        "kind",
        "ifc",
        "parameters",
        "forms",
        "symbols",
        "operation",
        "connectors",
        "types",
        "rename",
        "form",
        "symbol",
        "delete_form",
        "delete_symbol",
    ];
    if !keys.iter().any(|k| p.get(*k).is_some_and(|v| !v.is_null())) {
        let f = s.model.families.get(&name).ok_or_else(|| bad(c, format!("there's no family `{name}`")))?;
        let (_, warnings) = f.problems(&|m| s.model.materials.contains_key(m));
        let mut reply = family_reply(&name, f, &warnings);
        if dry {
            reply["dry_run"] = json!(true);
            reply["ok"] = json!(true);
            reply["changes"] = family_changes(Some(f), f, &Patches::default(), None);
        }
        return Ok(reply);
    }
    let was = s.model.families.get(&name).cloned();
    let mut f = if overwrite.is_some() { Default::default() } else { was.clone().unwrap_or_default() };
    let field = |key: &str, e: serde_json::Error| bad(c, format!("`{key}`: {e}"));
    let mut patches = Patches::default();
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
        patches.forms_replaced = forms.len();
        f.forms = forms;
    }
    if let Some(v) = p.get("symbols").filter(|v| !v.is_null()) {
        let mut symbols: Vec<crate::family::Symbol> = serde_json::from_value(v.clone()).map_err(|e| field("symbols", e))?;
        crate::library::keep_parts(&mut f.symbols, &mut symbols);
        patches.symbols_replaced = symbols.len();
        f.symbols = symbols;
    }
    // One form or one symbol of the family, patched by its `id` or its `guid`: its fields merged over
    // what that one holds, a `null` taking one away, its place and its id and GUID kept. Whole lists,
    // `forms` and `symbols`, replace as they always have; these only touch the one they name.
    if let Some(v) = p.get("form").filter(|v| !v.is_null()) {
        patches.forms_patched.push(patch_part(c, &name, &mut f.forms, v, "form")?);
    }
    if let Some(v) = p.get("symbol").filter(|v| !v.is_null()) {
        patches.symbols_patched.push(patch_part(c, &name, &mut f.symbols, v, "symbol")?);
    }
    if let Some(list) = p.get("delete_form").and_then(Value::as_array) {
        for v in list {
            patches.forms_deleted.push(take_part(c, &name, &mut f.forms, v, "form")?);
        }
    }
    if let Some(list) = p.get("delete_symbol").and_then(Value::as_array) {
        for v in list {
            patches.symbols_deleted.push(take_part(c, &name, &mut f.symbols, v, "symbol")?);
        }
    }
    if let Some(v) = p.get("connectors").filter(|v| !v.is_null()) {
        let mut connectors: Vec<crate::family::Connector> = serde_json::from_value(v.clone()).map_err(|e| field("connectors", e))?;
        crate::library::keep_parts(&mut f.connectors, &mut connectors);
        f.connectors = connectors;
    }
    if let Some(op) = string(c, &p, "operation")? {
        f.operation = operation(c, &op)?;
    }
    // Which fixture or fitting it is; an empty one takes it away. `Family::check` says the kinds its category has.
    if let Some(kind) = string(c, &p, "kind")? {
        let kind = kind.trim();
        f.kind = (!kind.is_empty()).then(|| kind.to_string());
    }
    // The IFC class its components go out as, written as IFC writes it; `null` forgets it.
    match p.get("ifc") {
        None => {}
        Some(Value::Null) => f.ifc = None,
        Some(v) => {
            let class: crate::family::IfcClass = serde_json::from_value(v.clone()).map_err(|e| field("ifc", e))?;
            f.ifc = Some(class.written());
        }
    }
    if let Some(v) = p.get("types").filter(|v| !v.is_null()) {
        let mut types = serde_json::to_value(&f.types).map_err(|e| bad(c, e))?;
        merge(&mut types, v);
        f.types = serde_json::from_value(types).map_err(|e| field("types", e))?;
    }
    // Renaming, checked before the model is touched: onto a name another family has, refused.
    let rename = match string(c, &p, "rename")? {
        Some(to) => {
            let to = to.trim().to_string();
            if to.is_empty() {
                return Err(bad(c, "`rename` needs a name"));
            }
            if to != name && s.model.families.contains_key(&to) {
                return Err(bad(c, format!("there's a family `{to}` already")));
            }
            Some(to)
        }
        None => None,
    };
    let to = rename.clone().unwrap_or_else(|| name.clone());
    // Every error of what it comes to, each addressed to what has it, and every warning of it.
    let (errors, warnings) = f.problems(&|m| s.model.materials.contains_key(m));
    if !errors.is_empty() {
        // What it's worth knowing comes with what stops it: an author fixes both at once, and a
        // warning never hides behind the error that came first.
        let mut said: Vec<String> = errors.iter().map(|e| format!("family `{to}`: {e}")).collect();
        said.extend(warnings.iter().map(|w| format!("family `{to}`: warning: {w}")));
        return Err(said.join("\n"));
    }
    if dry {
        if overwrite.is_some() {
            let mut preview = (*s.model).clone();
            preview.families.insert(to.clone(), f.clone());
            preview.sync_library();
            preview.check().map_err(|e| bad(c, e))?;
        }
        let mut reply = family_reply(&to, &f, &warnings);
        reply["dry_run"] = json!(true);
        reply["ok"] = json!(true);
        reply["changes"] = family_changes(was.as_ref(), &f, &patches, rename.as_deref());
        return Ok(reply);
    }
    let m = s.model_mut();
    if let Some(to) = rename {
        m.families.remove(&name);
        m.library.family_renamed(&name, &to);
        for e in m.elements.values_mut() {
            if let Some(f) = e.family_mut()
                && *f == name
            {
                f.clone_from(&to);
            }
        }
    }
    m.families.insert(to.clone(), f.clone());
    Ok(json!({"name": to, "family": f, "warnings": warnings}))
}

/// A family as `family` reads it: what it is, each of its types' parameters worked out (formulas and
/// all), and what's worth knowing about it.
fn family_reply(name: &str, f: &crate::family::Family, warnings: &[String]) -> Value {
    let values = f
        .types
        .keys()
        .filter_map(|t| f.values_in(t, &BTreeMap::new(), crate::family::DEFAULT_WALL).ok().map(|v| (t.clone(), v)))
        .map(|(t, v)| (t, Value::Object(v.iter().map(|(k, v)| (k.clone(), val(v))).collect())))
        .collect::<Map<String, Value>>();
    json!({"name": name, "family": f, "values": Value::Object(values), "warnings": warnings})
}

/// A parameter's value worked out, as JSON: a number, or a material's name.
fn val(v: &crate::family::Val) -> Value {
    match v {
        crate::family::Val::Number(x) => json!(x),
        crate::family::Val::Text(t) => json!(t),
    }
}

/// A family's parameters by name, for what `dry_run` says changed.
fn by_name(f: &crate::family::Family) -> BTreeMap<&str, &crate::family::Parameter> {
    f.parameters.iter().map(|q| (q.name.as_str(), q)).collect()
}

/// What a change comes to, worked out but not made: the parameters and types added, taken away and
/// changed; the forms and symbols the lists were replaced with, and those patched or deleted by
/// their id; and the new name, if the change renames the family.
fn family_changes(was: Option<&crate::family::Family>, now: &crate::family::Family, patches: &Patches, rename: Option<&str>) -> Value {
    let (was_p, now_p) = (was.map(by_name).unwrap_or_default(), by_name(now));
    let words = |names: Vec<&str>| names.into_iter().map(str::to_string).collect::<Vec<_>>();
    let parameters = json!({
        "added": words(now_p.keys().copied().filter(|k| !was_p.contains_key(k)).collect()),
        "removed": words(was_p.keys().copied().filter(|k| !now_p.contains_key(k)).collect()),
        "changed": words(now_p.iter().filter(|(k, v)| was_p.get(*k).is_some_and(|w| w != *v)).map(|(k, _)| *k).collect()),
    });
    let types = json!({
        "added": now.types.keys().filter(|k| was.is_none_or(|w| !w.types.contains_key(*k))).cloned().collect::<Vec<_>>(),
        "removed": was.into_iter().flat_map(|w| w.types.keys().filter(|k| !now.types.contains_key(*k))).cloned().collect::<Vec<_>>(),
        "changed": now.types.iter().filter(|(k, v)| was.is_some_and(|w| w.types.get(*k).is_some_and(|o| o != *v))).map(|(k, _)| k.clone())
            .collect::<Vec<_>>(),
    });
    json!({
        "parameters": parameters,
        "types": types,
        "forms": {"replaced": patches.forms_replaced, "patched": patches.forms_patched, "deleted": patches.forms_deleted},
        "symbols": {"replaced": patches.symbols_replaced, "patched": patches.symbols_patched, "deleted": patches.symbols_deleted},
        "rename": rename,
        "deleted": false,
    })
}

/// One form or one symbol of the family, patched by its `id` or `guid`: the fields it's given are
/// merged over what that one holds, a `null` takes one away, and its place, its id and its GUID stay
/// as they are. `what` says which it is, for the messages: `form`, or `symbol`.
fn patch_part<T>(c: &str, family: &str, parts: &mut [T], patch: &Value, what: &str) -> Result<Id, String>
where
    T: crate::library::HasPart + serde::Serialize + serde::de::DeserializeOwned,
{
    let Value::Object(map) = patch else {
        return Err(bad(c, format!("`{what}` must be {{id | guid, …the {what}'s fields}}, not {patch}")));
    };
    let key = map
        .get("id")
        .or_else(|| map.get("guid"))
        .filter(|v| !v.is_null())
        .ok_or_else(|| bad(c, format!("`{what}` needs the {what}'s `id` or `guid`")))?;
    let at = part_at(c, parts, key, what, family)?;
    let mut whole = serde_json::to_value(&parts[at]).map_err(|e| bad(c, e))?;
    let mut patch = patch.clone();
    if let Some(map) = patch.as_object_mut() {
        map.remove("id");
        map.remove("guid");
    }
    merge(&mut whole, &patch);
    parts[at] = serde_json::from_value(whole).map_err(|e| bad(c, format!("{what} {} of family \"{family}\": {e}", shown(key))))?;
    let (id, _) = parts[at].ident();
    Ok(*id)
}

/// One form or one symbol of the family taken away, by its `id` or `guid`; the others keep their
/// places, their ids and their GUIDs.
fn take_part<T>(c: &str, family: &str, parts: &mut Vec<T>, key: &Value, what: &str) -> Result<Id, String>
where
    T: crate::library::HasPart,
{
    let at = part_at(c, parts, key, what, family)?;
    let mut gone = parts.remove(at);
    let (id, _) = gone.ident();
    Ok(*id)
}

/// The place in `parts` of the one `key` names, by its id or its GUID.
fn part_at<T: crate::library::HasPart>(c: &str, parts: &mut [T], key: &Value, what: &str, family: &str) -> Result<usize, String> {
    let wanted = serde_json::from_value::<Guid>(key.clone()).ok().filter(|g| !g.is_nil());
    let Some(wanted) = wanted else {
        return Err(bad(c, format!("`{what}` needs the {what}'s `id` or `guid`, not {key}")));
    };
    parts
        .iter_mut()
        .position(|part| {
            let (id, guid) = part.ident();
            *guid == wanted || wanted.pending() == Some(*id)
        })
        .ok_or_else(|| bad(c, format!("{what} {} is not of family \"{family}\"", shown(key))))
}

/// What a form or a symbol was named by, in a message: the id, or the GUID, as it was given.
fn shown(key: &Value) -> String {
    match key {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

fn symbol_set(s: &mut Session, p: &Value) -> R {
    use crate::symbol_set::{Names, SETS};
    let c = "symbol_set";
    let set = string(c, p, "set")?.ok_or_else(|| bad(c, format!("`set` is required: {}", SETS.join(", "))))?;
    if !SETS.contains(&set.as_str()) {
        return Err(bad(c, format!("no symbol set `{set}`: there is {}", SETS.join(", "))));
    }
    let names = match string(c, p, "names")? {
        Some(code) => Names::from_code(&code).ok_or_else(|| bad(c, format!("`names` is en, nl or fr, not `{code}`")))?,
        None => Names::of(s.model.language),
    };
    let m = s.model_mut();
    let (mut loaded, mut kept) = (Vec::new(), Vec::new());
    for (name, family) in crate::symbol_set::families(&set, names) {
        if m.families.contains_key(&name) {
            kept.push(name);
        } else {
            m.families.insert(name.clone(), family);
            loaded.push(name);
        }
    }
    Ok(json!({"set": set, "loaded": loaded, "kept": kept}))
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
/// A connector, worked out, as JSON: its load in VA in all and in each phase.
fn connector_json(c: &crate::family::ConnectorValues) -> Value {
    json!({
        "id": c.id, "guid": c.guid, "kind": c.kind, "system": c.system, "poles": c.poles, "voltage": c.voltage,
        "load_va": c.load_va, "load_va_by_phase": c.load_va_by_phase, "power_factor": c.power_factor,
        "classification": c.classification,
    })
}

fn families(m: &Model) -> Value {
    let all: Map<String, Value> = m
        .families
        .iter()
        .map(|(n, f)| {
            let parameters: Vec<&str> = f.parameters.iter().map(|p| p.name.as_str()).collect();
            (
                n.clone(),
                json!({
                    "category": f.category, "types": f.types.keys().collect::<Vec<_>>(), "parameters": parameters,
                    "connectors": f.connectors.iter().map(|c| json!({"kind": c.kind, "system": c.system})).collect::<Vec<_>>(),
                }),
            )
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
            let op = match string(c, p, "operation")? {
                Some(name) => operation(c, &name)?,
                None => old.as_ref().and_then(|t| t.operation),
            };
            let t = OpeningType {
                width: width.or(old.as_ref().map(|t| t.width)).ok_or_else(|| need("width"))?,
                height: height.or(old.as_ref().map(|t| t.height)).ok_or_else(|| need("height"))?,
                sill: sill.or(old.as_ref().map(|t| t.sill)).unwrap_or(if cat == "window" { 900.0 } else { 0.0 }),
                operation: op,
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
        // A note's new text keeps its runs on what didn't change, unless it is given runs of its own.
        if let (Element::Text(old), Element::Text(n)) = (e, &mut e2)
            && old.text != n.text
            && !fields.contains_key("runs")
        {
            n.runs = crate::text::remap(&old.text, &n.text, &old.runs);
        }
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
        Element::Schedule(_) | Element::PlanView(_) => {
            return Err(bad(c, format!("{category} {id} has no place in plan; move its viewport on its sheet")));
        }
        // A camera view has a place in plan: its mark. The camera goes as a whole (the plan point of its eye and its
        // target, each keeping its height), so moving, copying, turning or mirroring it keeps it looking the same way.
        Element::View3D(v) => match v.camera {
            Some(cam) => {
                let lift = |p: [f64; 3]| {
                    let q = f(P2(p[0], p[1]));
                    [q.0, q.1, p[2]]
                };
                v.camera = Some(crate::model::ViewCamera { eye: lift(cam.eye), target: lift(cam.target) });
            }
            None => return Err(bad(c, format!("view3d {id} has no place in plan; move its viewport on its sheet"))),
        },
        Element::CadImport(ci) => {
            if mo.mirror {
                return Err(bad(
                    c,
                    format!("CAD import {id} can't be mirrored; mirror it in the program that made the drawing, or turn it"),
                ));
            }
            (ci.at, ci.angle) = turned(f, ci.at, ci.angle);
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
            // A flat one's shape goes with it, each point at its height.
            r.shape.points.iter_mut().for_each(|x| x.at = f(x.at));
            r.shape.lines.iter_mut().for_each(|l| (l.a, l.b) = (f(l.a), f(l.b)));
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
        Element::RoomSeparator(rs) => (rs.start, rs.end) = (f(rs.start), f(rs.end)),
        Element::Rebar(rb) => {
            for q in &mut rb.path {
                let moved = f(P2(q[0], q[1]));
                (q[0], q[1]) = (moved.0, moved.1);
            }
        }
        Element::Floor(fl) => {
            fl.boundary.iter_mut().chain(fl.openings.iter_mut().flatten()).for_each(|q| *q = f(*q));
            // A mirror turns the outline clockwise; floors keep theirs counter-clockwise.
            if mo.mirror {
                fl.boundary.reverse();
            }
            fl.arrow = fl.arrow.map(|a| a.map(f));
            (fl.pattern_origin, fl.pattern_angle) = turned(f, fl.pattern_origin, fl.pattern_angle);
            // Its shape's points and lines go with it, each at its height.
            fl.shape.points.iter_mut().for_each(|x| x.at = f(x.at));
            fl.shape.lines.iter_mut().for_each(|l| (l.a, l.b) = (f(l.a), f(l.b)));
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
        // A frozen room's boundary goes with it.
        Element::Room(r) => {
            r.point = f(r.point);
            if let Some(k) = &mut r.kept {
                k.map(f);
            }
        }
        Element::Door(o) | Element::Window(o) if !within(o.host) => match (mo.slide, m.wall(o.host)) {
            (Some(by), Some(w)) => o.offset += by.dot((w.end - w.start).unit()),
            _ => return Err(bad(c, format!("{category} {id} turns and mirrors only with its wall; select the wall too"))),
        },
        Element::Door(_) | Element::Window(_) => {}
        Element::Dimension(d) => d.at = f(d.at),
        // In plan, its points and edges; their heights stay.
        Element::Dimension3D(d) => {
            let flat = |q: &mut [f64; 3]| {
                let P2(x, y) = f(P2(q[0], q[1]));
                (q[0], q[1]) = (x, y);
            };
            flat(&mut d.at);
            for r in &mut d.refs {
                flat(&mut r.at);
                if let Some(e) = &mut r.edge {
                    e.iter_mut().for_each(flat);
                }
            }
        }
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
    // The walls, and what points at each: found among those, not every element.
    let lookup = m.lookup();
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
            let at = ([was.0, was.1, was.0, was.1], m.base_z(before), m.top_z(before));
            let host = lookup.walls_near_at(&[at], 2.0 * TOL).into_iter().find_map(|other| match m.elements.get(other) {
                Some(Element::Wall(o))
                    if !moving.contains(&other)
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
    // The walls whose ends may lie on a taken wall's line as it was.
    let boxes: Vec<_> = moved.iter().map(|(before, _)| (crate::derived::axis_box(before), m.base_z(before), m.top_z(before))).collect();
    let near = if boxes.is_empty() { BTreeSet::new() } else { lookup.walls_near_at(&boxes, 2.0 * TOL) };
    for id in near {
        let Some(w) = m.wall(id) else { continue };
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
    // Doors and windows in walls that changed keep their place, or go with their wall: those pointing at them, by id.
    let hosted: BTreeSet<Id> =
        walls.keys().filter_map(|w| m.elements.guid(*w)).flat_map(|g| lookup.referrers(&g).collect::<Vec<_>>()).collect();
    for (id, e) in hosted.into_iter().filter_map(|id| Some((id, m.elements.get(id)?))) {
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
                d.refs.iter_mut().filter(|r| turned(&r.id)).for_each(|r| *r = r.mirrored(hosts.contains_key(&r.id)));
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
                        let opening = matches!(m.elements.get(r.id), Some(Element::Door(_) | Element::Window(_)));
                        r.id = to;
                        if mo.mirror {
                            *r = r.mirrored(opening);
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

fn align_element(s: &mut Session, p: &Value) -> R {
    let c = "align";
    let id = id_opt(c, p, "id")?.ok_or_else(|| bad(c, "`id` is required"))?;
    let line = |name| -> Result<Line, String> {
        match points(c, p, name)?.as_slice() {
            [a, b] => Ok((*a, *b)),
            _ => Err(bad(c, format!("`{name}` must contain two points"))),
        }
    };
    let placement = crate::align::Placement::new(line("reference")?, line("line")?)?;
    let id = s.model.elements.id(id).ok_or_else(|| bad(c, format!("no element {id}")))?;
    if crate::align::lines(&s.model, id).is_empty() {
        return Err(bad(c, format!("element {id} has no straight alignment line in plan")));
    }
    if s.model.elements.get(id).and_then(|e| s.model.run_geom(e)).is_some_and(|g| g.vertical()) && placement.angle.abs() > 1e-9 {
        return Err(bad(c, "an upright run has no plan rotation; choose a parallel reference"));
    }
    if let Some(Element::Door(o) | Element::Window(o)) = s.model.elements.get(id)
        && let Some(w) = s.model.wall(o.host)
        && (placement.angle.abs() > 1e-9 || placement.by.cross((w.end - w.start).unit()).abs() > TOL)
    {
        return Err(bad(c, "a door or window can only slide along its host wall; align its jambs, or align the wall"));
    }
    let mo = Motion { map: &|q| placement.map(q), slide: (placement.angle.abs() <= 1e-9).then_some(placement.by), mirror: false };
    relocate(c, s, &[id], &mo, None)?;
    let (reference, line) = (line("reference")?, line("line")?);
    let moved = (placement.map(line.0), placement.map(line.1));
    let lock = if flag(c, p, "lock")? { Some(alignment_lock(c, s, id, id_opt(c, p, "to")?, reference, moved)?) } else { None };
    Ok(json!({"aligned": id, "angle": placement.angle, "by": placement.by, "line": [moved.0, moved.1], "lock": lock}))
}

/// Align's padlock: element `id`'s reference along `moved`, and the reference of element `to` (else of whatever has one) along
/// `reference`, kept on one line: a dimension between them, its one segment locked at 0 (`Dimension::alignment`), which
/// `constraints::keep` holds as it holds the others.
fn alignment_lock(c: &str, s: &mut Session, id: Id, to: Option<Guid>, reference: Line, moved: Line) -> Result<Id, String> {
    let m = &s.model;
    // Whether line `l` lies along the line through `on`.
    let along = |l: Line, on: Line| {
        let (u, len) = ((on.1 - on.0).unit(), on.0.dist(on.1));
        len > TOL && l.0.dist(l.1) > TOL && [l.0, l.1].iter().all(|q| (*q - on.0).cross(u).abs() <= TOL)
    };
    let of = |e: Id, on: Line| m.reference_lines(e).into_iter().find(|(r, l)| r.point.is_none() && along(*l, on)).map(|(r, _)| r);
    let mine = of(id, moved).ok_or_else(|| bad(c, format!("element {id} has no line along the one aligned to lock")))?;
    let theirs = match to {
        Some(g) => {
            let t = m.elements.id(g).ok_or_else(|| bad(c, format!("`to`: no element {g:?}")))?;
            of(t, reference).ok_or_else(|| bad(c, format!("element {t} has no line along the reference to lock to")))?
        }
        None => m
            .elements
            .keys()
            .filter(|e| **e != id)
            .find_map(|e| of(*e, reference))
            .ok_or_else(|| bad(c, "nothing has a line along the reference to lock to: a wall's face or centre, a grid…"))?,
    };
    let level = m
        .elements
        .get(id)
        .and_then(|e| m.level_of(e))
        .or_else(|| m.levels().first().map(|(l, _)| m.guid(*l)))
        .ok_or_else(|| bad(c, "the project has no level for the lock"))?;
    let d = Dimension {
        level,
        refs: vec![theirs, mine],
        at: micron((moved.0 + moved.1) * 0.5),
        type_name: crate::dims::default_type(&m.dimension_types, DimKind::Aligned).cloned().unwrap_or_default(),
        locked: vec![crate::model::Lock { segment: 0, length: 0.0 }],
        alignment: true,
        ..Dimension::default()
    };
    Ok(s.model_mut().insert(Element::Dimension(d)))
}

/// A typed value for a dimension's segment, as Revit's: what the segment ends at moves along the dimension (its second
/// reference, as the dimension's line runs) so the segment is that long; a locked segment is locked at it.
fn dimension_value(s: &mut Session, p: &Value) -> R {
    let c = "dimension_value";
    only(c, p, &["id", "value", "segment"])?;
    let id = element_id(c, &s.model, p.get("id").unwrap_or(&Value::Null))?;
    let Some(Element::Dimension(d)) = s.model.elements.get(id) else {
        return Err(bad(c, format!("{} {id} isn't a dimension", s.model.elements.get(id).map_or("nothing", |e| e.category()))));
    };
    if !matches!(d.kind, DimKind::Aligned | DimKind::Linear) || d.alignment {
        return Err(bad(c, format!("dimension {id} isn't an aligned or a linear one: only their lengths are typed")));
    }
    let mut d = d.clone();
    let value = num(c, p, "value")?.ok_or_else(|| bad(c, "`value`, the length in mm, is required"))?;
    if !(value > TOL && value <= MAX_COORD) {
        return Err(bad(c, format!("`value` is a length above 0 and at most {MAX_COORD} mm, not {value}")));
    }
    let (n, st) = s.model.stations(&d);
    let segment = count_opt(c, p, "segment", 0)?;
    let (Some((from, ..)), Some((to, _, r))) = (st.get(segment), st.get(segment + 1)) else {
        return Err(bad(c, format!("dimension {id} has segments 0 to {}, not {segment}", st.len().saturating_sub(2))));
    };
    let by = value - (to - from);
    let moved = s.model.elements.id(r.id).ok_or_else(|| bad(c, "what it measures to is gone"))?;
    if let Some(l) = d.locked.iter_mut().find(|l| l.segment == segment) {
        l.length = value;
        s.model_mut().elements.insert(id, Element::Dimension(d));
    }
    if by.abs() > 1e-9 {
        move_by(s, moved, n * by)?;
    }
    Ok(json!({"id": id, "moved": moved, "by": hundredth(by)}))
}

/// Element `id` moved `by` in plan, as `move` moves it: what keeps a dimension's constraints.
pub(crate) fn move_by(s: &mut Session, id: Id, by: P2) -> Result<(), String> {
    move_elements(s, &json!({"ids": [id], "by": [by.0, by.1]})).map(|_| ())
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

fn rename_type(s: &mut Session, p: &Value) -> R {
    let c = "rename_type";
    only(c, p, &["category", "family", "type", "name"])?;
    let to = name_param(c, p, "name")?.ok_or_else(|| bad(c, "`name` is required: the type's new name"))?;
    let (kind, from) = entry_of(c, &s.model, p)?;
    let label = kind.label();
    // A family's type is `family: type` in the library.
    let (to_entry, to) = match (&kind, from.split_once(": ")) {
        (LibKind::FamilyType, Some((f, _))) => (format!("{f}: {to}"), to),
        _ => (to.clone(), to),
    };
    if to_entry == from {
        return Ok(json!({"renamed": {"kind": label, "from": from, "to": to}}));
    }
    let m = s.model_mut();
    if m.library_names().iter().any(|(k, n)| *k == kind && *n == to_entry) {
        return Err(bad(c, format!("there's already a {label} `{to}`")));
    }
    if !m.move_entry(&kind, &from, &to_entry) {
        return Err(bad(c, format!("there is no {label} `{from}`")));
    }
    rename_entry(m, &kind, &from, &to_entry);
    let shown = |n: &str| n.split_once(": ").filter(|_| kind == LibKind::FamilyType).map_or(n, |(_, t)| t).to_string();
    Ok(json!({"renamed": {"kind": label, "from": shown(&from), "to": to}}))
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
    #[cfg(test)]
    let mut twin = s.clone();
    let result = delete_indexed(s, p);
    #[cfg(test)]
    {
        let expected = delete_from_scratch(&mut twin, p);
        assert_eq!(result, expected, "indexed deletion returned a different reply");
        assert_eq!(s.model, twin.model, "indexed deletion left a different model");
    }
    result
}

fn delete_indexed(s: &mut Session, p: &Value) -> R {
    let m = &s.model;
    let ids = ids_param("delete", m, p)?;
    let lookup = m.lookup();
    let mut gone: BTreeSet<Guid> = ids.iter().map(|id| m.guid(*id)).collect();
    let mut frontier = gone.clone();
    // Only referrers can be hosted by a deleted element. Keep the original cascade rules.
    while !frontier.is_empty() {
        let candidates: BTreeSet<Id> = frontier.iter().flat_map(|g| lookup.referrers(g)).collect();
        let mut next = BTreeSet::new();
        for id in candidates {
            let guid = m.guid(id);
            if gone.contains(&guid) {
                continue;
            }
            let hosted = match m.elements.get(id) {
                Some(Element::Door(o) | Element::Window(o)) => gone.contains(&o.host),
                Some(Element::Railing(r)) => r.host.is_some_and(|h| gone.contains(&h)),
                Some(Element::Rebar(rb)) => rb.host.is_some_and(|h| gone.contains(&h)),
                Some(Element::PlanView(v)) => gone.contains(&v.level) || v.parent.is_some_and(|p| gone.contains(&p)),
                Some(Element::Tag(t)) => gone.contains(&t.host) || t.view.is_some_and(|v| gone.contains(&v)),
                Some(Element::Dimension(d)) => d.view.is_some_and(|v| gone.contains(&v)),
                Some(Element::Dimension3D(d)) => gone.contains(&d.view),
                Some(Element::Viewport(v)) => gone.contains(&v.sheet) || gone.contains(&v.view),
                Some(Element::Text(n)) => gone.contains(&n.view),
                Some(Element::DetailLine(l)) => gone.contains(&l.view),
                Some(Element::FilledRegion(r)) => gone.contains(&r.view),
                Some(Element::DetailGroup(g)) => gone.contains(&g.view),
                Some(Element::RepeatingDetail(r)) => gone.contains(&r.view),
                Some(Element::Spot(sp)) => gone.contains(&sp.view) || sp.host.is_some_and(|h| gone.contains(&h)),
                Some(Element::CadImport(ci)) => ci.view.is_some_and(|v| gone.contains(&v)),
                _ => false,
            };
            if hosted {
                next.insert(guid);
            }
        }
        gone.extend(&next);
        frontier = next;
    }
    let mut dimensions: BTreeSet<Id> =
        gone.iter().flat_map(|g| lookup.referrers(g)).filter(|id| matches!(m.elements.get(*id), Some(Element::Dimension(_)))).collect();
    let mut kept = Vec::new();
    while let Some(id) = dimensions.pop_first() {
        let guid = m.guid(id);
        let Some(Element::Dimension(d)) = m.elements.get(id) else { continue };
        if gone.contains(&guid) || !d.refs.iter().any(|r| gone.contains(&r.id)) {
            continue;
        }
        let mut d = d.clone();
        d.refs.retain(|r| !gone.contains(&r.id));
        if d.refs.len() < 2 {
            gone.insert(guid);
            // The oracle visits dimensions once in id order; preserve that ordering even
            // when a dimension references another dimension that has just gone.
            dimensions.extend(
                lookup.referrers(&guid).filter(|other| *other > id && matches!(m.elements.get(*other), Some(Element::Dimension(_)))),
            );
        } else {
            kept.push((id, d));
        }
    }
    let mut deleted: Vec<Id> = gone.iter().filter_map(|g| m.elements.id(*g)).collect();
    deleted.sort_unstable();
    let ties: BTreeSet<Id> = gone
        .iter()
        .flat_map(|g| lookup.referrers(g))
        .filter(|id| match m.elements.get(*id) {
            Some(Element::Wall(w)) => w.attach_top.is_some_and(|g| gone.contains(&g)) || w.attach_base.is_some_and(|g| gone.contains(&g)),
            Some(Element::Roof(r)) => r.join.is_some_and(|g| gone.contains(&g)),
            // A 3D view whose work plane's level goes sketches on the lowest level again.
            Some(Element::View3D(v)) => v.work_plane.as_ref().is_some_and(|wp| wp.level.is_some_and(|l| gone.contains(&l))),
            _ => false,
        })
        .collect();
    // Linework is not currently part of Element::refs; inspect the small view index as
    // well as graphics overrides, without mutably visiting every model element.
    let views: BTreeSet<Id> = lookup
        .levels()
        .chain(["planview", "section", "elevation", "view3d"].into_iter().flat_map(|cat| lookup.rare(cat)))
        .filter(|id| !gone.contains(&m.guid(*id)))
        .filter(|id| {
            m.elements
                .get(*id)
                .and_then(graphics::graphics_of_element)
                .is_some_and(|g| g.elements.keys().any(|id| gone.contains(id)) || g.linework.iter().any(|l| gone.contains(&l.element)))
        })
        .collect();
    let m = s.model_mut();
    for id in &gone {
        m.elements.remove(id);
    }
    massing::forget(m, &gone);
    for id in views {
        if let Some(g) = m.elements.get_mut(id).and_then(graphics::graphics_mut) {
            g.elements.retain(|id, _| !gone.contains(id));
            g.linework.retain(|l| !gone.contains(&l.element));
        }
    }
    for (id, d) in kept {
        m.elements.insert(id, Element::Dimension(d));
    }
    for id in ties {
        match m.elements.get_mut(id) {
            Some(Element::Wall(w)) => {
                if w.attach_top.is_some_and(|g| gone.contains(&g)) {
                    w.attach_top = None;
                }
                if w.attach_base.is_some_and(|g| gone.contains(&g)) {
                    w.attach_base = None;
                }
            }
            Some(Element::Roof(r)) if r.join.is_some_and(|g| gone.contains(&g)) => r.join = None,
            Some(Element::View3D(v)) if v.work_plane.as_ref().is_some_and(|wp| wp.level.is_some_and(|l| gone.contains(&l))) => {
                v.work_plane = None
            }
            _ => {}
        }
    }
    Ok(json!({"deleted": deleted}))
}

#[cfg(test)]
fn delete_from_scratch(s: &mut Session, p: &Value) -> R {
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
                        Element::Rebar(rb) => rb.host.is_some_and(|h| gone.contains(&h)),
                        // A plan view goes with its level and its parent; what is annotated in a view goes with it.
                        Element::PlanView(v) => gone.contains(&v.level) || v.parent.is_some_and(|p| gone.contains(&p)),
                        Element::Tag(t) => gone.contains(&t.host) || t.view.is_some_and(|v| gone.contains(&v)),
                        Element::Dimension(d) => d.view.is_some_and(|v| gone.contains(&v)),
                        Element::Dimension3D(d) => gone.contains(&d.view),
                        Element::Viewport(v) => gone.contains(&v.sheet) || gone.contains(&v.view),
                        // Notes and spot elevations go with their view, spots with what they read.
                        Element::Text(n) => gone.contains(&n.view),
                        Element::DetailLine(l) => gone.contains(&l.view),
                        Element::FilledRegion(r) => gone.contains(&r.view),
                        Element::DetailGroup(g) => gone.contains(&g.view),
                        Element::RepeatingDetail(r) => gone.contains(&r.view),
                        Element::Spot(sp) => gone.contains(&sp.view) || sp.host.is_some_and(|h| gone.contains(&h)),
                        // An import in one view only goes with it.
                        Element::CadImport(ci) => ci.view.is_some_and(|v| gone.contains(&v)),
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
    // A 3D view whose work plane's level goes sketches on the lowest level again.
    for e in m.elements.values_mut() {
        if let Element::View3D(v) = e
            && v.work_plane.as_ref().is_some_and(|wp| wp.level.is_some_and(|l| gone.contains(&l)))
        {
            v.work_plane = None;
        }
    }
    for id in &gone {
        m.elements.remove(id);
    }
    // What was made from a mass that goes is ordinary from then on.
    massing::forget_from_scratch(m, &gone);
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

fn standard(s: &mut Session, p: &Value) -> R {
    let c = "standard";
    if let Some(code) = string(c, p, "set")? {
        let std = crate::standards::TechnicalStandard::from_code(&code)
            .ok_or_else(|| bad(c, format!("`set` is nbr, iso, ansi, eu or jis, not `{code}`")))?;
        let flag = |key: &str, default: bool| -> Result<bool, String> {
            match p.get(key) {
                None | Some(Value::Null) => Ok(default),
                Some(Value::Bool(b)) => Ok(*b),
                Some(v) => Err(bad(c, format!("`{key}` is true or false, not {v}"))),
            }
        };
        let adapt_all = flag("adapt_all", true)?;
        let (lw, os, un) =
            (flag("adapt_line_weights", adapt_all)?, flag("adapt_object_styles", adapt_all)?, flag("adapt_units", adapt_all)?);
        s.model_mut().apply_standard(std, lw, os, un);
    }
    let m = &s.model;
    let std = m.standard();
    Ok(json!({
        "standard": std.code(),
        "name": std.name(),
        "short_name": std.short_name(),
        "region": std.region(),
        "governing_body": std.governing_body(),
        "references": std.reference_standards(),
        "projection": std.projection_method().name(),
        "default_units": std.default_units().name(),
        "default_sheet_size": std.default_sheet_size(),
        "all": crate::standards::TechnicalStandard::ALL.iter().map(|s| json!({
            "code": s.code(),
            "name": s.name(),
            "short_name": s.short_name(),
            "region": s.region(),
            "description": s.description(),
            "references": s.reference_standards(),
        })).collect::<Vec<_>>()
    }))
}

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

fn room_area(s: &mut Session, p: &Value) -> R {
    let c = "room_area";
    only(c, p, &["at"])?;
    if p.get("at").is_some_and(|v| !v.is_string()) {
        return Err(bad(c, "`at` is centre or finish"));
    }
    if let Some(name) = string(c, p, "at")? {
        let at = RoomArea::parse(&name).ok_or_else(|| bad(c, format!("`at` is centre or finish, not `{name}`")))?;
        if at != s.model.room_area {
            s.model_mut().room_area = at;
        }
    }
    Ok(json!({"at": s.model.room_area.name()}))
}

fn georeference(s: &mut Session, p: &Value) -> R {
    let c = "georeference";
    const TEXTS: [&str; 5] = ["description", "geodetic_datum", "vertical_datum", "map_projection", "map_zone"];
    const NUMBERS: [&str; 5] = ["eastings", "northings", "height", "rotation", "scale"];
    let keys: Vec<&str> = ["crs", "clear"].into_iter().chain(TEXTS).chain(NUMBERS).collect();
    only(c, p, &keys)?;
    let given = keys.iter().filter(|k| **k != "clear").any(|k| !absent(p.get(*k)));
    match p.get("clear") {
        None | Some(Value::Null) => {}
        Some(Value::Bool(true)) if !given => {
            if s.model.georeference.is_some() {
                s.model_mut().georeference = None;
            }
            return Ok(json!({"georeference": null}));
        }
        Some(Value::Bool(true)) => return Err(bad(c, "`clear` takes the georeference away, and nothing else with it")),
        Some(Value::Bool(false)) => {}
        Some(_) => return Err(bad(c, "`clear` is true or false")),
    }
    if given {
        let crs = string(c, p, "crs")?;
        let mut g = match (&s.model.georeference, &crs) {
            (Some(g), _) => g.clone(),
            (None, Some(_)) => crate::model::Georeference { scale: 1.0, ..Default::default() },
            (None, None) => {
                return Err(bad(c, "the project has no georeference yet: give its coordinate reference system's name, `crs` (EPSG:31983)"));
            }
        };
        if let Some(crs) = crs {
            g.crs = crs;
        }
        let mut texts = [&mut g.description, &mut g.geodetic_datum, &mut g.vertical_datum, &mut g.map_projection, &mut g.map_zone];
        for (key, field) in TEXTS.into_iter().zip(texts.iter_mut()) {
            if let Some(t) = string(c, p, key)? {
                **field = Some(t).filter(|t| !t.trim().is_empty());
            }
        }
        // Map coordinates run past the model's own (`num`'s `MAX_COORD`): `Georeference::check` bounds them.
        let mut numbers = [&mut g.eastings, &mut g.northings, &mut g.height, &mut g.rotation, &mut g.scale];
        for (key, field) in NUMBERS.into_iter().zip(numbers.iter_mut()) {
            if let Some(v) = p.get(key).filter(|v| !v.is_null()) {
                **field = v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(c, format!("`{key}` must be a number, got {v}")))?;
            }
        }
        g.check().map_err(|e| bad(c, e))?;
        if s.model.georeference.as_ref() != Some(&g) {
            s.model_mut().georeference = Some(g);
        }
    }
    Ok(json!({"georeference": s.model.georeference}))
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

fn export_crosscheck(s: &mut Session, p: &Value) -> R {
    let c = "export_crosscheck";
    only(c, p, &["path"])?;
    let description = crate::crosscheck::describe(&s.model);
    match string(c, p, "path")? {
        Some(path) => {
            let text = serde_json::to_string_pretty(&description).map_err(|e| bad(c, e))?;
            crate::autosave::atomic_write(&PathBuf::from(&path), &text).map_err(|e| bad(c, e))?;
            Ok(json!({"path": path}))
        }
        None => Ok(json!({"description": description})),
    }
}

fn compare_crosscheck(s: &mut Session, p: &Value) -> R {
    let c = "compare_crosscheck";
    only(c, p, &["path", "text"])?;
    // A table's text is far longer than a name: `text` takes as much as a file does (`crosscheck::read` refuses more).
    let text = match p.get("text") {
        Some(Value::String(t)) => Some(t.clone()),
        Some(v) if !v.is_null() => return Err(bad(c, "`text` must be the text of a table")),
        _ => None,
    };
    let text = match (string(c, p, "path")?, text) {
        (Some(path), None) => {
            let size = std::fs::metadata(&path).map_err(|e| bad(c, format!("{path}: {e}")))?.len();
            if size > crate::crosscheck::MAX_TEXT as u64 {
                return Err(bad(c, format!("{path} is over {} MB", crate::crosscheck::MAX_TEXT >> 20)));
            }
            std::fs::read_to_string(&path).map_err(|e| bad(c, format!("{path}: {e}")))?
        }
        (None, Some(text)) => text,
        _ => return Err(bad(c, "give `path`, a table's file, or its `text`")),
    };
    let theirs = crate::crosscheck::read(&text).map_err(|e| bad(c, e))?;
    Ok(crate::crosscheck::compare(&crate::crosscheck::ours(&s.model), &theirs).to_json(100))
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
    only(c, p, &["name", "target", "lines", "drawn", "rename", "delete"])?;
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
    match p.get("drawn") {
        None => {}
        Some(Value::Null) => pattern.drawn = None,
        Some(v) => pattern.drawn = Some(v.as_f64().ok_or_else(|| bad(c, format!("`drawn` is a view's scale, a number, not {v}")))?),
    }
    let lines = shaped(c, p, "lines", "[{angle, spacing, origin?: [x, y], shift?, dashes?: [mm, -mm, …]}, …]")?;
    if let Some(t) = shaped::<Target>(c, p, "target", "drafting or model")? {
        pattern = if lines.is_none() { pattern.retargeted(t) } else { FillPattern { target: t, ..pattern } };
    }
    if let Some(lines) = lines {
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

fn make_fill_pattern(s: &mut Session, p: &Value) -> R {
    let c = "make_fill_pattern";
    only(c, p, &["ids", "from", "to", "name", "target"])?;
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let (from, to) = (point_req(c, p, "from")?, point_req(c, p, "to")?);
    let Some(name) = name_param(c, p, "name")? else { return Err(bad(c, "`name` the new pattern's is required")) };
    if m.fill_patterns.contains_key(&name) {
        return Err(bad(c, format!("there's already a fill pattern `{name}`")));
    }
    let target = shaped::<Target>(c, p, "target", "drafting or model")?.unwrap_or(Target::Model);
    let made = crate::fills::make::from_lines(m, &ids, from, to).map_err(|e| bad(c, e))?;
    let pattern = made.as_target(target);
    pattern.check(&name).map_err(|e| bad(c, e))?;
    s.model_mut().fill_patterns.insert(name.clone(), pattern.clone());
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
        // `component` first: the family categories given with it are given on top of it, in whatever order they came.
        let mut given: Vec<(String, Given)> = given.into_iter().collect();
        given.sort_by_key(|(cat, _)| cat != graphics::COMPONENT);
        for (cat, g) in given {
            crate::styles::known(&cat).map_err(|e| bad(c, e))?;
            let was = styles.get(&cat).copied().unwrap_or_else(|| crate::styles::inherited(&styles, &cat));
            let style = crate::styles::ObjectStyle { projection: g.projection.unwrap_or(was.projection), cut: g.cut.unwrap_or(was.cut) };
            style.check(&cat).map_err(|e| bad(c, e))?;
            // A category given what it starts with has nothing of its own to keep, unless `component` says another.
            if style == crate::styles::inherited(&styles, &cat) {
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
    let warnings = m.warnings();
    Ok(json!({
        "name": m.name, "units": "mm", "project_units": m.units.name(), "room_area": m.room_area.name(), "language": m.language.code(), "standard": m.standard().code(), "path": s.path, "levels": levels,
        "counts": counts,
        "types": m.types, "materials": m.materials, "images": images(m), "families": families(m), "parameters": m.parameters,
        "view_filters": m.view_filters, "view_templates": m.view_templates, "phases": m.phases, "phase_filters": m.phase_filters,
        "phase_overrides": m.phase_overrides, "option_sets": m.option_sets, "warnings": warnings,
        "voltages": m.voltages, "distribution_systems": m.distribution_systems, "wire_types": m.wire_types,
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
    let rooms = crate::rooms::Rooms::new(m);
    let page: Vec<Value> = hits.iter().skip(offset).take(limit).map(|id| element_json_with(m, *id, &rooms)).collect();
    Ok(json!({"count": hits.len(), "returned": page.len(), "elements": page}))
}
