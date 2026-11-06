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

### Finding a system by place

People know where the site is, not which EPSG code its survey is in. `places.rs` finds countries and cities by name offline
(Natural Earth's countries, with their names in English, Portuguese, Spanish, French, German and Italian, and its ~7,300
populated places, biggest first: `assets/map/places.tsv`, 318 KB, written by `places.py`), accents aside (`fold`: "sao
paulo"), and later words may name the state or country ("paulo brasil"). Each place is paired with the systems surveys
there use, the one to pick first: the country's national grid (`grids`: British National Grid, Lambert-93, RD New, LV95…),
the place's UTM zone (or Gauss-Krüger band, MGA zone, Lo zone) on the country's datum (`zones`: SIRGAS 2000, ETRS89,
NAD83(2011), NAD83(CSRS), GDA2020, JGD2011, CGCS2000, POSGAR 2007, Hartebeesthoek94, Mexico ITRF2008), then the zone on
WGS 84, which is anywhere's. Zones are named as EPSG names them and looked up by that name, so one this version lacks is left
out, never invented; `every_country_s_systems_are_known` holds each pattern to the dataset. Which systems a country uses is
ours, a starting list: a country not in it gets the WGS 84 zone, and the name search finds the rest. `crs {search}` returns
the places beside the systems found by name: `places: [{kind, name, region, country, iso, longitude, latitude, systems:
[{code, name, unit, why}]}]`.

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

## Links

A link (`model::Link`) is placed `by` (`link::By`): `manual`, at `at` turned `angle` (origin to origin, as before); `base`,
its Project Base Point on this project's, turned `angle` about it and raised to it; or `shared`, where its shared
coordinates say: its active location (or the one `location` names), carried to this project's active location. Its
origin's map coordinates go through this project's `to_internal`, its turn is the two angles' difference, and its levels
are raised by the difference in elevation. `link::placement` works this out each time the link is drawn (`bring`), so a
change to either project's location moves it, and nothing is written into the linked file. A link has no scale: where the
two locations' scales differ, `link` says so in a `warning` and places it by origin and turn alone. `link` replies with
where it stands (`at`, `angle`, `raised`). A link placed by `base` or `shared` isn't moved or turned by hand: `move` and
`rotate` refuse it and say to `modify` it with `by: manual` first.

`coordinates {acquire: <link id>}` is Revit's Acquire Coordinates: this project's active location is solved, angle and
scale from the link's location, so that the link's origin, where it stands now, has the link's map coordinates here.
Tests: the link tests in `coordinates_io_tests.rs`.

## Site's three origins

Visibility/Graphics has a **Site** model category with three subcategories, `internalorigin`, `projectbasepoint` and
`surveypoint` (`graphics::SUBCATEGORIES`), hidden unless a view shows them, as Revit's are: `graphics {"view": …, "show":
["projectbasepoint"]}`, or their rows under Site in the dialog. A plan draws them (`render::origin_marks`), annotation-sized
and owned by no element, so never picked: the Internal Origin as a red x arrow and a green y arrow, the Project Base Point as
a circle with a cross in it, the Survey Point as a circle with a triangle in it. Hiding Site hides all three. Sections and the
3D view don't draw them yet.

## Ids

Every location is a library entry (`Kind::Location`), with an id and a GUID as every element and entry has, and so are the
two points (`Kind::CoordinatePoint`). The `coordinates` report gives `id` and `guid` for each location and for `base` and
`survey`, and `location` replies with the one it made or changed; a rename keeps both. The window's Locations table shows
the id (a new row has none until it is applied).

## Rotate Project North

`rotate_project_north` (`angle`, or `true_north: true` for the active location's angle; about `about`, else the Project
Base Point) turns every element with a place in plan, through `relocate` as `rotate` does, and both points; every
location's angle goes back by as much (wrapped to -180..180), and a link placed by base points turns with it, so nothing
moves in the world, only the project's axes. Levels, sheets, schedules, 3D views and links by shared coordinates stay (the
last follow the locations). Plan crops stay as they are.

## The desktop: Manage > Project Location

`crates/ui-egui/src/coordinates.rs`. The panel has **Coordinates**, which opens the window, and **Rotate Project North**,
a tool on the plan that works as Rotate does (`Tool::RotateNorth`, in `canvas.rs` beside `Tool::Rotate`): it starts on
the Project Base Point (Place Centre moves the centre), a click is the angle's start and the next its end, and between them
the whole plan turns as a ghost, with project north (up, muted) and true north (in the accent, as it will be once turned)
drawn from the centre and the angle at the cursor. The Options Bar takes an angle typed (Enter) and has **Align with True
North** (the active angle, in its name). It runs `rotate_project_north` with `about` the centre, one undo step, and goes
back to Modify. Its icon is a north arrowhead in a counter-clockwise turn arrow.

The window is designed to answer "where is this building?" at a glance, and to remove Revit's traps:

- **The summary**, always in sight above the tabs: a dial with project north straight up and true north as the accent's
  arrow at the active angle; the active location, switched in place; the Survey Point's shared East, North and Elevation in
  large monospace figures in the map's own unit; the angle; the map system as a chip (`EPSG:31983`) with its name; and,
  with one, the latitude and longitude.
- **Locations**: every location in a striped table, typed into (Revit's can only be made by moving things): an Active
  radio, the name, East, North and Elevation in the grid's unit, the angle and the scale. Add Location copies the active
  one. Delete is off for the active one and for the last one. **Shift every location** adds the same amounts to all of
  them, the "+5 m from the survey" case.
- **Points**: the Internal Origin, the Project Base Point and the Survey Point. Each shows its internal X, Y and Z, typed in
  the project's units, and its shared coordinates beside them, worked out live. Revit's two ways of moving the Survey Point
  (clipped and unclipped) are two named choices, each saying what it does to the building. Under them is **Specify
  Coordinates at a Point**.
- **Map System**: an offline **world map** (`world.rs`: Natural Earth's 1:110m coastlines and borders, public domain, 34 KB
  in `assets/map/world.bin`), a placeholder until stage 8's tiles: a plain latitude and longitude grid, dragged to pan and
  scrolled to zoom about the cursor, every location a pin (the active one in the accent), the latitude and longitude typed as
  a cross, and a click picks them for Place. Above it the map system is a dropdown: the button shows the one picked (its code as a chip, its name and unit) or
  none, and opens a search of places and systems as you type. **Places** lists each country or city found (a pin, its
  state and country, its latitude and longitude) with its systems under it, the first tagged Recommended and each saying
  why (national grid, the national datum's zone, WGS 84's); **Systems** lists EPSG's projected systems whose code or name
  matches. A system picked under a place also takes the place's latitude and longitude for Place and flies the map there
  (a country wide, a city close); the Survey Point waits for Place, since a city's middle is rarely the site. Clear beside
  it takes the map system away. **Place by latitude and longitude** puts the Survey Point on the globe.
- **Convert**: a calculator, changing nothing: a point between the internal, base, shared and WGS84 frames, with Swap and
  Copy.

It is a draft (`CoordinatesDialog`), worked out with the core's own `Coordinates`, and OK or Apply runs it as one undo step
named "Coordinates": `crs`, then the location renames (`renames::plan`), then `coordinates` with `move: system` (the draft
has already worked out what moving the point did to the locations), then each location changed, then `activate`, then the
deletes. A refusal stays in the window and says why. It looks the same in the three looks: only the theme's tokens
(`pal()`, `card`). Tests: `crates/ui-egui/src/tests/coordinates.rs`.

## Commands

- `coordinates`: reports everything (the internal origin and both points, internal and shared, the active location and every
  location); `base` and `survey` move the points, `move: point|system` how the Survey Point goes; `acquire` takes a link's.
- `link`: `by: manual|base|shared` and `location`.
- `location`: makes (a copy of the active one), edits, renames, activates and deletes a location; `shift` adds to one, or to
  every location when no `name` is given (the survey says +5 m: one command).
- `specify_coordinates`: the active location solved for a point.
- `crs`: sets, clears or searches the CRS; a search also finds countries and cities, each with its systems.
- `spot_coordinate`, and `origin` on `spot` and `elevation_base` on `level` (and `modify`).
- `rotate_project_north`: the model turned, the world kept.
- `convert_coordinates`: points between `internal`, `base`, `shared` and `wgs84`, of the active location or the one named.

Every change is a command and so one undo step. Tests: `coordinates_tests.rs`, and the payloads in the hostile sweep.

Not yet (the plan's later stages): plans turned to true north, the north arrow and the markers, and the points' grips and
Properties on the canvas.
