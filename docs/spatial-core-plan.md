# The next core: cached, spatial, incremental

A plan, done in pieces on `feat/spatial-core`, for a core that is **functionally equal to today's and structurally faster**:
every command, file, drawing, schedule and export gives the same result, and the work an edit costs follows what it
touches, not the size of the project. Background: [discussion #118](https://github.com/IPedrax/BIMCraft/discussions/118)
and [performance.md](performance.md).

## The idea in one paragraph

Today a command changes the model, then the engine checks the whole model, and every view, room and schedule is worked
out again from every element. The next core keeps the model as the only truth, but treats everything worked out from it
(wall joins, room boundaries, geometry, plan fragments, quantities, checks) as **cached results that record what they
read**. Reading is recorded per element and per tile of space, so an edit invalidates only the results that read what
it changed, including the neighbours a move brings near or takes away. Results are pure, so they are computed on every
core at once; history is a log of changes over structurally shared storage, so snapshots, undo and readers on other
threads cost almost nothing.

## What stays

- `Session::execute` and the command catalogue: the CLI, scripts, MCP and the desktop see no difference.
- Ids, GUIDs, the library, the file format and `Model::check`'s rules and messages.
- The rules of [CLAUDE.md](../CLAUDE.md): never crash, hostile input, a thin UI, test first.
- **The full computation stays as the oracle.** Every incremental result has a from-scratch twin; tests and a fuzzer
  run random command sequences and compare them. Any difference is a bug. That is what makes caching safe.

## What changes

| Today | Next |
|---|---|
| `Model::check` walks every element after each command | Checks are cached per element and per rule; an edit rechecks what it touched and what read it |
| A room's boundary scans every element of the project | Rooms read the walls of the tiles around them, cached until one of those walls changes |
| Snapshots share the model until the first change, then clone collections | Persistent maps share everything an edit didn't touch |
| Caches are keyed on the whole model (`Arc` pointer) | Cached by element revision; a change set says what changed |
| Wall index by height only | An XY tile index (and height bands) for joins, rooms, snapping, picking, views |
| Family geometry reused within one operation | Geometry cached per (family, type, values), instances are transforms |
| The 3D scene and the plan rebuilt after an edit | Built from per-element fragments; only changed fragments rebuilt and uploaded |
| Element storage is one map of a big enum | Hot data (bounds, level, category) kept in columns beside it for parallel queries |

## The reference: a 100-storey tower of a million elements

Small samples hide what grows. The baseline is `tower`, an original procedural office tower in
[`sample/tower.rs`](../crates/core/src/sample/tower.rs): 100 storeys at 4 m on an 84 × 60 m plate of 6 m bays, built
through the ordinary commands, about **10,000 elements a storey, a million in all**:

- structure: a slab, 165 columns and the beams of every grid line, four lift shafts;
- envelope: 3 m facade panels, each with two windows;
- **about 195 rooms a storey that bound**: perimeter offices, a ring corridor that has the inner block as an island,
  meeting rooms, and a core of service rooms divided by crossing walls; each with its ceiling, doors and tags;
- furniture and devices in every room; luminaires and their conduits and junction boxes, sprinklers with their
  branch pipes, tees and hangers, air terminals, smoke detectors, fan coils, a cable tray round the corridor.

Rooms are the point of it: today a room finds its boundary from every wall of the project, so the cost of rooms grows
with rooms × elements. A core that scales makes it rooms × neighbours.

The baseline is measured with `examples/performance.rs --fixture tower` and recorded in
[performance.md](performance.md) before anything else changes. Every piece below re-runs it and must not change a
result (the same rooms, areas, checks, plans and IFC), only the time.

## Pieces

Each piece is a set of commits that leaves the workspace green and the product working.

0. **Reference and baseline.** The tower (`tower` command, `--fixture tower`), the measurements that scale (check,
   edit, room boundaries, inspect, plan, scene, save and open, memory), recorded at 1, 10 and 100 storeys.
1. **Oracle harness.** A test helper that runs a command sequence and compares every cached result with its from-scratch
   twin; a seeded fuzzer of random edits on a small tower. In place before the first cache.
2. **Spatial index.** An XY tile index of element bounds, kept per snapshot; room boundaries, wall joins, `wall_holding`,
   `reference_near`, snapping and picking ask it for candidates and keep their exact rules.
3. **Change sets.** Every command reports what it added, changed and removed (derived by diffing the snapshot's maps at
   first, then recorded at the source). Undo and redo report theirs.
4. **Incremental check.** `Model::check` split into per-element rules, project rules and cross-element rules; after a
   command, only the changed elements and what references them are checked; the full check stays in tests and as the
   fallback for commands that change the library or project settings.
5. **Persistent storage.** `Elements` on structurally shared maps, so a snapshot and an edit don't copy the project.
6. **Derived-result cache.** Wall outlines and joins, room regions, family geometry and plan fragments cached by element
   revision with recorded dependencies (elements and tiles read), shared across commands, views and threads.
7. **Views from fragments.** Plans, sections and the 3D scene assembled from cached fragments; the GPU mesh updated in
   chunks; scene work off the UI thread.
8. **Columns and parallel queries.** Bounds, level, category and phase kept in columns; schedules, filters and
   visibility as parallel queries over them.
9. **Chunked files and lazy loading** (later): a project opened level by level, links as partitions.
10. **The same numbers as Revit.** Faster is only worth having if the results are right, and "right" for a BIM tool is
    what the commercial tool its users check against reports. A cross-check replicates the small tower in Revit and
    compares its calculations with ours:
    - BIMCraft writes the tower as a neutral description (`export_crosscheck`: levels, wall axes with their types'
      thickness and height, floors by outline, doors and windows by host and offset, rooms by point), never Autodesk
      content;
    - an original script for Revit's API (in `tools/revit-crosscheck/`, run by the user on Windows, since Revit can't
      run here) builds the same model natively from that description, with its own system families and no template
      content copied, and exports Revit's schedules: each room's area, perimeter and volume, each wall's length, area
      and volume, each floor's area and volume, door and window counts by host, as one table `kind,id,quantity,value`
      (by element id: the same commands give the same ids, where GUIDs are new each time);
    - that table is checked in as a fixture (numbers only), and a test (`crosscheck::tests` in `crates/core`) builds the
      same tower and compares quantity by quantity within a stated tolerance (`compare_crosscheck` does it from the CLI);
    - where the two define a quantity differently (a room's height for volume, a wall's area net of openings, where a
      room is measured), the difference is written down in `docs/areas/schedules.md` and the test applies Revit's rule
      on our side or names the known difference, so no gap is silently accepted.
    The test is in place before piece 7 changes how anything is computed, so every later piece is held to both the oracle
    (the same as before) and Revit (the same as the industry).

## Targets

| Operation | 5,000 elements | 1,000,000 elements |
|---|---|---|
| A local edit (move a column, change a parameter), validated | < 5 ms | < 16 ms |
| Undo | < 1 ms | < 1 ms |
| Every room's boundary (inspect) | < 50 ms | < 2 s, parallel |
| A plan of one storey after an edit | < 5 ms | < 16 ms |
| Open the project | < 0.5 s | < 10 s to the first view |

Timings are measured on a release build on an idle machine, as [performance.md](performance.md) says. Correctness
tests are never traded for a timing.

## Fragment cache contract (implementation priority 2)

The review puts dependency completeness and memory ownership ahead of persistent cache reuse.
The implementation should proceed in these bounded steps:

- [x] Close the `SectionWork::is_of` library-only mutation gap, with regressions for type/family edits,
  undo, redo, branching snapshots and direct model-library mutations.
- [x] Give types/materials/families shared ownership with mutation-sensitive identity and transparent serialization.
  Snapshot cloning and ordinary mutable access preserve isolation; replacing a plain value uses `.into()`.
- [ ] Narrow fragment keys to the individual definitions they read, with retained-allocation accounting rather than
  an `Arc<Model>` that indirectly retains a whole project. The initial `SectionWork` guard is conservative.
- [x] Complete the full workspace regression checkpoint for the shared-definition guard (1,107 core, five cache integration, 621 UI tests, and the remaining suites passed).
- [x] Expose per-element runtime revisions so a changed chunk does not invalidate all of its unchanged elements.
  Revisions survive snapshot copies, and change conservatively when mutable access is granted; GUIDs remain
  the cross-file identity. Runtime revisions are never serialized and never replace authoritative GUIDs.
- [ ] Cache conservative bounds independently of tessellation. Reuse analytic primitive bounds and transformed
  family bounds; keep exact extrema for framing where a looser bound would change output.
  - [x] Lazy column/footing and straight-run bounds, narrow type/level dependencies, configurable byte-budgeted LRU
    (six integration and two independent solid-corner/nonfinite unit tests passed).
  - [ ] Hosted/family bounds and integration with spatial discovery.
    - [x] Swept-profile and imported-mesh family envelopes, transformed to mirrored/rotated component bounds
      (three core unit and seven bounds integration regressions passed).
    - [ ] Individual family-definition keys, void/revolved envelopes, hosted geometry and spatial discovery.
- [ ] Retain geometry fragments with explicit read dependencies: own/reference revisions, host geometry,
  levels, relevant types/families/materials/settings, and spatial queries. Record empty queries too.
- [ ] Invalidate spatial reads around both old and new bounds, including long elements outside ordinary tiles.
- [ ] Account and evict retained data under a byte budget before enabling cross-command reuse in the desktop.

The first library guard may be conservative. It must be exact: hashing alone is not a correctness proof, and
an element-state key cannot detect a library-only edit. Shared immutable definition identities make checks
cheap. Authoritative snapshots preserve definitions, while a derived guard can retain a weak identity that
keeps no old definition data alive and still detaches on unique-owner mutation. Narrow those identities to individual
entries as dependency recording becomes available; measure cloning on library edits separately from local edits.

Runtime revision snapshots should retain metadata only, rather than old element chunks or models. Keep revision
columns in the same id chunks as elements, sharing unchanged columns across snapshots. Compare column identities
first, then only the changed chunks' stamps. Mutable iterators must stamp elements as they are yielded; merely
constructing an iterator or taking its first item must not revise every other element. Insert/remove/clear,
GUID-preserving replacement, skipped snapshots and undo branches all need explicit tests. Revision metadata
must not affect model equality, GUIDs, serialized bytes or the existing semantic change journal.

Runtime revision implementation is in `ids/revisions.rs`; six integration regressions pass. Deletion
now targets indexed referrers and affected views instead of requesting mutable access to every element. Its
original full scan is retained as an independent unit-test oracle; nine core and 12 UI targeted tests pass.
The full workspace checkpoint including the new primitive bounds API passed: 1,110 core, six revision, six bounds,
five cache integration, 621 UI tests, and the remaining suites.

The first independent bounds cache is implemented in `bounds.rs`: lazy column/footing and straight-run
queries, per-element/level revisions and used type dimensions, conservative unknown results, and byte-budgeted
LRU eviction. It does not yet replace section framing or implement hosted/family bounds. Keep the overall
bounds and fragment tasks open until those paths and their output oracles are covered.

The wider deletion run exposed empty reference buckets left by incremental lookup removal, previously hidden
by deletion's full mutable sweeps. Empty reference and rare-kind buckets are now removed; focused regressions
and the corrected full checkpoint passed. The earlier failing run is superseded by that corrected checkpoint.

`Shared::identity()` now provides weak allocation tokens for derived guards; `SectionWork` uses these instead
of strong ownership of whole old type/material/family maps. The three standalone source tests verify unique-owner
mutation, branch identity, and dropping definition data while its token lives. All 17 cache/bounds/revision
integration regressions and workspace Clippy passed for this ownership refinement.

The next bounded step is in `view3d/fragments.rs`: optional `render::SectionCache` retains isolated column/run
geometry and exact extrema under a 128 MiB default byte budget. Prepared work shares retained geometry and
resolves paint/texture ordinals from the current snapshot. The uncached full renderer remains the output oracle.
The output/rebuild/parallel-order integration tests and full workspace checkpoint passed. The uncached renderer
keeps its original contiguous storage, avoiding a full-buffer conversion. Desktop adoption is still pending. This path still enumerates
and assembles the model, and unsupported categories still use full preparation; it does not satisfy the overall
incremental acceptance criterion below.
Counters explicitly report all element visits and uncached preparations separately from primitive hits/builds,
so zero primitive rebuilds cannot conceal work on unsupported categories.

Family envelopes now cover supported swept profiles and imported meshes without constructing triangles. The
component cache transforms their local box corners and conservatively checks a weak family-map identity; unrelated
family edits may still invalidate these boxes. Visible voids and revolved forms return unknown. Three core unit
tests compare supported local bounds with full geometry (including the default library), and seven bounds
integration tests cover placement, direct family edits and annotation reuse. Root-capacity accounting was added
to both bounded caches; the checkpoint passed (1,117 core, seven bounds, four fragment integration, 621 UI tests,
remaining suites and doctests; Clippy clean). These envelopes are query
data; exact section framing still uses the original extrema.

The next dependency audit reproduced stale section fitting after replacing CAD or point-cloud data without an
element edit: the cached model extent also reads their bulk/bounds metadata. `GeometryKey` now captures names
and floating-point bounds bits, retaining no drawing entities or scan points. Two output regressions failed
before this fix; all seven work-cache regressions and workspace Clippy passed afterward.

The next worker-preparation step should use supported conservative component envelopes for culling, and
compute exact component extrema lazily only when fitting, datum spans or a model extent needs them. Keep the
full `SectionWork::new` path as the oracle. Loose boxes must never become framing endpoints; unsupported
families retain the full path. Structural tests should prove that hidden interior components remain deferred,
while outlying components still contribute exact fitting/datum coordinates. Begin with an opt-in core constructor;
desktop adoption must also propagate cancellation through lazy fitting, datum-span and extent resolution, and
must never cache partial extrema after cancellation.

The opt-in core step is implemented as `SectionWork::new_deferred[_cancellable]`, with per-work lazy exact
points and counters. Five integration tests passed, covering interior/outlying components, component-only
models, unsupported forms, construction cancellation, and five views of the two-storey tower. Fitting, PNGs,
picking, face names, coverage and datum endpoints match the full oracle. The final workspace checkpoint passed
at wrap-up: 1,118 core, 621 UI, 181 Revit-reader, 118 updater tests, all integration suites and doctests; formatting
and workspace Clippy are clean. `new` and the existing worker retain full preparation; cancellation through lazy queries and worker
adoption remain pending. The resolved counter counts exact-extrema construction, not visible mesh generation.

### Dependency audit for the next fragment steps

| Product | Reads to preserve | Current boundary |
|---|---|---|
| Column/footing bounds | Own revision, base/top level revisions, used type dimensions and shape | First lazy cache; no triangulation or whole-library retention |
| Straight-run bounds | Own revision, base level revision, used type dimensions and shape | Material name/colour does not affect bounds |
| Component bounds | Used family/type/instance parameters, form profile/axis/mesh and visibility, mirroring, level and placement | Bound local forms before tessellation and transform their corners; retain `Unknown` for unsupported forms. Exact framing still needs the full extrema oracle |
| Painted run solids | Bounds inputs plus effective material colour/transparency and texture assignment | `view3d::skin` encodes a material's ordinal in the sorted library; inserting another material can change that ordinal. Refresh appearance separately or capture this dependency before retaining painted solids |
| Joined wall solids | Own/type/level inputs, openings, both endpoint neighbour queries, neighbour types/heights, attachment geometry | Endpoint queries must invalidate even when formerly empty; moved candidates affect old and new locations. Long/unindexed walls remain global candidates |
| View drawing fragments | View scale/settings, annotation dependencies, effective graphics and linework | `Element::refs` currently omits graphics linework references. Indexed deletion explicitly scans the small view index for them; future drawing dependencies must capture the actual reads |

The primitive bounds cache does not establish a safe key for walls, families, painted solids or view drawings.
Keep the full renderer as the oracle while extending each product's read set.

For the wall step, persist body-tile and height metadata before retaining joined fragments. Record endpoint
tile reads even when their candidate set is empty, plus the unindexed/long-wall candidate set. A type's thickness
changes a wall's body reach without changing the wall's own element revision; update the old/new tiles of its
users before validating neighbours. Attached wall heights also depend on the attachment's geometry/type/levels.
The existing per-operation `walls::Index::new` still scans the model, so retaining that index unchanged would
not remove whole-model preparation. Track type users and changed elements explicitly, then compare local
updates with the existing index and full prisms before desktop adoption.

Start with a bounded CPU fragment cache (initial target 128 MiB, configurable independently of authoritative
undo history). Charge vector capacities, dependency keys, prototype data and retained definition ownership;
shared allocations count once. Oversize fragments may be used for the current operation without being cached.
Eviction must release derived allocations and must not modify the model, invalidate undo, or change output.
Keep cache hit/miss/rebuild/eviction/retained-byte counters for structural tests and performance probes.

Acceptance requires a remote annotation edit to rebuild zero 3D fragments, a local geometry edit to rebuild
only its dependency closure, a library-only edit to rebuild every affected fragment, and all output to match
the from-scratch twin across undo/redo and branches. Do not mark the whole piece complete after only adding
keys, an unbounded map, or a cache that still enumerates the whole model on each local edit.

## Status

- [x] Plan
- [x] 0. Reference and baseline (performance.md: 21 s an edit, 16 s a room at a million elements)
- [x] 1. Oracle harness: rooms and walls against brute force; the full check after every command in tests; a seeded fuzzer
- [x] 2. Spatial index: wall joins by plan tile, rooms a level at a time (a plan of 10 storeys 326 s → 0.15 s)
- [x] 3. Change sets: each model's journal; sweeps, undo and redo by what differs
- [x] 4. Incremental check (an edit of a million elements 21.5 s → 0.27 s with piece 5)
- [x] 5. Persistent storage: chunks of 1,024 ids shared by copies (memory 2.2 → 1.1 GB)
- [ ] 6. Derived-result cache: the lookup (references both ways, walls by tile and height, levels, grids, columns, the kinds a project has few of), the library's parts persistent and synced by what changed, undo that doesn't check again. The annotated tower (1.2 million elements): every edit, 2D or model, 4.5 to 10.5 ms; undo under 0.01 ms; the full check 2.2 s
- [ ] 7. Views from fragments: begun: a plan's passes over the elements that may show in it (each drawn again among every element in tests), regions' ink by grid; with 2,000 2D elements a storey, a storey's plan at 1.2 million elements 2.8 s -> 0.45 s; the lookup finds a plan's elements (components and runs by height band, the library's part added as it asks; 2D by view; doors by wall): 0.45 s -> 0.17 s, 155 ms for the storey alone. A drawing set of 406 sheets (`tower {sheets: true}`): sheets, viewports and views checked as a set (a new sheet 2.2 s -> 12 ms), a sheet's views from its referrers (a plan's sheet 1.2 s -> 0.23 s). Left: per-element fragments, and the 3D scene at scale: sections and elevations have a plan of their own, [section-views-plan.md](section-views-plan.md)
- [ ] 8. Columns and parallel queries
- [ ] 9. Chunked files and lazy loading
- [ ] 10. The same numbers as Revit: `export_crosscheck`, the script (`tools/revit-crosscheck`), `compare_crosscheck` and the test are in, rooms' height and volume among the quantities since #181 merged in; waiting for Revit's table of the small two-storey tower, to be run on Windows
