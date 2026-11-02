# Terrain

Revit's toposurface. The geometry is `crates/core/src/topo.rs` (a TIN, heights, contours, areas; no model); the element is
`Element::Toposurface` (`model.rs`); the commands `toposurface`, `height_at` and `contours` are in `commands.rs`; the tests are `topo.rs`
and `topo_tests.rs`. OpenStreetMap buildings stand on it ([OSM import](osm-import.md)); the project's place on Earth is
[georeferencing](georeferencing.md).

## The element

`Toposurface`: `points` ([x, y, z] mm, **z above sea level, not over a level**), optional `breaklines` (polylines in 3D that triangle
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

## Not yet

A desktop tool (the Site tab; `TS` is free for it), pads and subregions, cut and fill, slope maps, contours cut by a section, points
from a point cloud, LandXML and DEM import (the planned `geofetch` crate would bring elevation tiles).
