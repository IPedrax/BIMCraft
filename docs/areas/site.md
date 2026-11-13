# The site: toposolids

Revit 2024's Toposolid: the project's ground as a **solid** — a boundary on a level, a thickness under it, and a top shaped
by elevation points and split lines. The element is `Toposolid` (`model.rs`); the commands are `toposolid` (`TS`) and
`contours`; the tests are in `tests.rs`.

## The element

`Toposolid`: `level`, `boundary` (a simple ring in plan, counter-clockwise; concave fine), `offset` (its flat top's height
over the level), `thickness` (300 by default; its bottom is that far under its top everywhere), `shape` (points and split
lines on its top, a floor's `Shape`), `interval` (the mm between the contour lines plans draw; 1000, 0 for none), a `name`
and `params` (phases, design options, project parameters).

A point's `z` is above the flat top, as a floor's shape points are. `toposolid`'s `points` are survey points whose z is in
project coordinates, and the command stores them relative to `elevation(level) + offset`. `add_point {absolute}` does the same
conversion.

## Create from Sketch, then Add Point

`toposolid {boundary, points?, level?, offset?, thickness?, interval?, name?}` makes one. It refuses a boundary that isn't
simple (the sketch solver: crossings, spikes, zero-length edges), a point outside it, fewer than three corners, and a
thickness of 0 or less or an interval out of range. `Model::check` holds the same rules and checks the shape as a floor's is
checked. A point may lie below the level: the bottom follows the top, so the solid is its thickness everywhere (a pit is
just a dip in the top), where Revit refuses a point below its base.

A toposolid is **shapeable**: `Model::shapeable` returns a `Floor` view of it (`toposolid_as_floor`), as `roof_as_floor` does
for a flat roof. So `add_point`, `edit_shape`, `add_split_line` and `reset_shape` work on it with no code of their own, and
the desktop's Add Point and Modify Sub Elements (Elevation Base, Along Surface or Absolute, the height labels and the 3D
grips) do too. `reset_shape` leaves it flat at its offset, its perimeter kept.

## Geometry

- `toposolid_faces`: its top in convex pieces, each with its plane (`floor_faces` of its view): one plane without a shape,
  its triangulated planes with one.
- `toposolid_solid`: a closed `Polyhedron`, its top's planes and, `thickness` under them, the same planes plumb (the bottom
  follows the top, so its volume is its plan area times its thickness).
- `toposolid_range`, `toposolid_volume`, `toposolid_slabs` (the 3D view's slabs).
- `toposolid_contours`: at each multiple of `interval` between its top's lowest and highest, the segments where the top
  crosses that height (at most `MAX_CONTOURS`, 1,000 levels, and `MAX_CONTOUR_SEGMENTS`, 20,000 segments in all). A corner
  exactly at a height counts as above it.

The triangulation is the floor shape editor's (`triangulation::constrained`), not a second TIN engine.

## Where it shows

- **Plans**, on its own level: its boundary, and its contour lines at `interval`. `contours {id}` gives the lines as data.
- **3D**: its top's pieces, each a slab down to the plane under it.
- **Schedules**: category `toposolid`: Name, Level, Area, Volume, Thickness.
- **IFC**: an `IfcSlab` with `.BASESLAB.`, a closed faceted Brep, in the level's storey, with `Qto_SlabBaseQuantities`.
- **File**: `FORMAT` 20 (a build that reads up to 19 refuses a toposolid as an unknown element kind).
- **Visibility/Graphics and Object Styles**: the `toposolid` category.

## Not yet

- **Create from Import**: CSV points and 3D DXF contours.
- **A toposolid type**: layers (structure), contour display by type (start and end elevation, line style subcategories).
- **Sub-Divide**: a sub-area with its own offset, material and inherit-contours.
- **Graded Region**: a new toposolid for the proposed state, the old one Existing, and cut/fill.
- **Excavate**: a floor, roof or toposolid cutting it; hosting site components, parking, railings; slab edge.
- **The desktop tool**: the Massing tab's Toposolid button and its Options Bar (a command creates one for now), and the
  Properties rows.
- **Editing the outline**: `edit_boundary` refuses a toposolid yet; delete it and make it again.
- **Snaps, picking and tags**: snaps and alignment guides don't know its points, only its lines pick it in plan (no fill),
  and a tag can't be hosted on it.
- **IFC import**: a `.BASESLAB.` comes in as a floor, not yet as a toposolid.
