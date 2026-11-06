# Coordinates

Where a project stands in the world: Revit's Project Base Point, Survey Point and shared coordinates, with the traps its
users complain of taken out. Why and what comes next: [the research](../coordinates-research.md) and
[the plan](../coordinates-plan.md).

## The model

`coordinates.rs` holds `Model::coordinates` (`Coordinates`), left out of a file while it is the default:

- `base`: the **Project Base Point**, internal mm. A reference people move anywhere, which elevations, spot coordinates and
  links will measure from; moving it moves no geometry.
- `survey`: the **Survey Point**, internal mm. A control point, usually a benchmark near the building: each location says
  what shared coordinates it has. There is no hidden shared origin and no clipped state. `move_survey` moves it with
  `Move::Point` (the point alone: every location's numbers are worked out again, so the building stays where it is in the
  world) or `Move::System` (the numbers stay, and the building moves in the world).
- `locations` and `active`: the project's **locations**, Revit's named sites, by name, and the one the shared coordinates
  are in. A `Location` is the Survey Point's `east`, `north` and `elevation` (mm), the `angle` from project north to true
  north (degrees, clockwise, so east, positive) and the `scale` from ground to grid (1 unless a map projection's; heights are
  never scaled). A project starts with one, `INTERNAL` ("Internal", in the project's language), at zero.

The transform, from internal `p` to shared, with `d = p - survey`:

    east  = location.east  + scale * (dx cos a - dy sin a)
    north = location.north + scale * (dx sin a + dy cos a)
    elev  = location.elevation + dz

`Location::to_internal` undoes it; `Coordinates::convert` goes between the frames `Internal`, `Base` (from the Project
Base Point, along the project's axes) and `Shared`. `specify` solves the active location so that one internal point has the
shared coordinates given (Revit's Specify Coordinates at a Point).

**Why the bias.** Geometry is kept near the internal origin, and map coordinates (a UTM northing is 7.5e9 mm) only ever
pass through the transform. f64 would hold such a number to a micrometre, but the math on geometry wouldn't hold it: the
graphics card's f32 steps are half a metre that far out, squared lengths lose their digits, and `TOL` and the
triangulation's grid assume small numbers. Shared values are bounded by `MAX_SHARED` (1e11 mm), internal ones by
`MAX_COORD`.

**Ids.** Each location is a library entry (`Kind::Location`), and so are the two points (`Kind::CoordinatePoint`, named
`BASE_POINT` and `SURVEY_POINT`), so all have an id and a GUID. `active` is a name reference in `Model::lib_refs_mut`, so a
file holds its GUID and a rename follows.

`Coordinates::check` (from `Model::check`) refuses a point out of range, no location, more than `MAX_LOCATIONS`, an active
name that isn't one, and a location out of bounds (`Location::check`: finite, within `MAX_SHARED`, an angle from -360 to 360,
a scale in `SCALE`, 0.9 to 1.1).

## A map's CRS and WGS84

`Coordinates::crs` is an EPSG code: a projected CRS the shared coordinates are a grid of. `crs.rs` looks it up in
`crs-definitions` (EPSG's definitions as proj strings, CC0; `lookup` refuses a code it lacks and a geographic one), names
it by its WKT's name, and gives its grid unit (metre, foot, US survey foot: shared coordinates stay mm, and the grid's own
numbers are the mm over `Info::mm`). `to_wgs84` and `from_wgs84` go through `proj4rs`, pure Rust PROJ.4: 2D, with the
definition's Helmert shift, so within centimetres of PROJ on a UTM or Lambert grid and metres where PROJ would use a grid
shift. That is what WGS84 is for here, the map and the sun; survey numbers never go through it. `search` scans every code
(a match, fast) for a code or all words of a name. EPSG's terms ask for the attribution in `NOTICE`.

`Frame::Wgs84` is [longitude, latitude, elevation mm]; `convert` returns an error without a CRS. `report` adds each
point's `wgs84` and the `crs`. `location` takes `latitude` and `longitude` for the Survey Point's east and north.

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

## Far coordinates at import and export (the bias)

Geometry stays near the internal origin; what comes in at map coordinates goes through the active location.

- **CAD** (`import_cad`, `position: shared`): the drawing's coordinates are the map's, in its units. Its middle goes where
  `to_internal` puts it, turned `-angle` and scaled `scale / location.scale`, so every point of it is where the transform
  says. Past `MAX_COORD` it is refused with a message naming `acquire`; `acquire: true` first solves the active location
  (`Coordinates::acquire`, angle and scale kept) so the drawing's middle, rounded to the metre, is the internal origin.
- **Point clouds** (`point_cloud`, `origin: shared`): the scan's middle by `to_internal`, its height too, turned `-angle`;
  a scan has no scale, so a location's scale isn't applied to it. `acquire` takes the middle in plan and the lowest point's
  height, to the metre, so it stands on 0.
- **IFC export** (`save` to .ifc, `ifc_origin`: internal, base, survey, shared; `ifc::export_at`): the site's placement puts
  the file's origin at that internal point, or (shared) the map's own coordinates, turned to true north. Where the project has
  a CRS or its location isn't zero, `IfcMapConversion` from the file's coordinates to `IfcProjectedCRS` (named `EPSG:<code>`
  and described by its name, or named after the location without one), in the CRS's unit (metre, foot, US survey foot as a
  conversion-based unit): eastings, northings and height of the file's origin, the x axis (cos, sin of the angle) and the
  scale; for `shared` the conversion is the identity, so nothing is turned twice. The context's `TrueNorth` agrees with it,
  and with a CRS the site has `RefLatitude`, `RefLongitude` (compound degrees) and `RefElevation`.
- **IFC import** (`import::georeference`): `IfcMapConversion` (its map unit, else the project's), else the context's
  `TrueNorth`, becomes the active location, composed with the shift a far model is moved by; a target named `EPSG:<code>`
  this version knows becomes the CRS. So what went out comes back where it was in the world, from every origin, and a model
  at map coordinates keeps them instead of losing the shift. IFC2x3's `ePSet_MapConversion` isn't read yet.

Tests: `coordinates_io_tests.rs`. Checked with IfcOpenShell 0.9 (`ifcopenshell.util.geolocation`): an export of a
project in SIRGAS 2000 / UTM 23S reads back as EPSG:31983 in metres, its x axis turned 10°, scale 0.9996, true north
agreeing, the site at -23.55, -46.633, and `local2global` puts the wall on the map coordinates the project gives it.

## Commands

- `coordinates`: reports everything (the internal origin and both points, internal and shared, the active location and every
  location); `base` and `survey` move the points, `move: point|system` how the Survey Point goes.
- `location`: makes (a copy of the active one), edits, renames, activates and deletes a location; `shift` adds to one, or to
  every location when no `name` is given (the survey says +5 m: one command).
- `specify_coordinates`: the active location solved for a point.
- `crs`: sets, clears or searches the CRS.
- `spot_coordinate`, and `origin` on `spot` and `elevation_base` on `level` (and `modify`).
- `convert_coordinates`: points between `internal`, `base`, `shared` and `wgs84`, of the active location or the one named.

Every change is a command and so one undo step. Tests: `coordinates_tests.rs`, and the payloads in the hostile sweep.

Not yet (the plan's later stages): links by shared coordinates, plans turned to true north, and the
desktop.
