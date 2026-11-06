# Coordinates

Where a project stands in the world: Revit's Project Base Point, Survey Point and shared coordinates, with the traps its
users complain of taken out. Why and what comes next: [the research](../coordinates-research.md) and
[the plan](../coordinates-plan.md).

## The model

One location model: main's `Georeference` (`model.rs`, IFC's map conversion) is the project's **active site**, and
`coordinates.rs` adds what Revit has round it. #284 brought its own `Location` anchored at the Survey Point; it was folded
onto the georeference when it came onto main (2026-11-13), keeping its math and its commands, so IFC stays a straight
write.

- `Model::georeference`, the active site: the map point of the **internal origin** (`eastings`, `northings`, `height`,
  mm), `rotation` (degrees counterclockwise from the map's east to project x, which is the angle from project north to true
  north, clockwise: #284's `angle`) and `scale` (grid over ground, in plan only: heights stay true). A site needs no
  coordinate reference system (`crs` empty): any field makes one. None is the internal coordinates as they are
  (`coordinates::NONE`); `Model::set_site` keeps it none when a site comes back to that.
- `Model::coordinates` (`Coordinates`), left out of a file while it is the default:
  - `base`: the **Project Base Point**, internal mm. A reference people move anywhere, which elevations, spot coordinates
    and links measure from; moving it moves no geometry.
  - `survey`: the **Survey Point**, internal mm. A control point, usually a benchmark near the building. A site as Revit shows
    it, a `Location`, is the Survey Point's `east`, `north` and `elevation` in it, with its `angle` and `scale`
    (`Georeference::location`). Every site is anchored at the internal origin, so `Model::move_survey` with `Move::Point` (the
    point alone) changes only the point and the building stays where it is in the world; with `Move::System` every site is
    solved again (`Georeference::specify`) so the point keeps its numbers, and the building moves in the world.
  - `active` and `sites`: the active site's name (`INTERNAL`, "Internal" in the project's language, as a project starts) and
    Revit's other **named sites**, each a `Georeference` where it would put the project. `Model::activate_site` swaps one in
    as the georeference and keeps the one that was active beside it.

The transform, from internal `p` to shared, a the rotation and k the scale:

    east  = eastings  + k (x cos a - y sin a)
    north = northings + k (x sin a + y cos a)
    elev  = height + z

`Georeference::to_internal` undoes it; `Frames` (the coordinates read through a site, `Model::frames` for the active one)
converts between the frames `Internal`, `Base` (from the Project Base Point, along the project's axes) and `Shared`.
`Georeference::specify` solves a site so that one internal point has the shared coordinates given (Revit's Specify
Coordinates at a Point), its rotation and scale kept.

**Why the bias.** Geometry is kept near the internal origin, and map coordinates (a UTM northing is 7.5e9 mm) only ever
pass through the transform. f64 would hold such a number to a micrometre, but the math on geometry wouldn't hold it: the
graphics card's f32 steps are half a metre that far out, squared lengths lose their digits, and `TOL` and the
triangulation's grid assume small numbers. Shared values are bounded by `MAX_MAP` (1e13 mm, `on_map`), internal ones by
`MAX_COORD`.

**Ids.** Each site is a library entry (`Kind::Location`), and so are the three points (`Kind::CoordinatePoint`, named
`INTERNAL_ORIGIN`, `BASE_POINT` and `SURVEY_POINT`), so all have an id and a GUID. `active` is a name reference in
`Model::lib_refs_mut`, so a file holds its GUID and a rename follows.

`Coordinates::check` (from `Model::check`) refuses a point out of range, more than `MAX_LOCATIONS` sites, a name that is empty
or more than a line, the active name listed again among the others, and a site out of bounds (`Georeference::check`).

**FORMAT 34.** A build that reads up to 33 would refuse a georeference without a CRS and lose the points and sites, so files
with coordinates are 34. Files of 19 and 33 need no step: their georeference is the active site, both points at the
internal origin. #284's own files (its unreleased builds wrote 19) keep their points; its `locations` and `crs` are left
out and `open` lists them (`left_out`).

## IFC: one map conversion

`save` to `.ifc` takes `ifc_origin` (`ifc::Origin`: internal, the default; base; survey; shared) and writes through
`ifc::export_at`. The site's placement puts the file's origin at that internal point, or (shared) the map's own coordinates,
turned to true north. There is one `IfcMapConversion`, always the active site at the file's origin: its eastings, northings
and height are the site's shared coordinates of that point, in metres; its x axis is the site's rotation; and its scale, as
IFC4 has it, carries the unit with the ground-to-grid scale (0.9996 in metres is 0.0009996, as IfcOpenShell's `xyz2enh`
takes it). For `shared` the conversion turns and moves nothing, its scale only the unit; the site's placement can move and
turn but not scale, so there a point lies at its ground distance from the origin, not its grid one. The conversion and its
`IfcProjectedCRS` (named and described by the georeference's `crs` and texts) are written only when a coordinate reference
system is named. The context's `TrueNorth` is written whenever the project has a site, from its rotation (straight up for
`shared`). IFC4's single Scale scales heights too, which the model never does, so a reader drifts heights by the grid
factor; IFC4.3's FactorZ is written 1.

This fixes #342's merge, which kept main's conversion beside #284's: main's was anchored at the internal origin whatever
`ifc_origin` said, so it was off by the origin's offset (and twice for shared).

Import (`import::georeference`) reads the first conversion to a named CRS back as the site, composed with the shift a far
model is moved by; without one, the context's `TrueNorth` and that shift make a site with no CRS, so what went out at map
coordinates comes back keeping them. Tests: `coordinates_io_tests.rs` (`one_map_conversion_is_placed_at_the_files_origin`,
and the round trip from every origin), `georef_tests.rs`.

## Elevations, spots and schedules

`Origin` (base, survey) says what a reading measures from: `Coordinates::elevation` turns an internal height into one from
the Project Base Point (`z - base.z`) or the Survey Point (the location's elevation plus `z - survey.z`), and
`Coordinates::plan` a plan point into east and north from the base point, along the project's axes, or in shared
coordinates. A `Spot` has an `origin` (`Model::spot_reads`, what its text and the `elements` and `spot` replies say;
`spot_value` stays the internal height, which sections place it by) and, with `coordinate`, is a spot coordinate: in a plan
only, its north over its east beside a cross (`Model::spot_coordinate`, `render::coordinate_mark`), with no host. A
`Level` has an `elevation_base`; its head in sections and elevations and the level schedule's Elevation show
`Model::level_shown`, and `elements` gives `elevation_shown` beside the internal `elevation` commands take. Columns, structural
columns, footings and components give `shared_east`, `shared_north` and `shared_elevation` (their point at their base), which
their schedules offer as Shared East, Shared North and Shared Elevation.

## Commands

- `georeference`: the active site as IFC has it, at the internal origin; any field makes one, `clear` takes it away.
- `coordinates`: reports everything (the three points, internal and shared, each with its id and GUID, the active site,
  every site as Revit shows it, and the georeference); `base` and `survey` move the points, `move: point|system` how the
  Survey Point goes.
- `location`: makes (a copy of the active site), edits, renames, activates and deletes a site, its numbers the Survey
  Point's; `shift` adds to one, or to every site when no `name` is given (the survey says +5 m: one command).
- `specify_coordinates`: the active site solved for a point.
- `convert_coordinates`: points between `internal`, `base` and `shared`, of the active site or the one named.

Every change is a command and so one undo step. Tests: `coordinates_tests.rs`, and the payloads in the hostile sweep.

Not yet (the plan's later stages): a CRS and WGS84, levels' and spots' elevation base and spot coordinates, imports and
exports through the transform (IFC's `IfcMapConversion`), links by shared coordinates, plans turned to true north, and the
desktop.
