# Coordinates: plan

What we build for coordinates, and in what order. The research behind it, covering Revit's behaviour and its users'
complaints, is in [coordinates-research.md](coordinates-research.md). This plan follows the owner's decisions of
2026-11-06.

## Decisions

1. **The Project Base Point stays.** It is the reference for elevations, spot coordinates and linking (PBP to PBP), and it
   can be moved anywhere without touching geometry. Every file's internal origin is its own, so a point people choose has
   to exist. Archicad's User Origin plays the same part.
2. **Real location through a chosen coordinate system.** Every projected CRS is in a list to pick from (by EPSG code or
   name). Through it, shared coordinates convert to WGS84, which puts the project on an OpenStreetMap map (always WGS84)
   and gives the sun its latitude and longitude.
3. **Many named locations, typed and fine-tuned.** Each one holds east, north, elevation and angle, and each value can be
   edited in a list. "Shift all" moves every location at once, for example when the survey says +5 m.
4. **Far coordinates through a bias.** Geometry stays near the internal origin, and every number shown, typed, imported or
   exported goes through the active location's transform. See "The bias" below.
5. **No Publish for now.** Something better comes later.
6. **Views turn between Project North and True North**, as Revit's do.
7. **Agents first.** Every fact and action is a command, and `info` reports it.

## The bias: does it work, and is there something better?

It works, and it is the established practice, not a workaround:

- IFC4's `IfcMapConversion` is exactly this: local engineering coordinates plus eastings, northings, orthogonal height,
  an x-axis direction (the angle) and a scale. Civil tools call it a "local engineering CRS" (ISO 19111). Bonsai/Blender
  and the point cloud tools (an f64 origin with f32 offsets, which our `cloud.rs` already does) work this way.
- Revit's shared coordinates are also a bias. Revit fails at imports and exports because they ignore it: CAD comes in at
  internal coordinates against a hard 20-mile limit.

**Why it is needed.** Storing f64 millimetres would hold a UTM northing (7.5e9 mm) to about 1e-6 mm, so plain storage
isn't the problem. The math on the coordinates is:

- the graphics card's f32 has about 512 mm steps at 7.5e9 mm, and 64 mm at 1e9;
- squared lengths and cross products lose most of their digits;
- `TOL` (1 mm) and the triangulation's 0.01 mm grid assume small numbers.

Keeping geometry near zero fixes all of these at once.

**What makes it better than "just add a number":**

- **The bias is the whole transform, not only a translation.** It covers east, north and elevation, an angle to True
  North, and a scale (grid to ground, 1 by default). A map grid isn't square to the building, and surveyors need the
  scale factor, which Revit lacks.
- **Elements never hold shared coordinates.** There is one transform per location, so changing it (+5 m) is one edit,
  never a rewrite of the model.
- **Imports default to "by shared coordinates":** the file's numbers go through the inverse transform. A far file in a
  project with no location yet offers to *acquire* one. Its bias is taken from the file and rounded to whole metres, so
  the shown numbers stay clean, and the geometry lands near zero. IFC import already does half of this (`import.rs`),
  but throws the shift away; it will keep it.
- **Exports write local coordinates plus the conversion where the format has one** (IFC: `IfcMapConversion` and
  `IfcProjectedCRS`). Where the format has none (a DXF export, if we ever write one), they add the transform back. f64
  text in DXF holds map coordinates fine.
- **A "near" limit.** `Model::check` refuses geometry farther from the internal origin than `MAX_COORD` (1e9 mm today).
  An import that would land past it after the transform is refused with a message that names the fix: set or acquire a
  location.

## The model

`coordinates.rs` (new):

- **Internal Origin.** Fixed, never stored, and drawn only on demand.
- **Project Base Point.** One per project, an element (`Element::BasePoint`, with an id and a GUID, pickable and
  pinnable), at an internal `P3`. It never moves geometry.
- **Survey Point.** One per project, an element too, at an internal `P3`, usually a benchmark near the building. It is a
  *control point*, not a hidden origin: each location says what shared coordinates it has. There is no Shared Site
  Origin and no clipped/unclipped flag. Its grip offers both of Revit's behaviours by name, so nobody does either by
  accident:
  - **Move point only:** its numbers update, and the building stays where it is in the world.
  - **Move the shared system with it:** its numbers stay, and the building moves in the world.
- **Locations.** Library entries (`Kind::Location`, ids and GUIDs). Each one holds:
  - `name`;
  - `shared: [E, N, Z]`: the Survey Point's shared coordinates, in mm;
  - `angle`: from project north to true north, in degrees;
  - `scale`: grid to ground, 1 by default.

  One location is active (`Model::coordinates.active`), and a project always has at least one: "Internal", with zeros.
  The transform is `shared(p) = R(angle) * (p - survey) * scale + location.shared`.
- **CRS.** One per project (`Option<Crs>`: EPSG code, name, unit). Every location is in it. The CRS's unit is shown and
  typed: metres, feet, and US survey feet as exactly 1200/3937 m, which Revit can't.
- **File format.** It goes from 17 to 18. `migrate` gives an older file the two points at the origin, an "Internal"
  location, and no CRS.

**Map and sun only.** WGS84 is used only for the map and the sun. `crs-definitions` (CC0, EPSG's definitions as proj
strings) and `proj4rs` (MIT OR Apache-2.0, pure Rust) do shared → geographic. Their datum shifts are good to about a
metre, which is plenty for a map and the sun, and survey numbers never go through them. EPSG's terms of use ask for an
attribution, which goes in `NOTICE`.

## Commands (agents first)

| Command | Does |
| --- | --- |
| `coordinates` | Reports everything: the points (internal and shared), the active location, every location, the CRS, and latitude/longitude. Sets the Base Point and the Survey Point (`move: point \| system`). |
| `location` | `add`, `edit` (any of E, N, Z, angle, scale), `rename`, `delete`, `activate`, `shift` (a delta to every location, or to the ones named). |
| `specify_coordinates` | A picked internal point and its shared E, N, Z (and optionally an angle): solves the active location. |
| `convert_coordinates` | Points between `internal`, `base`, `survey`/`shared` and `wgs84`, either way. |
| `crs` | Sets or clears the project's CRS. `search` lists matches by code or name, for the picker and for agents. |
| `rotate_project_north` | (stage 6) Turns the geometry about a point and makes up the angle of every location, so nothing moves in the world. |

Existing commands gain parameters:

- `spot` gets `from`;
- levels get an elevation base;
- `link` gets `by`;
- `import_cad`, `point_cloud` and `import_ifc` get `positioning`;
- `export_ifc` gets `origin`.

`Model::check` adds these rules, each with an error that says what to fix:

- every value is finite;
- |E|, |N| and |Z| ≤ 1e11 mm;
- the angle is between -360 and 360;
- the scale is between 0.9 and 1.1;
- location names are unique;
- there is exactly one active location;
- the CRS code is known.

## Stages

Each stage is test-first through commands, and gets its own commit. Targeted tests while working; the full checks before
a push.

1. **Core.** The model above, `coordinates.rs`, the commands `coordinates`, `location`, `specify_coordinates` and
   `convert_coordinates` (without WGS84), the check rules, format 18 and migrate, and `info`. Also the hostile-params
   sweep.
2. **CRS and WGS84.** `crs`, the definitions and the transform, CRS units including US survey feet, and
   `convert_coordinates` to and from WGS84. Tested against published control points: SIRGAS 2000 / UTM 23S, a UTM zone
   in the north, a US State Plane zone in US feet, and a British National Grid point. Where available, crosscheck with
   IfcOpenShell's or pyproj's numbers.
3. **Elevations and spots.**
   - A level type's Elevation Base: Project Base Point or Survey Point (shared).
   - Spot elevations `from`: relative, base or survey.
   - A new **spot coordinate** with a type: origin, N/E order, prefixes, and the CRS unit.
   - Schedule fields for shared E, N and Z.
   - Tested through PNG pixels and schedule rows.
4. **The bias at import and export.**
   - DXF/DWG and point clouds: positioning by shared coordinates (the default), origin to origin, center to center or
     base point, and acquire from the file.
   - IFC import reads `IfcMapConversion`, `IfcProjectedCRS` and IFC2x3 `ePSet_MapConversion` into a location, or keeps
     the far shift as one.
   - IFC export takes `origin` (internal, base, survey or shared) and writes `IfcMapConversion`, `IfcProjectedCRS`, and
     `IfcSite` `RefLatitude`, `RefLongitude` and `RefElevation`, with `TrueNorth` agreeing and the angle applied only
     once.
   - Tests: STEP references, a round trip (export, import, the same numbers), and IfcOpenShell's georeference reading.
5. **Links.** `by`: manual, origin to origin, base point to base point, or shared coordinates (with which of the link's
   locations). Worked out at each load, never written into the linked file. Acquire a location from a link.
6. **Views.**
   - A plan's `orientation`: Project North or True North. It turns the view (the canvas already draws rotated), never
     the model.
   - A north arrow annotation that reads the angle.
   - `rotate_project_north`.
   - Markers for the Internal Origin, the Base Point and the Survey Point in plans, sections and the 3D view, as a
     Visibility/Graphics category that is hidden by default.
7. **Desktop.**
   - Manage > Coordinates: an editable locations table with Shift All, the CRS picker with search, and
     Specify/Report/Convert.
   - Properties for the two points.
   - Their grips, which ask: point only, or the system.
   - The cursor read out in shared coordinates.
   - Positioning in the import and link dialogs.
   - Tested with egui_kittest, and looked at in the three looks.
8. **Map and sun.**
   - The Location dialog shows OpenStreetMap tiles in WGS84 with the building's footprint at its place, and a click
     there can set latitude and longitude.
   - `sun`: the sun's azimuth and altitude for a place, date and time, from the public NOAA formulas.
   - **This needs a rule change**, because today only `crates/update` reaches the network. The proposal: a
     `crates/map` under the same rules for hostile input as `crates/update`.
     - OSM's tile usage policy: a User-Agent naming BIMCraft, visible "© OpenStreetMap contributors" attribution, no
       bulk prefetching, and a cache that honours the headers.
     - Off with one setting, and never reached by tests.

## Left open

- **Publish, or something better.** Decision 5.
- **The map's tile server.** OSM's own servers or a configurable URL, decided at stage 8.
- **A scale factor worked out from the CRS** at the site, rather than typed. After stage 2.
- **IFC4.3's further georeferencing entities.** Checked against the schema before stage 4.
