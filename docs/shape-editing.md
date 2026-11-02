# Shape editing for floors and flat roofs: design

This is the plan for IDEAS **MOD-16**: points and split lines on the top of a floor or a flat roof, so one element can fall
to drains, carry cross-falls, ridges and valleys, and keep a flat structure under a screed that tapers. It is what a
terrace, a roof garden, a podium's paving, a shower floor or a car park deck needs, and what BIMCraft can't model today:
each floor is level or one sloped plane, and nothing else. The rules in [CLAUDE.md](../CLAUDE.md) apply to all of it
(everything is a command, `Model::check` validates, the core has no UI, test first, clean room).

Status, on the branch `mod-16-shape-editing` (raised upstream as IPedrax/BIMCraft#183):

- **Phase 0**, the triangulation, is built (`crates/core/src/triangulation.rs`).
- **Phase 1**, shapes on floors, is built in the engine (`crates/core/src/shape_tests.rs`, and
  [levels, walls and roofs](areas/levels-walls-roofs.md) says how): the data and parts, the four commands, the checks,
  `floor_faces` and `shape_planes`, 3D, plans, IFC, `inspect`, attached walls, transforms, `edit_boundary`, snaps and
  file format 12. Three things planned for it are not done yet:
  - IFC writes a closed shell for each piece, as sloped floors' are, not one faceted Brep for the slab, and no
    `NetVolume`;
  - `inspect` gives no `volume_m3`;
  - sections take the shaped slabs from the 3D view's solids, but no test looks at one, and IfcOpenShell hasn't checked
    the IFC.
- **Phases 2 to 5** are a proposal.

Line numbers are as of `0733fd4`.

## What users expect

The workflow architects know from the established tools, in our own words:

- A floor (or a roof with no sloped edges) gets **points** on its top. Each point has a height relative to the top the
  element would have without them. The corners of the outline are points too, at 0 until raised or lowered.
- **Split lines** cross the top between two points. They are creases the surface must fold along, so the user decides
  where a ridge or a valley runs instead of leaving it to the triangulation.
- The surface between the points and lines is **flat triangles**. The tool picks the creases it needs and draws them
  apart from the user's own.
- A layer of the type can be **variable**. Then the bottom of the element stays flat and that layer takes up the
  difference: the screed tapers, the concrete under it doesn't. With no variable layer, the whole slab follows its top
  at its type's thickness.
- **Reset Shape** takes it all off again.

## What BIMCraft has today

| What | Where | Note |
|---|---|---|
| A floor's top is one plane: level, or through the slope arrow's tail rising `slope` degrees | `Model::floor_top`, `model.rs:3737` | `walls::Plane` is `z + g·p`, one gradient |
| A floor's slabs: its outline less its openings, lifted onto that plane, with a uniform `rise` | `Model::floor_slabs`, `model.rs:3755`; `floor_depth`, `model.rs:3749` | thickness is square to the slope, so plumb it's thicker |
| A roof's top is already **many planes**: `roof_faces` gives `(rings, Plane)` per slope | `model.rs:3575`, `roof_pieces` `3727`, `roof_slabs` `3722` | the shape we need, already used by the 3D view, walls and IFC |
| Layers: `function`, `material`, `thickness`; no variable flag | `Layer`, `model.rs:327`; `FloorType` `1886`, `RoofType` `1944` | |
| Triangulation: ear clipping of one simple ring, no holes, no inner points, no constraints | `geom::triangulate`, `geom.rs:153` | not enough for this |
| Faces of an arrangement of segments | `geom::faces`, `geom.rs:263` | useful to check split lines |
| A prism between two planes, cut by sections | `Solid::Prism { ring, bottom, top }`, `view3d.rs:159` | used by walls, ramps, runs |
| IFC: a prism between two planes as a shell | `Step::prism` and `Step::shell`, `ifc.rs:409`, `456` | used by ramps and walls |
| Walls attached to a floor follow one plane over its outline | `Bound::of`, `walls.rs:443` | the roof branch above it already follows many planes |
| IFC import of a slab whose top isn't one plane | `import.rs:1642`, `top_of` `273` | left out ("its top isn't flat"), or read as the plane of its largest upward face |
| Revit import of a sloped or stepped floor | `crates/rvt/src/floors.rs:206` | comes in level, with a note |
| File format | `FORMAT = 11`, `model.rs:90` | |

**The key point:** roofs already prove the pipeline can carry a top made of several planes. The work is to give floors
(and flat roofs) a top made of triangles, then let each consumer read "a list of planar pieces" where it reads "one plane"
today.

## The approach: a top made of planar pieces

One new query gives every consumer what it needs:

```rust
/// A floor's top as planar pieces in plan, each with its plane: one piece for a level or sloped floor,
/// the triangles of its shape (coplanar neighbours merged) for a shaped one.
pub fn floor_faces(&self, f: &Floor) -> Vec<(Vec<P2>, walls::Plane)>
```

`floor_top` stays for the unshaped case and as the reference the point heights are measured from. `floor_slabs` is
rebuilt on `floor_faces`. For a roof, `roof_faces` gains the same branch for a shaped flat roof, so `roof_pieces`,
`roof_slabs` and everything downstream work as they do for slopes.

### Data

```rust
/// Points and split lines on a floor's or a flat roof's top (IDEAS MOD-16).
pub struct Shape {
    pub points: Vec<ShapePoint>,
    pub lines: Vec<SplitLine>,
}

/// A point on the top, `z` mm above (negative: below) where the top would be without the shape.
pub struct ShapePoint { pub id: Id, pub guid: Guid, pub at: P2, pub z: f64 }

/// A crease the top folds along, between two of the shape's points or outline corners.
pub struct SplitLine { pub id: Id, pub guid: Guid, pub a: P2, pub b: P2 }
```

- `Floor` and `Roof` get `#[serde(default, skip_serializing_if = "Shape::is_empty")] pub shape: Shape`.
- `ShapePoint` and `SplitLine` are **parts** (`HasPart`, listed in `parts_now`, `sync_parts`, `forget_parts`), as sketch
  lines are, so a schedule, an agent or a later "edit this point" can name one by id.
- Heights are **relative** to the unshaped top, so changing the floor's `offset` moves the whole shape with it. Commands
  may take `absolute: true` and convert when they run.
- A corner of the outline (or of an opening) not listed among the points is at 0. A point may lie on an outline edge,
  where it adds a corner.
- Every end of a split line is one of the points or an outline corner. A command that adds a line with a loose end adds
  that end as a point, at the height the top has there at the time.
- `Layer` gets `#[serde(default, skip_serializing_if = "Not::not")] pub variable: bool`.
- **File format 12**, with a no-op step in `Model::migrate`. An older build that read a shaped floor would drop the shape
  and draw it flat. That's a misreading, not a field it can do without, so the bump is needed (CLAUDE.md, "A file is read
  kindly").

### Geometry

1. **Triangulate in plan:** a constrained Delaunay triangulation of the outline and its openings. The constraints are the
   outline edges, the opening edges and the split lines; the points are the inner vertices. Delaunay makes the result
   deterministic: the same points always give the same creases. Ties between cocircular points are broken by the
   corners' order, x and then y.
2. **Lift:** each vertex's z is the unshaped top there plus its point's `z`.
3. **Merge:** neighbouring triangles whose planes agree (within 0.01 mm at the shared vertices) merge into one polygon.
   A floor with points all at 0 is still one piece, and plans and IFC don't fill with needless diagonals.
4. **Thickness is plumb** for a shaped element: the type's thickness measured vertically, not square to each facet.
   Facets of different pitch then meet without gaps or overlaps underneath. At drainage falls of 1–2 % the difference is
   under 0.03 %. Sloped floors without a shape keep today's rule.
5. **With a variable layer**, the bottom is flat: the unshaped top less the type's thickness. Layers above the variable
   one follow the top, layers below it follow the bottom, and the variable layer is whatever is left at each point. Each
   facet becomes a `Solid::Prism` between its top plane and the flat bottom, not a `Solid::Slab` with a uniform rise.
6. **Caps:** `MAX_SHAPE_POINTS` (2,000) and `MAX_SPLIT_LINES` (2,000); every `z` within `MAX_COORD`. A triangulation
   past the cap is refused with the count, never truncated.

**The triangulation (built, phase 0).** The codebase writes its own geometry (the straight skeleton, the sketch solver,
CSG), its lints forbid panics, and the 3D view and plans run outside `Session::execute`'s `catch_unwind`, so a panic in
a dependency there would take the window down. So it is written here rather than taken from a crate such as `spade`:
`triangulation::constrained(outline, holes, points, lines)`, about 500 lines and 350 of tests.

- **Corners:** those within `TOL` of each other become one, the first in order of x and then y. An edge is cut at any
  corner within `TOL` of it, so a point on the outline bends it there.
- **Exact:** whether a corner lies left of a line, or inside a circle, is worked out in whole numbers on a 0.01 mm grid
  (`i128`), so a nearly flat triangle can't come out the wrong way round. That bounds a region to `MAX_ACROSS`, 10 km,
  well inside the 19 km where the in-circle sum would no longer fit.
- **Steps:** a sweep west to east triangulates the corners' convex hull; Lawson's flips make it Delaunay; Sloan's flips
  bring in each missing edge of the outline, the holes and the lines; Lawson's flips again, never across those edges; and
  a walk in from the hull, changing between outside and inside at each outline or hole edge, keeps the region's
  triangles.
- **Same answer in any order:** the output's corners and triangles are in a fixed order, so the same input in any
  order, either way round, gives an equal `Triangulation`.
- **Refused, saying why:** more than `MAX_CORNERS` (20,000) corners; a point not finite or past `MAX_COORD`; a region
  more than 10 km across; an outline or a hole with no area; a hole outside the outline or over another; a point or a
  line outside the outline or in a hole; a line shorter than a millimetre; edges that cross. Past 20 million flips it
  gives up rather than hold a command.
- **Speed:** 2,000 points with two lines in an L with a hole: about 17 ms, in the test build (the core is optimised
  there too).
- **Tests:** in the module. A random sweep of 400 cases, at least 100 of which must come out, checks that every triangle
  turns counter-clockwise, that they cover the region, and that no edge crosses a line. Leaving out the last Lawson pass
  or Sloan's flips each fails tests.

## Commands

The ids follow the established tool names, as the codebase does elsewhere.

| Command | Params | What it does |
|---|---|---|
| `edit_shape` | `{id, points?: [{at, z, id?}], lines?: [{a, b, id?}], absolute?}` | Replaces the shape all at once, as `edit_boundary` does a sketch: what is sent back with an `id` keeps it. Without `points` or `lines`, it replies with the shape as it is: the points, including the corners with their heights; the split lines; the generated creases; and the pieces with their planes. That reply is what agents need to reason about falls. |
| `add_point` | `{id, at, z?, absolute?}` | One point; `z` defaults to the top's height there now (0 relative). |
| `add_split_line` | `{id, a, b}` | One line; loose ends become points at the current height. |
| `reset_shape` | `{id}` | Removes the shape. |
| `type` (floor, roof) | layers take `variable: true` | At most one per type; not on walls or ceilings. |
| `modify` (floor) | unchanged | Rejects `slope` or `arrow` on a shaped floor, with the reason (see Validation). |
| `falls` (phase 5) | `{id, drains: [[x, y], …], fall: 1.5, ridges?: [[[x, y], [x, y]], …]}` | Better than the established tools: sets point heights so water runs to the drains at no less than `fall` %, adding split lines along the valleys. Agents and architects describe a terrace this way, not point by point. |

Every reply lists the pieces' count and the lowest and highest point of the top. `inspect` on a floor already reports
`top: [lo, hi]` (`commands.rs:2122`); it must take the shape's points into account, not only the outline's.

## Validation (`Model::check`)

New rules, each with an error that says what to fix:

- A point outside the outline, or inside an opening: "point 100231 at (12500, 3400) is outside floor 100190: move it inside, or delete it".
- A split line that crosses another, crosses an opening, or leaves the outline (reuse `sketch::solve`'s problem kinds and wording).
- A split line end that is neither a point nor an outline corner.
- Two points within 1 mm of each other.
- A shape on a sloped floor (with `arrow`), or on a roof that isn't by footprint with `slope` 0. In version 1 the two are
  exclusive: shape a level element, and give its slope through the points.
- More than one variable layer in a type.
- **A variable layer thinner than nothing** anywhere: "at point 100231 the screed would be −12 mm: raise the point by 12
  mm or thicken the variable layer".
- With no variable layer, nothing bounds the depth: the slab just follows the top.

## What changes, consumer by consumer

| Consumer | Today | Change |
|---|---|---|
| `Model::floor_slabs` (`model.rs:3755`) | one plane over the outline less openings | built on `floor_faces`; each piece less the openings |
| `Model::roof_faces` (`model.rs:3575`) | a flat roof is one plane | a shaped flat roof returns its pieces; `roof_rise` is plumb for them |
| 3D solids (`view3d.rs:511`, `612`) | `Solid::Slab` per piece | the same for constant thickness; `Solid::Prism` (top facet, flat bottom) with a variable layer |
| 3D layers (`view3d::layered`, `view3d.rs:741`) | a slab's layers in proportion; the `Prism` branch is walls only | a slab `Prism` branch: layers above the variable one from the top, below it from the bottom |
| Section layers (`render.rs:1230`) | each cut quad split in proportion to the layers' thicknesses | the same rule as 3D: fixed layers measured from the top or the bottom edge, the variable one between |
| Plan (`render.rs:1585`) | outline, openings, surface pattern, slope arrow | the pattern over each piece; split lines solid and generated creases dashed, thin, when the element is selected or being edited; each point's height as a label while editing |
| Attached walls (`walls.rs:454`) | one plane over the floor's convex pieces | each piece with its own plane, as the roof branch above it does |
| Shafts (`Model::less_shafts`, `model.rs:3773`) | per slab face | already per face: no change, but a test |
| Move, copy, rotate, mirror (`commands.rs:8236`) | outline, openings, arrow, pattern | the points' `at` and the lines' ends too; a mirror keeps the heights |
| `edit_boundary` | redraws the outline | keeps the points and lines inside the new outline, drops the rest, and names them in the reply |
| `inspect` (`commands.rs:2122`) | area and top from the outline | top range over every point; a volume (`volume_m3`) for every floor |
| Reinforce (`reinforce.rs:409`) | bars from `floor_top` | with a variable layer above the structure, the structure is flat: bars from the flat bottom. Otherwise refuse in version 1, saying why |
| IFC export (`ifc.rs:658`) | level: an extruded profile; sloped: per-piece Breps; quantities only `Width` when sloped | a shaped slab is **one closed `IfcFacetedBrep`**: top facets, a flat or following bottom, and plumb sides, all planar. Write `NetVolume` and `GrossVolume` for sloped and shaped slabs too, from the prisms. |
| Snaps (`snap.rs:320`, `603`) | outline edges and corners | the shape's points and split lines too |
| IFC import (`import.rs:1642`) | a top that isn't one plane is left out or forced flat | phase 5: read the upward faces' vertices as points relative to the highest plane, and the creases as split lines |
| Revit import (`crates/rvt/src/floors.rs:206`) | sloped or stepped comes in level | out of scope: reading the slab's shape from `.rvt` is research under the provenance fence |
| `lenient.rs`, `Element::refs_mut`, `parts_now` | | the new fields and parts, as the rules require |

## Desktop (`crates/ui-egui`)

- With a floor, or a flat roof, selected: a **Shape Editing** panel in the contextual tab, next to Slope Arrow
  (`lib.rs:3783`), with Modify Sub Elements, Add Point, Add Split Line and Reset Shape.
- **Add Point:** click on the top. The Options Bar takes the height and an Absolute check box.
- **Add Split Line:** two clicks, snapping to points, corners and edges.
- **Modify Sub Elements:** click a point or an edge (an edge sets both its ends) and type its height. Points drag in plan
  with the usual grips; dragging heights in the 3D view (`orbit.rs`) comes later.
- Every action runs a command. The canvas holds no geometry of its own (CLAUDE.md, "The UI is thin").
- egui_kittest tests: place a point at a model point, type −30, and check the reply's top range and the plan's pixels.

## Out of scope

- A different slope on each roof edge (upstream issue #74), and slope arrows on roofs and ceilings: separate items.
- Shaped ceilings; curved (smooth) tops; "pick supports" from beams.
- Toposolids. This work is their engine, though: a toposolid is a floor-like element whose top is a shape, so site and
  paving reuse `Shape`, the triangulation and every consumer change above.
- Reading shaped floors from `.rvt`.

## Phases, sizes and the tests that open each

Sizes as the roadmap gives them, for one developer working with agents.

| Phase | Size | First failing tests |
|---|---|---|
| **0. Triangulation** in `triangulation.rs` (built) | M | A square with a centre point gives 4 triangles; a split line is always an edge; an opening stays empty; the same input in any order gives the same triangles; duplicates, collinear runs, points on edges and the cap never panic (a property sweep). |
| **1. Shape on floors** (built, but for what the status above lists): data, commands, `check`, `floor_faces`, 3D, plan, sections (no variable layer), attached walls, transforms, `edit_boundary`, `inspect`, IFC export, snaps, format 12 | L | A 10 × 10 m floor with a drain at −30 mm gives 4 faces and the volume to 0.1 %; a section through the drain shows a V top; a wall attached under it follows the creases; IfcOpenShell validates the closed Brep with `express_rules=True`; mirror and move keep the shape; a format-11 file opens and a format-12 file is refused by 11; `hostile_params_never_panic_or_break_the_model` covers the new commands as it does every command. |
| **2. Variable layer** | M | A tile, screed (variable), membrane and concrete type: the section shows the concrete flat and the screed tapering; the 3D view's layers match; `check` refuses a point that makes the screed negative; reinforcement follows the flat structure. |
| **3. Desktop** | M | Add Point at a clicked model point with −30 typed; Add Split Line snapping to two corners; Reset Shape; the three looks screenshotted as `docs/areas/looking-at-results.md` asks. |
| **4. Flat roofs** | S | A flat roof by footprint with four drains; walls attached to it; a sloped roof refuses a shape. |
| **5. Extras**: `falls`, IFC import of multi-plane slabs, a spot slope annotation | M | `falls` with two drains gives at least 1.5 % everywhere and every point running to a drain; a shaped slab exported and imported comes back with the same points. |

**About two months in all.** After phases 0–2 (about a month) agents, the CLI and MCP can model falls to drains and
tapering screeds, and IFC carries them. The desktop tools follow.

## Open questions

1. Triangulation: written here (done in phase 0, in its own module rather than `geom`), unless the maintainer would
   rather have `spade`; #183 asks.
2. Shaped thickness plumb (proposed), or square to each facet as sloped floors are today?
3. Shape and slope arrow exclusive in version 1 (proposed), or a shape on top of a sloped plane?
4. Creases: plans draw every plane's edges always, as thin as the outline (built that way, as the simplest start).
   Generated creases dashed apart from the user's split lines, or shown only while selected, could come with the
   desktop tools.
5. Upstream first: IDEAS asks for an issue naming **MOD-16** before the work starts, so the fork doesn't drift from
   `IPedrax/BIMCraft`.
