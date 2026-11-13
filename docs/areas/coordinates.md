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

## Elevations, spots and schedules

`Origin` (base, survey, internal) says what a reading measures from: `Frames::elevation` turns an internal height into one
from the Project Base Point (`z - base.z`), the Survey Point (the site's shared elevation of it: its height plus `z`, never
scaled) or the internal origin, and `Frames::plan` a plan point into east and north from the base point, along the
project's axes, in shared coordinates, or as it is. A `Spot` has an `origin` (`Model::spot_reads`, what its text and the `elements` and `spot` replies say;
`spot_value` stays the internal height, which sections place it by) and, with `coordinate`, is a spot coordinate (below). A
`Level` has an `elevation_base`; its head in sections and elevations and the level schedule's Elevation show
`Model::level_shown`, and `elements` gives `elevation_shown` beside the internal `elevation` commands take. Columns, structural
columns, footings and components give `shared_east`, `shared_north` and `shared_elevation` (their point at their base), which
their schedules offer as Shared East, Shared North and Shared Elevation.

## Spot coordinates

A spot coordinate is a `Spot` with `coordinate`, in a plan only, of its own category (`spotcoordinate`, Spot Coordinates in
Visibility/Graphics, beside Spot Elevations). It has a type (`type`, `spots::SpotCoordinateType`, the project's
`spot_coordinate_types`, a library entry `Kind::SpotCoordinateType`), `leader` and `shoulder` (both on by default), and
`end`, where its leader ends (else 8 mm on paper up and right of its point, `render::leader_end`). Its elevation reads the top
of `host` at its point (what stands there when it is placed), else its level's. The type holds Revit's type parameters:
Rotate with Component; the leader's arrowhead (`filled`, `empty`, `dot`, `empty_dot`, `none`), the arrowhead's size, the
leader's and the arrowhead's pens, the leader's colour, the symbol on the point (none, a cross, a target) and its size; the
lettering (font, size, bold, italic, underline, width factor), the text's colour, its offsets from the leader and from the
symbol, above or below the leader, an opaque or transparent background; the Coordinate Origin (`base`, `survey`, `internal`;
`Origin::Internal` is the file's own numbers), the Top and Bottom Values (north/south, east/west, none), the indicators, the
elevation and its indicator, prefix or suffix, units and decimals, and alternate units (none, right of each number, or below
it) with their units, decimals, prefix and suffix. `SpotCoordinateType::lines` lays the text; `Model::spot_coordinate_text`
gives it, and `elements` replies with `text`, `east`, `north`, `elevation`, `type`, `leader` and `shoulder`.
`render/spots.rs` draws it, sizes in mm on paper: the symbol, the leader from the point to its end with the arrowhead on the
point (an empty one filled with paper so the leader doesn't show through), the shoulder as long as the text on the side the
leader leaves to, and the lines over it. Without a leader, the text stands beside the symbol.

A project starts with `Spot Coordinate - Project Base Point` and `Spot Coordinate - Survey Point` (in metres to four places,
as a surveyor's CAD reads a point out); `spot_coordinate` takes `type`, or without one the type that reads from `origin`
(`spots::default_type`), the Survey Point's when it gives none: after Acquire Coordinates, a spot coordinate reads what the
drawing's program says of its point. One from before types keeps reading from where it did. `spot_coordinate_type` lists, makes (`like`), changes, renames and deletes them.

Tests: `spot_coordinate_tests.rs`, `coordinates_tests.rs`. The desktop's tool and type dialog come with the Location window
(slice 7).

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

## Far coordinates at import and export (the bias)

Geometry stays near the internal origin; what comes in at map coordinates goes through the active site.

A civil drawing in metres often keeps AutoCAD's imperial default in its header (`$INSUNITS` inches). With units auto,
`import_cad` reads a drawing whose file says inches and whose middle is more than 50 000 units off its origin
(`looks_like_map`) in metres (the map system's unit once map systems name one, slice 6), and says so in `notes`; one that
says feet (US State Plane drawings are) is read in feet with a `warning`. Units given
are always kept. Placed origin to origin past what a model reaches, it is refused with the ways to bring it in.

- **CAD** (`import_cad`, `position: shared`): the drawing's coordinates are the map's, in its units. Its middle goes where
  `to_internal` puts it, turned `-angle` and scaled `scale / location.scale`, so every point of it is where the transform
  says. Past `MAX_COORD` it is refused with a message naming `acquire`; `acquire: true` first solves the active site
  (`Coordinates::acquire`, angle and scale kept) so the drawing's middle, rounded to the metre, is the internal origin.
- **Point clouds** (`point_cloud`, `origin: shared`): the scan's middle by `to_internal`, its height too, turned `-angle`;
  a scan has no scale, so a site's scale isn't applied to it. `acquire` takes the middle in plan and the lowest point's
  height, to the metre, so it stands on 0.

## Links

A link (`model::Link`) is placed `by` (`link::By`): `manual`, at `at` turned `angle` (origin to origin, as before); `base`,
its Project Base Point on this project's, turned `angle` about it and raised to it; or `shared`, where its shared
coordinates say: its active site (or the named site `location` names), carried to this project's active site. Its
origin's map coordinates go through this project's `to_internal`, its turn is the two angles' difference, and its levels
are raised by the difference in elevation. `link::placement` works this out each time the link is drawn (`bring`), so a
change to either project's site moves it, and nothing is written into the linked file. A link has no scale: where the
two locations' scales differ, `link` says so in a `warning` and places it by origin and turn alone. `link` replies with
where it stands (`at`, `angle`, `raised`). A link placed by `base` or `shared` isn't moved or turned by hand: `move` and
`rotate` refuse it and say to `modify` it with `by: manual` first.

`coordinates {acquire: <id>}` is Revit's Acquire Coordinates. From a link (a BIMCraft project, or an IFC file, whose
`IfcMapConversion` the import reads as its location): this project's active site is solved, rotation and scale from the
link's site, so that the link's origin, where it stands now, has the link's map coordinates here, and its map system
comes with it. From a CAD drawing placed in the project: its file's coordinates are taken for the map's, so where its origin
stands the shared coordinates are its (in mm, by its scale), its axes are the map's (the angle is less its turn), and the
elevation stays. The reply adds `acquired: {from, location}`. A Revit file can't be linked yet, and the Revit reader doesn't
read a file's survey or base point, so a Revit project's coordinates come over through its IFC export. In the desktop,
it is on the Location window's Manage tab: an Acquire button for each link and drawing, and Pick in the View, a tool
(`Tool::Acquire`) whose click takes a linked model's element (`link::link_of` finds its link) or a drawing.

`coordinates {reset: true}` is Reset Coordinates: the active site back on the internal origin (no offset, no turn, a scale
of 1), both points on it; the other sites and the map system stay. No link moves: each placed by base points or shared
coordinates is switched to `by: manual` where it stands, its height kept in `raised`. As Revit's file does, a link
remembers where it stands (`Link::placed`, kept up after every command while its file is loaded, `Model::links_moved`), so
a reset pins one whose file is gone where it last stood; one never loaded is refused.
Tests: the link tests in `coordinates_io_tests.rs`.

## Commands

- `georeference`: the active site as IFC has it, at the internal origin; any field makes one, `clear` takes it away.
- `coordinates`: reports everything (the three points, internal and shared, each with its id and GUID, the active site,
  every site as Revit shows it, and the georeference); `base` and `survey` move the points, `move: point|system` how the
  Survey Point goes.
- `location`: makes (a copy of the active site), edits, renames, activates and deletes a site, its numbers the Survey
  Point's; `shift` adds to one, or to every site when no `name` is given (the survey says +5 m: one command).
- `specify_coordinates`: the active site solved for a point.
- `convert_coordinates`: points between `internal`, `base` and `shared`, of the active site or the one named.
- `coordinates {acquire}` and `coordinates {reset}`: Acquire Coordinates from a link or a placed drawing, Reset Coordinates.
- `link`: `by: manual|base|shared` and `location`; `import_cad` (`position: shared`, `acquire`) and `point_cloud`
  (`origin: shared`, `acquire`).
- `spot_coordinate` (`type`, `end`, `leader`, `shoulder`, `host`, `origin`) and `spot_coordinate_type`; `origin` on `spot`
  and `elevation_base` on `level` (and `modify`).

Every change is a command and so one undo step. Tests: `coordinates_tests.rs`, and the payloads in the hostile sweep.

Not yet (the plan's later slices): Rotate Project North, plans turned to true north, map systems and WGS84, and the desktop.
