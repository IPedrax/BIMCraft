# Spatial core and section rendering: performance review and agent handoff

Reviewed: 2026-11-01. Source revision: `c19adddd342fbc1708280f31fc1746d540876a4c` (local checkout matched PR #188 at review time).

## Scope and status

The user requested a review and suggestions for a very fast, scalable, reliable BIM core. This document records that review for another agent; it does not authorize implementation by itself.

Sources inspected:

- [Section views plan](section-views-plan.md), including its checklist.
- [Spatial core plan](spatial-core-plan.md), including its status list.
- [PR #188 description](https://github.com/IPedrax/BIMCraft/pull/188).
- [Performance measurements](performance.md).
- Relevant geometry, rendering, cache, storage, sheet, and UI implementation referenced below.

This was code and architecture inspection. Benchmarks and tests were **not rerun**, and proposed speedups are not measured results. Audit targets below are not confirmed runtime defects unless explicitly described as observed behavior. Recheck the current source before implementing because this is a WIP branch.

The user has since authorized commits on this branch (2026-11-02). Do not push or open pull requests unless the user asks. Follow repository instructions for any subsequent implementation.

## Main conclusion

The architectural direction is sound. The highest-value next upgrade is to make geometry, visibility, and drawing incremental, rather than merely parallelizing the remaining whole-model work. Command execution is already fast; view regeneration and memory amplification are the next constraints.

Preserve the full-computation oracle. Distinguish exact-preserving optimizations from optional preview approximations, especially for LOD, GPU output, hidden-line export, and transparency.

## 1. Prioritize persistent geometry fragments

**Observed:** `SectionWork::new` still prepares geometry and bounds across the model. A BVH built after that preparation accelerates visibility but leaves the expensive model-wide preparation intact.

References: [SectionWork](../crates/core/src/render.rs), [solids_and_extremes](../crates/core/src/view3d.rs).

Introduce separate cached products:

- Conservative bounds and spatial metadata.
- Local solids, topology, and edges.
- Tessellated geometry.
- View-specific projected fragments.
- Appearance and selection state.

An instance move should update its transform, bounds, and affected spatial nodes. A material edit should update appearance. An annotation edit should leave 3D geometry untouched.

Bounds should be available without tessellating every object. Use analytic bounds for common primitives and conservatively transformed family-local bounds. Preserve tighter hull queries where exact framing requires them.

**Acceptance criterion:** changing one isolated object in a million-element model does not enumerate or regenerate unrelated objects. Dependency-driven changes may legitimately affect many objects; measure the affected set explicitly.

## 2. Make dependency tracking the correctness foundation

Cache keys must account for everything the result reads: host geometry, neighbouring walls, levels, family/type definitions, parameters, linked-model transforms, and relevant project settings.

**Audit target:** `SectionWork::is_of` checks only `m.elements.state()`, although prepared geometry depends on library definitions too. The desktop currently rebuilds on model-pointer changes, which provides protection there. The reusable core API deserves a library-only edit regression test before broadening cache reuse.

References: [SectionWork::is_of and section_picture_and_over_with](../crates/core/src/render.rs), [element state](../crates/core/src/ids.rs), [derived lookup](../crates/core/src/derived.rs).

Record empty spatial queries as dependencies. A room that previously found no wall in a tile must invalidate when a wall is inserted there. Moves must invalidate dependencies around both old and new locations.

Use separate revisions for geometry, placement, appearance, annotation, and quantities where worthwhile. Start conservatively, then narrow invalidation with tests. Cache identity must remain correct across undo, redo, branching edits, and independent model snapshots.

## 3. Remove hover and selection from section geometry regeneration

**Observed:** tints are part of the section picture key. A hover change calls `Cut::draw`, regenerating the picture and drawing. Caching unchanged frames does not remove that interaction cost.

Reference: [Shot, Cut::draw, and section_view](../crates/ui-egui/src/section.rs).

Retain base colour, depth, element ownership, and required face information. Apply hover and selection through compositing, as the existing GPU path already does conceptually. Separate region geometry from tint-dependent paint.

Theme changes should normally update paint rather than geometry.

**Acceptance criterion:** moving the pointer across elements triggers zero solid construction, triangulation, or visibility rebuilds. Verify tint-dependent regions and transparent surfaces still match their intended semantics.

## 4. Move worker rendering earlier, including the drawing overlay

The plan leaves the overlay on the UI thread, but its own 100-storey measurements put that work around two seconds. Even the proposed 150–300 ms overlay targets are too long for an interactive frame.

Compute geometry, visibility, and display lists on workers. Keep input handling, lightweight overlays, and result presentation on the UI thread.

Use:

- Immutable snapshot inputs.
- One latest pending request per view.
- Cooperative cancellation between meaningful chunks.
- Generation checks before publishing results.
- A bounded shared worker pool.
- Previous-image reprojection during pan/zoom, followed by a final render.

Keep the displayed image and picking data from the same generation. A click must not silently pick through newer geometry than the user sees. Bound retained snapshots and cancelled-job resources.

## 5. Share a two-level spatial hierarchy

**Observed:** a triangle BVH already exists for GPU picking. Extend the architecture rather than introducing unrelated acceleration systems for every feature.

Reference: [Mesh and Bvh](../crates/core/src/view3d/gpu.rs).

Recommended structure:

- An upper hierarchy over instances and element bounds.
- A lower hierarchy over each reusable geometry prototype.
- Shared traversal utilities for volume queries, picking, visibility, and shadow rays.

Refit changed bounds, insert/remove locally, and rebuild degraded subtrees in the background. Measure node visits and overlap: repeated refitting can preserve correctness while steadily worsening performance.

Benchmark the existing median-split approach against binned surface-area-heuristic construction. SAH is a useful starting point, not proof of optimal performance for every BIM query. See [PBRT's BVH discussion](https://www.pbr-book.org/4ed/Primitives_and_Intersection_Acceleration/Bounding_Volume_Hierarchies).

Keep existing XY tiles for local wall and room queries where they work well. One structure need not serve every access pattern.

## 6. Upgrade occlusion conservatively

**Observed:** front-to-back batched occluders already exist in `Scene::section`; the checklist lags the implementation.

Reference: [Scene::section and Tiles](../crates/core/src/view3d.rs).

Next candidates:

- Traverse BVH nodes instead of projecting every element.
- Add multiple depth hierarchy levels above the current 8-pixel tiles.
- Refresh only modified depth tiles and ancestors where profiling justifies the bookkeeping.
- Prioritize occluders by projected coverage and estimated cost as well as depth.
- Reuse tessellation between the pre-pass and final scene.
- Add a depth-only raster path that avoids unnecessary hit/lighting work.

Benchmark masked software occlusion for partial tile coverage. Treat [Intel's implementation and paper](https://github.com/GameTechDev/MaskedOcclusionCulling) as an algorithm reference compatible with the project's Rust constraints, not an automatic dependency choice.

**Audit target:** the current pre-pass selects occluders by element category. Any transparency-enabled path must use effective view/material opacity. Do not assume a wall, door, or window is opaque merely from its category.

Occluder approximations must never fill openings. Uncertain visibility must retain the object. Preserve clipping rules, depth ties, numerical margins, and stable final ordering. Existing pixel/owner/face comparisons remain important.

## 7. Eliminate repeated family work and allocation

**Observed:** `solids_and_extremes` already runs in parallel, but creates a new `Components` cache for each 256-element chunk. Repeated families can therefore be regenerated across chunks.

References: [solids_and_extremes](../crates/core/src/view3d.rs), [Components and its keys](../crates/core/src/component_geometry.rs).

Build or lazily share immutable prototypes by unique geometry key. Concurrent requests for the same prototype should share one computation.

Additional candidates:

- Replace repeated string cloning and JSON parameter serialization with interned identifiers and canonical parameter keys.
- Retain local geometry plus transforms instead of expanding every instance.
- Replace per-element small vectors with packed buffers and offsets where useful.
- Reuse scratch buffers for projection, raster bins, and candidate lists.
- Reduce simultaneous intermediate collections and duplicate bounds maps.

Measure allocations, bytes copied, and memory bandwidth alongside CPU time. More threads can amplify memory pressure rather than improve latency.

## 8. Extend retained GPU rendering to instances and changed chunks

**Observed:** a GPU renderer already exists and has strong documented warm-frame results. The structural opportunity is preparing and uploading the scene after changes.

References: [GPU Mesh](../crates/core/src/view3d/gpu.rs), [desktop GPU renderer](../crates/ui-egui/src/gpu.rs), [orbit scene lifecycle](../crates/ui-egui/src/orbit.rs), [measurements](performance.md).

The mesh currently holds expanded triangle vertices. Add prototype geometry buffers, instance transforms, and independently replaceable geometry chunks.

Evaluate indexed meshes where complete vertex attributes match. Preserve face normals, seams, ownership, and plane semantics; position-only deduplication is incorrect.

Add GPU culling and indirect submission after retained geometry works. Keep visibility decisions on the GPU where possible to avoid readback stalls.

Optional screen-space tessellation/LOD can help pipes, fittings, and repeated equipment during interaction. Make it an explicit quality mode: it cannot also promise exact equivalence to the existing tessellation. Publication output should use its own defined accuracy criteria.

## 9. Retain and spatially index 2D drawing fragments

The plan lookup is already close to storey-local cost. The next gain is avoiding regeneration and processing of all drawing primitives whenever a view changes.

References: [drawing generation](../crates/core/src/render.rs), [region compositing](../crates/core/src/render/regions.rs), [canvas](../crates/ui-egui/src/canvas.rs).

Cache model-space geometry separately from paper-space text, symbols, line weights, and scale-sensitive detail. Spatially index drawing fragments for painting, picking, snapping, and region overlap.

Pan should primarily change transforms and visible candidates. Zoom should regenerate only what genuinely depends on scale.

Dirty-region redraws must include old and new bounds and overlapping compositing dependencies. Otherwise moving an occluder can leave stale hidden geometry behind.

For schedules, virtualize visible rows and incrementally maintain affected quantities/groups. Export must produce all requested rows, but the desktop need not lay out millions of primitives to show a small viewport.

## 10. Revise vector hidden-line design before implementation

The proposed depth-buffer visibility followed by exact endpoints from rays is useful as an approximation strategy, but does not by itself guarantee exact vector output.

A thin visible interval can fall between samples. A few rays cannot prove that an edge contains no additional visibility transitions.

For reliable vector export:

- Preserve analytic/topological edges, including section intersections and silhouettes.
- Use spatial queries to identify potential occluding faces.
- Split candidate edges at geometric visibility events.
- Classify the resulting intervals.
- Use robust predicates and defined construction tolerances.

Raster depth can accelerate candidate selection, but screen-resolution survivors alone are insufficient for resolution-independent export. Open CASCADE's distinction between shape-based and polygonal algorithms is a useful reference: [hidden-line removal documentation](https://www.occt3d.com/dev/doc/overview/html/occt_user_guides__modeling_algos.html).

**Additional correctness constraint:** preserve cross-element coplanarity semantics when parallelizing plane processing. The current global `planes` function merges coplanar triangles across elements. Purely local plane IDs could introduce seams.

Reference: [planes](../crates/core/src/view3d.rs).

## 11. Clarify sharing for sheets and shadows

**Sheets:** share geometry, dependencies, and acceleration structures across viewports. Visibility remains viewport-specific. One elevation's culled scene cannot generally serve another elevation. Sharing the union is valid but may lose much of the culling benefit.

Reference: [sheet rendering and shared SectionWork](../crates/core/src/sheet.rs).

**Shadows:** share acceleration structures, but query shadow casters independently. An object invisible to the camera may cast a visible shadow. Camera-survivor geometry alone is insufficient.

Make these distinctions explicit before implementing section-plan pieces 8–10.

## 12. Design memory budgets and eviction with the caches

Caching everything indefinitely would replace compute stalls with memory exhaustion.

Define budgets for prototypes, tessellations, drawing fragments, images, GPU buffers, and retained snapshots. Evict expensive derived data independently of authoritative model history. Bound concurrent exports and background builds.

Instrument retained bytes by cache and snapshot generation. A small entry holding an `Arc<Model>` can retain much more memory than its own allocation suggests.

For chunked files, store bounds and dependency metadata that support spatial discovery before loading full geometry. Distinguish not-loaded from absent. Define how complete validation and export force required data to load.

## 13. Broaden performance and correctness evidence

The tower is valuable, but its repetition favours family caching and predictable spatial layouts.

Add fixtures for irregular buildings, dense MEP, large sites, linked models, long spanning elements, unique families, transparent facades, and large coordinates. Exercise type edits, level moves, bulk deletes, undo branches, and long editing sessions.

Track:

- Cold and warm latency, including p95/p99.
- Time from input to first response and to final-quality output.
- Elements visited, fragments rebuilt, cache misses, and BVH nodes visited.
- Triangles generated, uploaded bytes, allocations, and peak retained memory.

Keep twin tests, but add independent geometry invariants and adversarial cases. Twins can preserve a shared bug. Filtered robust predicates are particularly useful near collinearity and tangency; accurate predicates alone do not guarantee accurate intersection constructions. See [CGAL's explanation](https://www.cgal.org/FAQ.html).

Add structural regression assertions, such as a remote annotation edit rebuilding zero geometry fragments. These are more portable than timing thresholds alone.

Use targeted cases for holes, thin visible slivers, near-coplanarity, clipping boundaries, transparency, mirrored instances, library-only edits, snapshot reuse, and stale worker completion.

## Recommended implementation order

| Order | Deliverable | Main benefit |
|---|---|---|
| 1 | Separate tint/paint updates; move full view work to cancellable workers | Responsive interaction |
| 2 | Dependency-complete fragment and bounds caches, with memory budgets | Remove whole-model rebuilds |
| 3 | Incremental instance BVH and shared prototype geometry | Scale visibility and picking |
| 4 | Retained 2D fragments and partial GPU uploads | Fast redraw after edits |
| 5 | Hierarchical occlusion and measured allocation/raster optimizations | Reduce remaining computation |
| 6 | Geometric vector visibility and independent shadow queries | Reliable publication output |
| 7 | Chunked loading and broader query columns | Larger projects and faster startup |

These are priorities, not a requirement to implement everything at once. Each change should have a bounded scope, explicit correctness contract, and before/after evidence. Some preparatory dependency and cache work can support multiple deliverables.

## Reconcile the existing plans

- Section checklist piece 5 appears both checked and unchecked.
- Front-to-back occluder processing already exists, although the section checklist describes it as remaining work.
- Sheet sharing is partly implemented: distinguish shared model work from per-viewport visibility.
- PR and plan measurements differ without a consistent revision label. Record benchmark commit, hardware, fixture, resolution, quality mode, and warm/cold conditions together.
- Do not mistake the existing GPU renderer, parallel solid preparation, or picking BVH for missing features. The opportunity is to integrate them with incremental, shared geometry.

## Implementation tracking

The user authorized implementation after this review and asked for continuous work through the recommended order.
Keep this checklist current; mark a whole priority complete only when all of its acceptance criteria are covered.

- [x] **1. Responsive interaction (section/elevation milestone)**
  - [x] Separate section hover, selection and theme paint from geometry and visibility rebuilds.
  - [x] Retain section model cuts/datums; repaint region ink from the tinted picture.
  - [x] Verify zero picture/model-cut rebuilds on interaction and pixel/owner/face/coverage equality with fresh output.
  - [x] Run formatting, workspace Clippy and the workspace tests for the tint change (1,105 core and 615 UI tests passed).
  - [x] Bounded shared view workers with latest-request replacement and cooperative cancellation.
  - [x] Move section fitting, pictures and full drawing overlays off the UI thread, including cached level-grip endpoints.
  - [x] Publish displayed geometry and picking as one generation; reject stale completions and reproject during navigation.
  - [x] Complete the workspace regression checkpoint for worker integration (1,106 core, 621 UI, 181 Revit-reader and 118 updater tests passed; existing ignored tests unchanged).
- [ ] **2. Dependency-complete fragments and bounds, with memory budgets.**
  - [x] Confirm the direct library-only `SectionWork` failure with an output regression.
  - [x] Add shared, mutation-sensitive type/material/family definitions and a geometry-complete work guard.
  - [x] Verify direct type/family edits, snapshot isolation, undo/redo branches, unchanged JSON, and annotation definition sharing.
  - [x] Complete the full workspace checkpoint for this guard (1,107 core tests, five cache integration tests, 621 UI tests; no failures).
  - [x] Add metadata-only per-element revisions, lazy mutable-iterator stamps, and branch-safe revision deltas (six integration regressions passed).
  - [x] Remove deletion's unrelated mutable accesses; compare indexed deletion with its original full-scan oracle (nine core and 12 UI targeted tests passed).
  - [x] Complete the full workspace checkpoint for runtime revisions, indexed deletion, and the primitive bounds cache (1,110 core, six revision, six bounds, five cache integration, 621 UI tests, and remaining suites passed).
  - [x] Cache column/footing and straight-run bounds separately from tessellation, with narrow dependencies and byte-budgeted eviction (six integration tests and two independent solid-corner/nonfinite unit tests passed).
  - [x] Add weak definition-identity tokens that detect unique-owner mutation without retaining definition data (all three `shared.rs` source tests passed directly).
  - [x] Verify weak-identity `SectionWork` guards against all 17 existing cache/bounds/revision integration regressions and workspace Clippy.
  - [x] Verify cleanup of empty incremental lookup buckets exposed by removing deletion's global mutable sweeps (focused and full workspace regressions passed).
  - [x] Retain isolated column/run geometry fragments under a byte budget, sharing stored geometry while refreshing appearance (four initial output/parallel-order integration and two material/lifetime unit tests passed; workspace Clippy clean).
  - [x] Complete the fragment checkpoint including all-visit/uncached-preparation counters and uncached storage preservation (1,114 core and 621 UI tests passed; stable-tree doctests rerun passed).
  - [x] Add conservative swept-profile/imported-mesh family envelopes and mirrored/rotated component bounds (three core unit and seven bounds integration tests passed).
  - [x] Verify root-capacity byte accounting and the family-bounds checkpoint (1,117 core, seven bounds, four fragment integration, 621 UI tests, remaining workspace suites and doctests passed; Clippy clean).
  - [x] Close CAD/cloud library-only extent changes in the work guard (both regressions reproduced before the fix; all seven work-cache tests and workspace Clippy passed).
  - [x] Use conservative component envelopes during worker preparation; defer exact extrema without changing framing or datum endpoints.
    - [x] Add opt-in `SectionWork::new_deferred` with snapshot-owned lazy extrema, full fallback for unsupported families, and structural counters (five integration tests passed, including tower sections/elevations; Clippy clean).
    - [x] Propagate cancellation through lazy fitting, datum and extent queries (`section_fit_cancellable`; sweeps of every checkpoint in `tests/component_preparation.rs`, which fail if a cancelled range is returned as if whole).
    - [x] Adopt `new_deferred_cancellable` in the section worker (`section_worker::tests`, including a real worker replacing a job).
    - [x] Measure it: a memory gain and no latency gain; see below. Cells moved from a hash to `placed`'s order to remove a regression.
  - [x] Complete the final deferred-preparation workspace checkpoint (1,118 core, five deferred-preparation, seven bounds, seven work-cache, four fragment, six revision integration, 621 UI tests and remaining suites/doctests passed; formatting and Clippy clean).
  - [x] Build the model's lookup before any section preparation (a bare model was quadratic: 116 s, now about 2 s).
  - [x] Leave round runs' tubes unmade until a view needs them (`deferred_runs`; `tests/component_preparation.rs`).
  - [x] Remove what grew with height from preparation: shafts per slab, walls per tile, family envelopes per chunk, the box hash.
  - [x] Run the picture's per-element loops on the render threads, in order; drop its wall index and its scan for point clouds.
  - [ ] Extend dependency-complete bounds/fragments to hosts, neighbouring geometry, families and empty spatial reads.
- [ ] **3. Incremental instance BVH and shared prototypes.**
- [ ] **4. Retained 2D fragments and partial GPU uploads.**
- [ ] **5. Hierarchical occlusion and measured raster/allocation improvements.**
- [ ] **6. Geometric vector visibility and independent shadows.**
- [ ] **7. Chunked loading and broader query columns.**

The temporary test linker wrapper in ignored `dist/section-tint-review/` disables memory-mapped linker output and
omits debug information from newly built validation crates and strips linked debug information to avoid local Btrfs
reservation stalls and reduce code-generation cost. Debug assertions remain enabled. Repository build settings
are unchanged. Existing Revit-number and benchmark tests remain ignored under their original conditions.

## Deferred preparation measured (2026-11-02)

Working tree on `c19addd`, release, 16 logical CPUs, annotated tower with its drawing set, North Elevation at 1600 x 1000 px,
cold first frame without tints. Each mode ran in its own process, three runs each, with the lookup built as a `Session`'s model
has it; median shown. Picture hashes were equal in every run. Single-digit-percent differences in the total are within noise.

| | 10 storeys, full | 10 storeys, deferred | 100 storeys, full | 100 storeys, deferred |
|---|---:|---:|---:|---:|
| Prepare the work | 143 ms | 123 ms | 2,079 ms | 1,884 ms |
| Fit | 7 ms | 14 ms | 74 ms | 161 ms |
| Overlay | 18 ms | 17 ms | 235 ms | 242 ms |
| Picture | 347 ms | 359 ms | 1,482 ms | 1,653 ms |
| Total | 512 ms | 514 ms | 3,868 ms | 3,993 ms |
| Retained by the work | 0.15 GB | 0.12 GB | 2.11 GB | 1.83 GB |
| Peak | 0.59 GB | 0.57 GB | 4.00 GB | 3.73 GB |

915 of 530,000 components resolved at 100 storeys. Deferring does not shorten a first frame: components' extrema were
already cheap, from each family's cached hull. Preparation's cost lies in the other elements' solids, and the picture is the
largest step, so that is where priority 2 and later should look for time.

## Lazy round runs measured (2026-11-02)

The same conditions as above at 100 storeys, median of three, with deferred now also leaving round runs unmade. Every
picture (fitted and zoomed) and drawing hashed the same as full preparation's.

| | North Elevation, full | North Elevation, deferred | Section A, full | Section A, deferred |
|---|---:|---:|---:|---:|
| Prepare the work | 2,079 ms | 1,493 ms | 2,101 ms | 1,512 ms |
| First frame (prepare, fit, overlay, picture) | 3,862 ms | 3,391 ms | 4,248 ms | 3,717 ms |
| Overlay at a new zoom | 249 ms | 299 ms | 338 ms | 380 ms |
| Retained by the work | 2.12 GB | 0.57 GB | 2.12 GB | 0.57 GB |
| Peak | 4.06 GB | 2.52 GB | 4.20 GB | 2.66 GB |

## Where preparation's time goes (2026-11-02)

Measured at 100 storeys before lazy runs, with per-element timers, whose lock inflates small elements. CPU by category: walls
29% (127 µs each), ceilings 19% (110 µs), components 18% (3.3 µs, 530,000 of them), pipes 17% and conduits 16% (22 µs). Wall
clock of the 1.7 s: parallel chunks 1.2 to 1.3 s, `box_of` 220 ms (a hash of 900,000), joining 100 to 130 ms, elements and the
wall index about 80 ms, ranges 27 ms.

`SectionCache` isn't worth adopting as it stands. At its 128 MiB default it thrashes: 8% hits, because a pass in element order
evicts what the next pass wants first. Unbounded it holds 1.56 GB and, at 100% hits, a pass takes 1.8 s against 2.0 s, still
slower than plain deferred preparation. A fragment cache has to hold walls and ceilings to pay, and the cost of what it stores
needs to be measured against rebuilding it.

## Quadratics in height removed (2026-11-02)

Profiling per category at both 10 and 100 storeys showed what cost more per element as the tower grew. A ceiling went from 110 to
3 µs, as `Model::less_shafts` asked every shaft of the project for each slab; it now reads a table kept per model state, with a
plan-box test first, which every caller shares (plans, quantities, IFC). A wall went from 123 to 21 µs at 100 storeys, the
same as at 10: a plan tile held its square's walls from every storey, and each wall end sorted them all; tiles are now searched by
height. Family envelopes are found once a pass, not once a chunk, and `box_of`'s hash is a binary search of `placed`.

The same conditions as above, 100 storeys, median of three; every picture and drawing hashed the same as before these changes:

| | North Elevation, full | North Elevation, deferred | Section A, full | Section A, deferred |
|---|---:|---:|---:|---:|
| Prepare the work | 1,236 ms | 710 ms | 1,298 ms | 726 ms |
| Fit | 79 ms | 148 ms | 75 ms | 148 ms |
| Overlay | 212 ms | 255 ms | 310 ms | 383 ms |
| Picture | 1,497 ms | 1,473 ms | 1,681 ms | 1,668 ms |
| First frame | 3,035 ms | 2,587 ms | 3,355 ms | 2,924 ms |
| Overlay at a new zoom | 222 ms | 276 ms | 288 ms | 361 ms |
| Retained by the work | 2.01 GB | 0.45 GB | 2.01 GB | 0.45 GB |
| Peak | 3.94 GB | 2.40 GB | 4.08 GB | 2.54 GB |

At 10 storeys a first frame is 463 ms deferred and 497 ms full, its peak 0.47 GB and 0.62 GB. Comparing cost per element across
sizes is what found these: the tower's repetition hid them in totals.

## The picture measured (2026-11-02)

The picture's per-element loops now run on the render threads in order, and it no longer builds a wall index or scans the model
for point clouds. That took it from 1,473 to 1,259 ms for the elevation and from 1,668 to 1,432 ms for the section at 100 storeys,
hashes unchanged: less than the steps' serial times suggested, because the rest is making triangles. Where it goes now:

| 100 storeys | North Elevation | Section A |
|---|---:|---:|
| Boxes, screens, what hides | 123 ms | 115 ms |
| Occluders: making triangles | 303 ms (1.64 M) | 75 ms (0.50 M) |
| Occluders: raster | 213 ms | 54 ms |
| Occluders: depth tiles and filter | 73 ms | 45 ms |
| Survivors' triangles | 231 ms (1.08 M) | 547 ms (2.16 M) |
| Face names | 31 ms | 60 ms |
| Planes (serial) | 116 ms | 181 ms |
| Final raster | 113 ms | 233 ms |

Fitted at 1600 x 1000 px, a storey is 8.5 px tall, about one 8 px tile, so tiles keeping their farthest depth prove little hidden:
over a million triangles survive for an elevation that shows far fewer.

## Next task for the receiving agent

Two directions, by measured cost. First, making triangles scales poorly on threads, each solid cloned and each layer allocated:
make them without the clone (`PreparedSolids::painted`, `layered`), and reuse an occluder's triangles in the final scene rather
than making them twice. Second, depth tiles too coarse for a fitted tower: benchmark finer or masked coverage (section 6 above),
keeping culling conservative. Overlay and fit scan every element's box, which a spatial structure (priority 3) would avoid. Then finish hosted/wall/spatial read dependencies and narrow
family keys. Joined walls need old/new tile
invalidation, empty-query dependencies, type users and long-wall handling. Preparation/assembly still enumerates
the model, so priority 2 remains incomplete. Keep the subsequent BVH/prototype, retained drawing/GPU and later
items in the recommended order above. Commit on this branch as work completes; do not push without asking.
