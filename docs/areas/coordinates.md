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

## Commands

- `coordinates`: reports everything (the internal origin and both points, internal and shared, the active location and every
  location); `base` and `survey` move the points, `move: point|system` how the Survey Point goes.
- `location`: makes (a copy of the active one), edits, renames, activates and deletes a location; `shift` adds to one, or to
  every location when no `name` is given (the survey says +5 m: one command).
- `specify_coordinates`: the active location solved for a point.
- `convert_coordinates`: points between `internal`, `base` and `shared`, of the active location or the one named.

Every change is a command and so one undo step. Tests: `coordinates_tests.rs`, and the payloads in the hostile sweep.

Not yet (the plan's later stages): a CRS and WGS84, levels' and spots' elevation base and spot coordinates, imports and
exports through the transform (IFC's `IfcMapConversion`), links by shared coordinates, plans turned to true north, and the
desktop.
