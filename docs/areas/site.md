# The site: toposolids

Revit 2024's Toposolid: the project's ground as a **solid** — a boundary on a level, a thickness under it, and a top shaped
by elevation points and split lines. The element is `Toposolid` (`model.rs`); the commands are `toposolid` (`TS`) and
`contours`; the tests are in `tests.rs`.

## The element

`Toposolid`: `type` (its type's name: what it is made of, see below), `level`, `boundary` (a simple ring in plan,
counter-clockwise; concave fine), `offset` (its flat top's height over the level), `shape` (points and split lines on its top,
a floor's `Shape`), `interval` (the mm between the contour lines plans draw; 1000, 0 for none), a `name` and `params` (phases,
design options, project parameters).

## The type: what it is made of

A toposolid type (`Model::types.toposolid`, a `FloorType`: `thickness` and `layers`) is a compound structure like a floor's,
from the top down: each layer a `function`, a `material` and a `thickness`. Its thickness is its layers' total, Revit's
**Default Thickness**, and that is how thick every toposolid of the type is: the instance's Thickness is read-only. The
project starts with `Generic - 300mm`, so a toposolid is 300 mm thick until its type says otherwise; `type {category:
toposolid, name, thickness? | layers?}` makes or changes one, and every toposolid of it follows. `Model::migrate` gives a file
from before `FORMAT` 20 that type.

A point's `z` is above the flat top, as a floor's shape points are. `toposolid`'s `points` are survey points whose z is in
project coordinates, and the command stores them relative to `elevation(level) + offset`. `add_point {absolute}` does the same
conversion.

## Create from Sketch, then Add Point

`toposolid {type?, sketch?, boundary?, points?, level?, offset?, interval?, name?}` makes one. Its outline comes as `sketch`
(the Sketch Editor's lines, solved into one ring as a floor's are) or as `boundary`; it refuses one that isn't simple
(the sketch solver: crossings, spikes, zero-length edges), a sketch with a loop inside its outline (a toposolid has no
holes), a point outside it, fewer than three corners, an interval out of range, and a type nothing has or one whose
thickness is 0. `Model::check` holds the same rules and checks the shape as a floor's is checked. A point may lie below the
level: the bottom follows the top, so the solid is its thickness everywhere (a pit is just a dip in the top), where Revit
refuses a point below its base.

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

- **Plans**, on its own level: its boundary, its contour lines at `interval`, and its type's top material's surface pattern —
  a named fill pattern or a built-in one — laid over the whole top in one frame, from `pattern_origin` and turned by
  `pattern_angle` (degrees counter-clockwise), so the pattern runs unbroken over a fold. `contours {id}` gives the lines as
  data.
- **3D**: its top's pieces, each a slab down to the plane under it, and its type's finish material's texture, on the plan's
  axes turned by the same angle from the same origin: a paving meets itself at a fold, and turns as the plan's pattern does.
- **Schedules**: category `toposolid`: Name, Level, Area, Volume, Thickness.
- **IFC**: an `IfcSlab` with `.BASESLAB.`, a closed faceted Brep, in the level's storey, with `Qto_SlabBaseQuantities`.
- **File**: `FORMAT` 20 (a build that reads up to 19 refuses a toposolid as an unknown element kind).
- **Visibility/Graphics and Object Styles**: the `toposolid` category.

## Not yet

- **Create from Import**: CSV points and 3D DXF contours.
- **The rest of the type**: contour display by type (start and end elevation, line style subcategories), the coarse-scale
  fill pattern and colour, a structural material, and the function. The interval is the instance's for now.
- **Its layers in 3D**: a toposolid is not split by its compound structure as a floor is, so its sides take its top
  layer's material and its lower layers show no colour there.
- **A variable layer**: a floor's or a roof's type may mark a layer `variable`, and a shaped one's underside then stays
  level at its base, that layer taking up the shape. A toposolid type may too (same `Layer`), but a toposolid ignores
  it: its bottom is always its top plumbed down by its layers' total, so the solid's volume is its plan area times that
  thickness. Honouring it would give a terrain the flat base Revit's `Variable` layer gives, and a body of earth under
  its lowest point instead of a slab thinning at every dip.
- **Its area**: `area_m2` is its boundary's plan area. There is no area of its 3D top, so a schedule has no sloped
  figure beside the plan one to choose between.
- **Split Element, and paint**: `split` takes walls and detail lines, so a toposolid cannot be sliced; its top has no
  face, so `paint` refuses it.
- **A wall attached to it**: `attach_top` and `attach_base` take a floor or a roof, not a toposolid.
- **Everywhere but its own plan**: only that level's plan draws it; another plan, or a section, does not, and its
  contours are drawn in plan only, not in 3D.
- **Smooth shading**: a shaped top is drawn faceted; there is no `Toposolid Smooth Shading` toggle.
- **A V/G surface-pattern override**: a view's override reaches a floor's top, not yet a toposolid's.
- **Sub-Divide**: a sub-area with its own offset, material and inherit-contours.
- **Graded Region**: a new toposolid for the proposed state, the old one Existing, and cut/fill.
- **Excavate**: a floor, roof or toposolid cutting it; hosting site components, parking, railings; slab edge.
- **An Options Bar at creation**: the command uses the default type (300 mm) and interval 1000; Properties changes them
  afterwards.
- **Editing the outline**: `edit_boundary` refuses a toposolid yet; delete it and make it again.
- **Snaps, picking and tags**: snaps and alignment guides don't know its points, only its lines pick it in plan (no fill),
  and a tag can't be hosted on it.
- **IFC import**: a `.BASESLAB.` comes in as a floor, not yet as a toposolid.
