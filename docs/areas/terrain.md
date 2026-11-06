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
from the element's own level. What is not over the terrain is left and said. `import_osm` does the same for the masses it makes, and `fetch_terrain` does it for the masses an import made earlier (and for their walls, floors and roof made by face), so the order of the two fetches does not matter ([geofetch](geofetch.md)); `stand_buildings: false` leaves them. Only the imported buildings follow a new terrain: what the user drew or placed by hand is theirs, and `place_on_terrain` stands it.

## Heights

A level's elevation and a terrain's z are both project coordinates, so a building at terrain height 3 m is 3 m over Level 1 whatever the
site's height above sea level; data in metres above sea level (a DEM) must have the project origin's `elevation` taken off first.

## The terrain is locked

Terrain is the one category plans and the 3D view will not pick, box or drag by accident (`graphics::locked`, `Model::pickable`), as Revit's pinned
elements are not: a click, a Tab step, a box (crossing or inside) and the start of a drag skip it, and in 3D a pixel of it picks nothing. Hold
**Ctrl+Alt** for a click (it replaces the selection; it is not Ctrl's add), or tick **Select Terrain** in the view control bar (shown only when the
project has terrain; `BimApp::select_terrain`, this window's and not the project's, so a file holds nothing of it and a new project keeps it) for
no key chord at all, as some Linux desktops take Alt+click. A click on terrain alone says how to pick it, once a session. It is a rule of the
windows' picking only: commands and agents name terrain by id as ever, Select All, Filter, Select by ID and the Project Browser reach it, and a
selection that holds it still moves and deletes. Only a plan's Modify tool is held back (Door, Dimension and the others look for other things).

## Aerial imagery

`fetch_imagery` lays aerial photographs over a toposurface ([geofetch](geofetch.md) has the sources, their terms and how a fetch goes). Its reply says which source made the picture and lists, as `skipped`, the sources tried before it with why they brought none.
It plans the tiles for the ground the toposurface covers (the box of its boundary, else of its points), asks the app's fetcher for them
(`GeoSource::imagery`; a build without one refuses), warps them into the project's own axes (`imagery::warp`: north up, the project's
turn undone) and keeps the result as a picture of the project (`Model::images`, at most 1024 px along its long side) on a material
named `Aerial imagery (<id>)` that the toposurface wears. The texture is `planar`: laid by where it is, once, with `size` the ground
it covers and `offset` its west and north corner, so the picture lies on the ground whatever the slope and its edge goes on beyond it
(the processor looks it up by east and north in `view3d::GROUND`, the card in `gpu.wgsl` by its spare word). Fetched again, from the
same source or another, it replaces the picture and the material, and one undo takes it back. The source, its credit and where its
terms are stay with the toposurface as the parameters Imagery Source, Imagery Credit and Imagery Terms. The 3D view shows the material's own colour where the picture has
no imagery (every mip level keeps its own alpha, `Level::alpha`, averaged as the colours are; `Sampler::at` takes the level's colour over white
less `(1 - a) * (255 - material)`, and the card's texels carry the alpha in their top byte and the material's colour in `lookups` word 7); **a plan leaves it clear** (below). The Site tab's Aerial Imagery button runs it ([the desktop](desktop.md#the-site-tab)). Not yet: imagery in the IFC export.

### Lining the photograph up with the map

A photograph and a map differ by metres, so `shift_imagery {id?, east?, north?, reset?}` moves the photograph a toposurface wears: `east` and
`north` are mm **from where the fetch put it** (the toposurface's west and north corner, `imagery::shift_of`), not from where it is, so
typing 2 m twice is 2 m, one alone keeps the other, `reset` puts it back, and up to a kilometre. It is the material's texture offset that
moves, so the 3D view (processor and card) and the plans show it at once, in one undo step; a photograph fetched again starts where the ground
is. The Site tab's **Align Imagery** (off when no terrain wears a photograph) opens a box with the shift east and north in the project's units,
Apply, Reset and Close; it stays open to be nudged again, and the plan shows the photograph move under its walls and masses.

### The photograph in plans

Every floor plan that shows the terrain draws the photograph under everything else (before the scans and the CAD imports), as
`render::aerial_imagery`: for each toposurface the view shows whose material wears a planar image texture, not turned (`angle` 0), one
`Prim::Image` of the window's own pixels, north up, made with the lookup the 3D view uses (`texture::Sampler`: the mip level a pixel
covers, blended between pixels; a row at a time, `Sampler::row`, which the tests hold equal to the point-by-point lookup). Only the part of
the picture the window shows is made, so the work and the memory are bounded by the window whatever the zoom (about 40 ms a
window of 1.5 megapixels, a plan being rebuilt on each step of a pan or a zoom; the timing is the ignored test
`a_window_full_of_photograph_is_drawn_in_a_moment`). It is cut to the terrain's boundary (an L-shaped terrain leaves its notch clear)
and **clear where the picture lets nothing through**: `Picture` keeps what a PNG lets through (`Picture::alpha`, a plane of one byte for
each pixel; `Sampler::coverage`) when it needs no shrinking, so a place with no imagery is paper in the plan, the PNG, the SVG and, masked,
the PDF; the 3D view reads the same picture, over the material's colour where it is clear, and a file written before or after is the same PNG it always was (no
`FORMAT` bump). It is nobody's (`owner` none): never picked, boxed, or snapped to. A plan that looks up (a ceiling plan) has none.

A view hides it, or tones it down, by its own category **Aerial Imagery** (`aerialimagery`, in Visibility/Graphics beside the model's
categories; `graphics::IMAGERY`, `Look::imagery`): hidden, or by **halftone** (half way to the paper) and **transparency** (that percent
let through), so a plan can have it faint under its linework, or none on the upper floors; a view template that controls Categories
controls it too. Nothing else of an override applies to it. A dark canvas dims a plan's pictures, the photograph with them; a sheet and a
PDF carry it as it is.

**The credit is drawn with the photograph, wherever it is shown.** A service's terms ask for its credit (EOX's make it mandatory), and
it travels with the data: the toposurface's `Imagery Credit` parameter, set by `fetch_imagery` from the table. `imagery::credits(model,
look)` is the credits of the photographs a view shows (the toposurfaces it shows that wear one, once each, sorted, at most
`MAX_CREDITS`, each one line cut at `MAX_CREDIT` characters: the parameter may hold anything). A plan letters them at its bottom right
(`render::credit_plate`: right-aligned, stacked upward, wrapped to the window's width, each line over a plate of paper so it reads over
the photograph, nobody's, last drawn), only when its photograph reached the window (so not in a ceiling plan, not where the view hides
the Aerial Imagery category or the terrain, not for a window the terrain is not in, not when transparency lets all of it through); a
section or elevation letters those of its look, as it draws the ground; a sheet's viewport letters them at paper size
(`draw_at`), and the PNG, the SVG and the PDF carry them because they carry the `Drawing`. The 3D view shows them as a label at its bottom
right in the window (whichever painter drew the view), and the two pictures the core makes of a 3D view, the `render` command's and a sheet's
3D viewport, are stamped with the same plate (`render::stamp_credits`). Not in the file or the IFC export, and the text only: the
address of a service's terms is the toposurface's `Imagery Terms` and the Site tab's result box.

## Not yet

A toposurface drawn by clicking points (the [Site tab](desktop.md#the-site-tab) has Pad, Place on Terrain, Cut/Fill and the fetches; `TS` is free), pads that grade the surface (a slope out to the ground) and subregions, slope maps, contours cut by a section, points
from a point cloud, LandXML. Elevation from tiles is `fetch_terrain` ([geofetch](geofetch.md)).
