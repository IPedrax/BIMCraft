# Site: plan

What we build for the site, and in what order: terrain from points and contours, pads cut into it, and the cut and fill
quantities between the ground as it is and as it will be. It is the **Site** row of [ROADMAP.md](../ROADMAP.md) ("Toposolids from
points or contours (and imported DXF or LandXML), pads, property lines with bearings, cut and fill quantities"); property
lines are already in ([property boundaries](areas/property-boundaries.md)). The rules in [CLAUDE.md](../CLAUDE.md) apply to all
of it: everything is a command, `Model::check` validates, never crash, the core has no UI, test first, clean room.

**Status (2026-11-14).** Slice 1 is #442. The surface engine of slice 2, `crates/core/src/site/surface.rs`, is done: it
builds, clips and queries a terrain from points, contours and breaklines, as the "Slice 2" section below says, but it is not
an element yet. The element this plan proposed (approach A) is not built here: #407 adds Revit 2024's Toposolid, a
shape-editable slab on a level, and the site work now builds on it instead of adding a second terrain element. What stays
of slice 2 is the import: a CAD import's contours, through this engine, into #407's toposolid as its shape points (one on each
boundary corner) and split lines. Where this document describes the element itself (its fields, `FORMAT`, drawing,
schedules, IFC), #407 decides. Slice 3 (pads, cut and fill, excavation) is proposed as its own issue once #407 lands.

## Why

A building on a slope starts with the ground: where it sits, how high its floor is over the terrain at each corner, and how
much earth has to be dug out or brought in to make a platform for it. Today BIMCraft can show a surveyor's contours only as a
flat CAD import: no surface in 3D or in sections, no heights to read, nothing to measure volumes against. The first user this
is for has a drone survey of a steep plot (about 80 % average slope, 1 m contours, a DXF in UTM metres) and a timber cabin to
build on it, and needs the earthworks quantified before building.

## Goals

1. Contours from a DXF or DWG keep their elevations when read.
2. A **toposolid**: a terrain element made from points and contours (typed in, or taken from a CAD import), a triangulated
   surface over a solid, seen in plans (as contours), sections (cut, with its top line) and 3D, scheduled, and exported to IFC.
3. A **pad**: a flat platform at an elevation, with side slopes (cut and fill batters) down or up to the existing ground.
4. **Cut and fill** volumes for each pad and for the whole site, exact for a triangulated surface, so a user can price the
   earthworks from them.
5. All of it drivable by agents (commands with params docs written for agents), as everything else is.

## Non-goals (for these slices)

- LandXML import (a later slice; the toposolid's data model must not stand in its way).
- Editing a toposolid's points one by one from the desktop (Modify Sub Elements); a toposolid is recreated, moved, turned,
  retyped or deleted. Point editing comes later, the way shape editing came to floors.
- Subregions, graded regions that are not pads, roads and corridors, swales.
- Hosting elements on the terrain (a component placed on its surface) and walls attached to it.
- Contour labels in plans.
- Swell and compaction factors: volumes are bank (in-place) volumes; what a truck carries is the user's multiplication.
- A desktop tool or ribbon button beyond what drawing the element needs. Commands come first; the desktop can follow.

## Anti-goals

- **Wrong volumes are worse than none.** A cut and fill number that is off by more than rounding, silently, would be a
  failure even if everything else works. Every volume is tested against a case whose volume is known exactly.
- **No fake terrain.** A toposolid is its own element and category (not a floor in disguise), exported as
  `IfcGeographicElement` (TERRAIN), never `IfcSlab`.
- **A drawing still reads as before.** Keeping elevations must not change how any existing DXF draws, its extents or what
  `import_cad` returns, except for the new elevation fields.
- **No user data in the repository.** Test files are synthetic, written in tests. A user's survey is never committed.

## Approaches

Considered:

- **A. A toposolid element of its own (chosen).** A new element and category, its surface a constrained Delaunay
  triangulation of its points with the contours as breaklines (reusing `triangulation.rs`), its solid closed down to a flat
  bottom. It is what the roadmap names, it reads right in schedules and IFC, and pads and cut and fill build on it naturally.
  It costs a new element: a format bump, a check, drawing in each view, IFC.
- **B. A shaped floor used as terrain.** A floor of a "terrain" type with `edit_shape` points from the contours. Quick, since
  shaped floors already have 3D, sections, volume and IFC; but it schedules and exports as a slab, draws no contours, and
  cut and fill against it would be bent around a floor's rules. Rejected.
- **C. Contours meshed outside and brought in with `import_mesh`.** Cheapest, but it is a Generic Model: not editable, no
  semantics, nothing to compute volumes against. Rejected.

## Decisions

### Slice 1: elevations in DXF (one pull request)

- `dxf.rs` keeps the third coordinate where the file gives one: an LWPOLYLINE's elevation (group 38, one for the whole
  polyline), a 3D POLYLINE's vertices (group 30 on each VERTEX), a LINE's ends (30, 31) and a POINT (30). An entity with no
  third coordinate has none (not zero), so a drawing can tell "flat" from "at elevation 0".
- Elevations are in drawing units, as the plan coordinates are, and go through the same unit and scale. An entity whose
  extrusion direction is not +Z is handled as the reader handles it today for plan coordinates; an elevation in a mirrored
  OCS (extrusion 0, 0, -1) is negated (tested).
- A DWG keeps its elevations through the bridge in `crates/dwg` the same way (tested with a DWG made from DXF text, as that
  crate's tests do).
- `cad_query` (and `import_cad`'s reply) report elevations: for each layer the range of elevations its entities have and how
  many have one, so an agent can see that a drawing is a contour map before using it.
- Nothing else changes: drawing, extents, picking and snapping stay 2D.

### Slice 2: the toposolid (one pull request)

**Element.** `Element::Toposolid`, category `topography` (shown as Toposolid), with an id and a GUID like every element. It
holds, in internal millimetres:

- `points`: the surface's vertices, `[x, y, z]`, z an internal elevation (not over a level).
- `breaklines`: pairs of point indices the triangulation must keep as edges (the contour segments).
- `boundary`: an optional closed outline in plan the surface is clipped to; without one, the convex hull of its points.
- `type`: a toposolid type with a `thickness` (the solid goes down to a flat bottom `thickness` below its lowest point) and a
  material (an earth material, original, in the starter library).
- `name`, and the usual phase and workset-like fields an element of this model carries.

The triangulation is derived, never saved: rebuilt from points, breaklines and boundary (cached as other derived geometry is).
`FORMAT` goes up by one with a `Model::migrate` step; `lenient.rs` leaves a toposolid out of a file it can't read, saying so.

**Command `toposolid`.**

```text
toposolid {points?: [[x,y,z],…], contours?: [{z, points: [[x,y],…], closed?}, …], cad?: id|name, layers?: [name,…],
           boundary?: [[x,y],…], simplify?: mm, type?, name?}
```

- `points` and `contours` are typed in; `cad` takes every polyline, line and point with an elevation from a CAD import in
  the project (optionally only `layers`), placed where the import stands (its position, angle and scale), its elevations
  turned into internal ones through the active location (a survey's elevations are shared coordinates, as its plan
  coordinates are). A `cad` import with no elevations is refused, saying so.
- `simplify` (default 50 mm) drops contour vertices that lie within that distance of the line through their neighbours, so a
  dense survey stays light; 0 keeps them all.
- `ignore_loops_under` (m², default 0: none) leaves out closed contours enclosing less than that area: the small loops
  photogrammetry draws round trees and bushes when its contours come from a surface model rather than a terrain model. The
  reply says how many it left out.
- **Flat triangles.** A triangulation of contours makes triangles whose three corners lie on one contour, at hilltops, in
  valley bottoms and along ridges: flat terraces the ground does not have, which bias volumes. They are removed, as is usual
  practice: points are added along the middle of each flat region, their elevations interpolated between the contour it lies
  on and the neighbouring contours (or, at a summit or a pit with no neighbour, extrapolated from the slope around it), and
  the surface triangulated again. The reply counts the points added. Tested with a cone from circular contours: no flat
  triangle remains, and its volume is the cone's within the chord error.
- Contour segments are breaklines, and so are 3D polylines whose elevation varies along them. Points closer than 1 mm in
  plan are one. Noisy data still makes a surface, and the reply's `notes` say what was done (design call, after the first
  user's drone survey had 6,941 crossings among the contours round its trees, all between neighbouring elevations, which a
  refusal would have left with no terrain at all): segments that cross another are not kept as edges, their vertices stay
  as points, with how many and the box they lie in; a position given at two elevations takes their mean, with where.
- Limits: at least three points not on one line; at most the triangulation's own limit (`MAX_CORNERS` in
  `triangulation.rs`, 20,000 today, after simplifying; the first user's survey has 7,817 vertices), so a toposolid stays as
  fast in `Model::check` as a shaped floor; raising that shared limit, or a faster triangulator for big surveys, is a later
  slice (design call). Coordinates finite and within `MAX_COORD`. Checked in `Model::check`.
- **Vertical datum** (design call). Typed `points` and `contours` are internal elevations. A `cad` import's elevations are
  shared (map) elevations, turned into internal ones through the active location's elevation, as Revit's survey data is; the
  import's level does not move them. A 2,900 m survey on a project whose location has elevation 0 stands 2.9 km over Level 1:
  correct, but rarely wanted, so `acquire: true` first sets the active location's elevation so the toposolid's lowest point is
  at internal 0, and without it the reply carries a `warning` when the surface is more than 100 m from every level.
- Returns `id`, how many points and breaklines it kept, the plan area, and the lowest and highest elevation.

`modify` changes its name and type; `move`, `rotate`, `mirror`, `copy` and `delete` work on it as on any element.

**Inspecting.** `elements` gives its `area_m2` (plan), `surface_m2` (the triangulated surface), `volume_m3` (its solid),
`elevation_min` and `elevation_max` (shared elevations, through the active location), and point and triangle counts. A new
command `elevation_at {at: [x, y], toposolid?}` gives the surface's elevation at a plan point (internal and shared), or says
the point is outside it: what an agent needs to tell how far a cabin's corner stands over the ground.

**Drawing.**

- Plans: contours worked out from the triangulation at the type's `contour_interval` (default 1000 mm), every fifth a major
  contour, drawn heavier (object style subcategories Minor Contours and Major Contours), and the boundary. A toposolid shows in
  every plan in projection, whatever the cut plane, as Revit's does; where a plan's cut plane passes through it, the cut is
  drawn with the material's cut pattern.
- Sections and elevations: the solid cut, filled with the material's cut pattern, its top as a heavy line.
- 3D: the triangles, shaded in the material's colour.
- Schedules: a Topography schedule with name, type, area, surface area, volume and the elevation range.

**IFC.** Out: `IfcGeographicElement` with `PredefinedType TERRAIN`, one closed faceted Brep (as a shaped floor's solid is
written), in the site's spatial structure, with `GrossVolume` and its area; IfcOpenShell must validate it with
`express_rules=True`, and it goes in `ci/ifc-coverage.txt`. In: an `IfcGeographicElement` TERRAIN that BIMCraft wrote comes
back as a toposolid (its points read back from the Brep's top faces); one from another tool may stay as it comes in today, a
generic element, and the pull request says which it does.

### Slice 3: pads, and cut and fill (one pull request)

**Element.** `Element::Pad`, category `pad`: a flat platform cut into or built up on a toposolid.

```text
pad {boundary: [[x,y],…], elevation, toposolid?: id, cut_slope?: 1.0, fill_slope?: 1.5,
     edges?: [{cut_slope?, fill_slope?}, …], strip?: mm, name?}
```

- `edges` gives each edge of the boundary (numbered from its first point) slopes of its own, overriding the pad's: on a
  steep site the uphill edge is cut at 1:1 and the downhill one retained (slope 0).
- Where a fill batter cannot reach the ground because the ground falls away faster than the batter (an 80 % slope under a
  1.5:1 batter), or a cut batter climbs slower than the ground rises, the pad is refused naming the edge, the ground's slope
  there and the batter's, and saying what would reach: a steeper batter or 0, a retained face.
- `strip` (mm, default 0): topsoil stripped over the whole disturbed area before grading, reported as `strip_m3` (the
  disturbed area times the depth) apart from cut and fill.
- **Steps.** Pads at different elevations may share an edge when that edge is retained (slope 0) on both: terraces down a
  slope. Their disturbed areas may otherwise not overlap.

- `boundary` is a simple closed outline (no crossings); `elevation` is the platform's top, internal; `toposolid` the one it
  sits on (the only one, by default; refused when there are several and none is named, or when the boundary is not inside
  it).
- `cut_slope` and `fill_slope` are batters, horizontal over vertical (1.0 is 45°, 1.5 is about 34°): from the pad's edge the
  ground is graded down (fill) or up (cut) at that slope until it meets the existing surface. `0` means a vertical face (a
  retaining wall's job, not modelled as a wall). The defaults are a common starting point for firm soil, not advice; the
  params doc says so.
- The toposolid keeps the **existing** surface as it was; the pads define the **proposed** surface (existing, replaced by
  each pad's platform and batters). In this slice, pads' **disturbed areas** (platform and batters) may not overlap each
  other (refused, saying which pads and where), so there is one proposed surface and each volume belongs to one pad (design
  call: merging batters that meet, the higher or lower winning, is a later slice).
- Where a batter cannot reach the existing surface inside the toposolid's boundary, the pad is refused, saying where.
- **Corners** (design call). Along a straight edge the batter is a plane. At a convex corner it is a cone round the corner,
  as graded ground is, faceted into planar triangles at most 5° apart (so the surfaces stay piecewise planar and the volume
  exact for what is built); at a concave corner, the two planes meet in a mitre. The tests compare each volume with the exact
  volume of the faceted geometry (within 0.1 %), and with the analytic one (prism, wedges, and cones of
  `(π/3)·s²·h³` in all at the four corners of a rectangular pad) within the faceting's error.
- A pad holds its toposolid's GUID. Moving, turning or mirroring a toposolid takes its pads with it; a pad moved alone must
  stay inside its toposolid (design call).
- **Phases** (design call). In this slice pads always apply, whatever a view's phase; the existing surface is shown as a
  dashed line and dashed contours. A view of an earlier phase showing the ground without pads is a later slice.

**Queries once pads exist** (design call). `elevation_at` returns both `existing` and `proposed` elevations (the same where
no pad reaches); a toposolid's `elements` details and the Topography schedule give the existing surface's figures and,
beside them, `proposed_volume_m3` and the proposed elevation range. Nothing an agent reads changes meaning silently.

**Volumes.** For each pad: `cut_m3`, `fill_m3` and `net_m3` (fill minus cut), `strip_m3`, the area it disturbs in plan
(platform and batters) and the batters' plan outline. A command `cut_fill {toposolid?}` sums them over the site.

**Excavation for foundations.** A building on piles or pads moves earth only where its foundations go. `cut_fill` also
reports `excavation_m3`: the volume of footings, strip footings and structural columns below the proposed ground (the
existing one where no pad reaches), from their solids, each element's share listed by id. So one command answers for any
foundation: a platform, steps, piles, or a mix. The volumes are exact for
the triangulated surfaces: the integral of (proposed − existing) over the overlay of the two triangulations, split where the
difference changes sign, which is exact because both surfaces are piecewise planar. A sampling method may be used only if
tests show it within 0.1 % of the exact volume on every case below and the reply says it is an estimate; the exact overlay is
expected.

**Drawing.** Plans draw the existing contours dashed inside the disturbed area and the proposed contours solid, the pad's
outline and its batters' toe and top lines. Sections cut the proposed ground, with the existing surface as a dashed line
over or under it. 3D shows the proposed surface. A schedule of pads lists elevation, area, cut, fill and net.

**IFC.** Out: the proposed terrain as the toposolid's `IfcGeographicElement` TERRAIN (its shape after the pads); each pad as
an `IfcGeographicElement` with `PredefinedType USERDEFINED` and `ObjectType "Pad"`, its platform as its solid, and its
volumes in a property set `BIMCraft_Earthworks` (Cut, Fill, Net, in m³). `IfcEarthworksCut` and `IfcEarthworksFill` are
IFC4.3, not IFC4, so they are not used. Pads are export-only (design call): an IFC file read back gives the proposed ground
as a toposolid with no pads, and pads as generic elements, so no cut and fill is counted twice; the `.bimc` project keeps
everything. The area write-up says so.

## Tests that must pass (beyond the rules' sweep)

- DXF: an LWPOLYLINE with 38, a 3D POLYLINE, a LINE and a POINT with z, a mirrored OCS, a DWG through the bridge; an
  existing drawing's rendered PNG unchanged (pixels).
- Toposolid: a plane `z = a·x + b·y + c` from contours gives the plane's elevation at any point inside (`elevation_at`), its
  exact surface area and volume; a cone from circular contours stays a cone within the chord error; crossing contours are
  demoted to points and a position given twice takes the mean, each with a note; the hostile-params sweep; save, open and migrate; IFC through IfcOpenShell.
- Per-edge slopes and steps: a pad retained on its downhill edge on a uniform 80 % slope (accepted, its volume analytic);
  the same pad with the default fill slope (refused, naming the edge and both slopes); two pads stepped along a retained
  edge. `strip_m3` exact; `excavation_m3` of footings and columns against their boxes.
- Volumes, each against its analytic volume, within 0.1 %: a pad on a flat terrain below it (fill only: a prism plus batter
  wedges and corner pyramids), above it (cut only), a pad on a uniform slope cut by a level platform (both, with the
  daylight line where the analytic one is), vertical faces (slope 0), and a pad touching the toposolid's boundary (refused).
- Plans, sections and 3D looked at as [looking at the results](areas/looking-at-results.md) says.

## Real-world check

The first user's survey (a DXF of 185 LWPOLYLINEs, 7,817 vertices, elevations 2,881 to 2,995 m at 1 m, header `$INSUNITS` in
inches though its numbers are metres) is checked locally after each slice: imported with `units: m`, turned into a
toposolid, plans and sections looked at, a pad placed. It is never committed. Two things it shows that the design must
survive: a header unit that lies (so `cad` uses the import's units, not the file's), and photogrammetry noise (small closed
loops round trees and bushes, which become bumps in the surface: the best cure is contours exported from the survey's
terrain model rather than its surface model; `ignore_loops_under` handles what is left).

## Order of work

1. An issue on IPedrax/bimcraft with this plan's summary, before code.
2. Three stacked branches on the contributor's fork, each started from the one before: `site/1-dxf-elevations`,
   `site/2-toposolid` (the element, commands, drawing, schedules, IFC, `docs/areas/site.md`, the README's feature table and
   the roadmap's row) and `site/3-pads-cut-fill`. The fork cannot push branches upstream, so each pull request is based on
   `main`; the second and third are opened as drafts saying which pull request they depend on and from which commit to
   review. When one is merged, the rest of the stack is rebased onto `main` (`git rebase --update-refs`, or
   `git rebase --onto main <old tip>` after a squash merge), so each pull request shows only its own commits.

Each pull request: test first; `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` pass; translations updated with `cargo i18n update` for any new label.

## Left to the builder

The Rust representation of elevations in `dxf::Shape`; module layout (`site.rs` already holds property boundaries' bearings
and validation: the site's new code goes beside it, in `site/` modules or new files, and the module map in CLAUDE.md gets its
lines);
how the triangulation is cached; the contouring algorithm and how contours are joined into polylines; the batter geometry
at convex and concave corners of a pad (fans at convex corners, a mitre at concave ones, as long as the volume tests pass);
the earth material's colour and cut pattern (original); the starter toposolid type's thickness (a sensible default, such as
1000 mm); which object style subcategories already exist to reuse; and anything a reviewer of the pull request asks for.
