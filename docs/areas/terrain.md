# Terrain

Revit's toposurface. The geometry is `crates/core/src/topo.rs` (a TIN, heights, contours, areas; no model); the element is
`Element::Toposurface` (`model.rs`); the commands `toposurface`, `height_at` and `contours` are in `commands.rs`; the tests are `topo.rs`
and `topo_tests.rs`. OpenStreetMap buildings stand on it ([OSM import](osm-import.md)); the project's place on Earth is
[georeferencing](georeferencing.md).

## The element

`Toposurface`: `points` ([x, y, z] mm, **z in project coordinates, as a level's elevation is: 0 is the height of the project's origin**,
which `geolocate`'s `elevation` puts above sea level; not over a level), optional `breaklines` (polylines in 3D that triangle
edges follow: ridges, valleys, kerbs), an optional `boundary` (a ring in plan; whole triangles outside it are dropped), a `material`
(none: the terrain's own colour), an `interval` (the mm between the contours plans draw: 1000 by default, 0 for none) and `params`
(so it has phases, design options and project parameters, category `toposurface`). No level: it is on every level's plan.

Caps (`Model::check`): 3 to `MAX_TERRAIN_POINTS` (50,000) points, `MAX_BREAKLINES` (1,000) breaklines of 2 to `MAX_POINTS` points,
an interval of 0 or at least `MIN_CONTOUR_INTERVAL` (10 mm). `Model::check` does not triangulate (it runs after every command);
the `toposurface` command does, so what makes no surface (fewer than three places, all on a line) is refused when it is made.
It moves, turns and mirrors in plan with its heights staying. File `FORMAT` 12 added it.

## The TIN

`topo::build(points, breaklines, boundary)` is `spade`'s constrained Delaunay triangulation. Of two points over one place the
breaklines' height and then the first stands. A breakline that would cross an earlier one is skipped, not a panic. `Tin::height_at`
finds the triangle through a grid index (None off the surface). `Tin::contours(interval, base)` cuts each triangle at every level (a
vertex exactly at a level counts as above it), joins the pieces by exact endpoints into open lines and closed rings, and refuses
more than `MAX_CONTOURS` (1,000) levels, naming the interval that would do. `plan_area`, `surface_area`, `z_range`.

`topo::cached` keeps the last 32 triangulations by a digest of their input, and each `Tin` keeps its contours (`contours_shared`), so
a plan drawn again, a height asked again, is not triangulating again.

## Where it shows

- **Plans** (`render.rs::terrain`): contours on every level (not in ceiling plans), every fifth from sea level heavier, in a
  category views can hide (`toposurface` is in `graphics::MODEL`, so also in Object Styles). A plan with only terrain fits to it.
- **3D** (`view3d.rs`): its triangles, in its material's colour; `tops_at` reads its height.
- **Schedules**: category `toposurface`: Name, Material, Area, Surface Area, Lowest, Highest, Points.
- **IFC**: an `IfcGeographicElement` (`.TERRAIN.`) with a face set, contained in the `IfcSite`, with its material. Import makes a
  toposurface of such an element, from its vertices (breaklines, boundary and interval are not in IFC).

## Pads, cut and fill, and standing things on the ground

**`pad`**: Revit's building pad, an element with a `level`, an outline flat at `offset` mm above it, and a `thickness` (300 by default): a slab
under its top. It is a category (`pad`: phases, project parameters, views may hide it), drawn as an outline on its own level's plan, a
slab in 3D, scheduled (Name, Level, Area, Top, Thickness, Cut, Fill, Net), moved, turned and mirrored in plan, and exported as an
`IfcSlab` (`.BASESLAB.`, object type `Pad`) on its storey, read back as a pad and never as a floor. `FORMAT` 13 added it.

**Cut and fill** (`Tin::cut_fill`, the `cut_fill` command, and `cut_m3`, `fill_m3`, `net_m3` in a pad's listing): cut is where the ground is above
the pad's top, fill where it is below; `net_m3` is fill less cut, so positive is earth to bring in. It is **exact**: the pad's outline is cut into
triangles (any simple outline, either way round, concave too), each is clipped to the triangles of the ground it meets, and in each piece the
ground less the pad's top is a linear function whose positive and negative parts are integrated, cutting the piece where it crosses zero.
Walls are vertical: the pad's edge is a step in the ground, not a slope. A pad partly off the terrain counts what is over it (`covered_m2`
against its whole `area_m2`). Two pads that overlap are counted twice each; keep them apart. `cut_fill {existing, proposed}` compares two
toposurfaces over where both are, by the midpoint of cells (`cell`, about 250,000 of them by default), so its volumes are near, not exact
(the error is the cells that straddle where the surfaces cross).

**`place_on_terrain {ids, terrain?, offset?}`** stands masses, columns, footings, components and walls on the ground: each at the lowest ground
under its footprint (corners and middle; a wall along its axis as a straight line), `offset` mm above it (negative for an embedment), measured
from the element's own level. What is not over the terrain is left and said. `import_osm` does the same for the masses it makes.

## Heights

A level's elevation and a terrain's z are both project coordinates, so a building at terrain height 3 m is 3 m over Level 1 whatever the
site's height above sea level; data in metres above sea level (a DEM) must have the project origin's `elevation` taken off first.

## Aerial imagery

`fetch_imagery` lays aerial photographs over a toposurface ([geofetch](geofetch.md) has the sources, their terms and how a fetch goes).
It plans the tiles for the ground the toposurface covers (the box of its boundary, else of its points), asks the app's fetcher for them
(`GeoSource::imagery`; a build without one refuses), warps them into the project's own axes (`imagery::warp`: north up, the project's
turn undone) and keeps the result as a picture of the project (`Model::images`, at most 1024 px along its long side) on a material
named `Aerial imagery (<id>)` that the toposurface wears. The texture is `planar`: laid by where it is, once, with `size` the ground
it covers and `offset` its west and north corner, so the picture lies on the ground whatever the slope and its edge goes on beyond it
(the processor looks it up by east and north in `view3d::GROUND`, the card in `gpu.wgsl` by its spare word). Fetched again, from the
same source or another, it replaces the picture and the material, and one undo takes it back. The source, its credit and where its
terms are stay with the toposurface as the parameters Imagery Source, Imagery Credit and Imagery Terms. A picture of a project has no
alpha, so a place without imagery shows white, not the terrain's colour. The Site tab's Aerial Imagery button runs it ([the desktop](desktop.md#the-site-tab)). Not yet: the plans' underlay, the credit
drawn in the views.

## Not yet

A toposurface drawn by clicking points (the [Site tab](desktop.md#the-site-tab) has Pad, Place on Terrain, Cut/Fill and the fetches; `TS` is free), pads that grade the surface (a slope out to the ground) and subregions, slope maps, contours cut by a section, points
from a point cloud, LandXML. Elevation from tiles is `fetch_terrain` ([geofetch](geofetch.md)).
