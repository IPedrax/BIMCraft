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

## Status

- [x] Plan
- [x] 0. Reference and baseline (performance.md: 21 s an edit, 16 s a room at a million elements)
- [x] 1. Oracle harness: rooms and walls against brute force; the full check after every command in tests; a seeded fuzzer
- [x] 2. Spatial index: wall joins by plan tile, rooms a level at a time (a plan of 10 storeys 326 s → 0.15 s)
- [x] 3. Change sets: each model's journal; sweeps, undo and redo by what differs
- [x] 4. Incremental check (an edit of a million elements 21.5 s → 0.27 s with piece 5)
- [x] 5. Persistent storage: chunks of 1,024 ids shared by copies (memory 2.2 → 1.1 GB)
- [ ] 6. Derived-result cache: begun with the lookup (references both ways, walls by tile and height, dimensions) and undo that doesn't check again: at a million elements a column edit 1.3 ms, a wall with its doors 6 ms, undo under 0.01 ms
- [ ] 7. Views from fragments
- [ ] 8. Columns and parallel queries
- [ ] 9. Chunked files and lazy loading
- [ ] 10. The same numbers as Revit: `export_crosscheck`, the script (`tools/revit-crosscheck`), `compare_crosscheck` and the test are in; waiting for Revit's table of the small two-storey tower, to be run on Windows
