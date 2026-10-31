# Performance baseline

The shareable fixture is an original procedural office tower: 48 × 36 m, 6 m structural bays,
20 storeys at 3.6 m, and 9,521 elements. It uses ordinary BIMCraft walls, windows, structural
columns, beams, floors, ceilings and furniture. Its source is
[`sample/highrise.rs`](../crates/core/src/sample/highrise.rs); it contains no customer geometry,
identifiers, names, locations or other project data. It is a load test, not construction documentation.
Generate another size with `highrise {"storeys": 40}` (1–80).

```sh
cargo run --release -p bimcraft -- --highrise
cargo run --release -p bimcraft-cli -- run --highrise --save dist/highrise.bimcraft
# Equivalently, use the small version-controlled script:
cargo run --release -p bimcraft-cli -- run --script samples/highrise.txt --save dist/highrise.bimcraft
```

## Repeatable measurements

Build first, then benchmark with the machine idle. Do not compare debug builds, cold dependency
builds, or measurements made while compiling or running the test suite. Record the machine,
build, resolution, thread count and fixture size. CPU rasterization scales with pixel count.

```sh
cargo build --release -p bimcraft-core --example performance
# Generates native/IFC files and plan/3D PNGs under ignored dist/.
target/release/examples/performance --repeat 9 --output dist/performance --report dist/performance.json
# Measures complete egui CPU frames after first paint; excludes GPU presentation.
cargo run --release -p bimcraft-ui-egui --example redraw
```

The core report separates fixture construction/import, complete model validation, a read command,
a validated 10 mm element move with undo between samples, native serialization, plan fitting,
plan regeneration, PNG encoding, scene construction and changing-camera frames. Frame and edit
samples include medians; the first frame warms the renderer. The UI probe also reports p95 and
maximum frame times, since medians alone can hide hover/highlight stalls. Full model validation and rollback
remain enabled. This is a baseline for these operations, not a claim that every command has the
same cost. The synthetic fixture's construction is not a measure of IFC import speed.

The egui probe forces idle and moving-pointer frames in plan and 3D. It measures the UI path,
including view lookup, picking and paint generation. An idle 3D frame reuses the existing image;
its time is not orbit rendering throughput. Core frame times exclude GPU upload/presentation,
interactive input latency and scene rebuild after an edit. Core frames use `Scene::render`'s
all-visible shaded default; the UI probe includes project/view graphics, so their costs differ.

## Private local fixtures

`/test_model/` is ignored in Git, including reports, exported models, images and derived files.
Keep all results from private inputs there. Do not move them into public sample or documentation
folders. Inspect source IFCs outside BIMCraft with IfcOpenShell. For local measurements:

```sh
target/release/examples/performance --input test_model/PRIVATE.ifc --repeat 9 --report test_model/results/private.json
```

Reports from private inputs can contain sensitive import error messages. Do not publish them.
The probe records the number of skip notices; inspect the full `open` command response locally
before interpreting a timing as successful import of the source building. Some IFC classes and
representations are unsupported, so a fast partial import is not a faithful full-project benchmark.
For example, a space-only coordination file may yield levels but no usable building geometry.

## Changes driven by the baseline

* Plans reuse their drawing while the immutable model snapshot, level, framing and plan/ceiling
  mode are unchanged. Selection, hover, tools and theme remain live overlays. Changes, undo,
  graphics overrides, zoom, resize and switching levels invalidate the cache. Each canvas keeps
  one drawing; it holds a weak model reference and cannot keep old project snapshots alive.
* Plan projection rejects walls outside the view's depth range before computing expensive joins.
* Scene and plan generation share a snapshot-local index of wall height bands and hosted
  openings. Candidate walls still go through the original exact overlap/join rules, in stable
  element order. Tall walls use a broad fallback, and bucket references are capped at one million
  to prevent unbounded expansion. This is a vertical index, not yet an XY spatial tree.
* Repeated component instances reuse local family geometry within an operation. Cache keys include
  family, type, mirroring and instance parameters. Heights are tested before placing off-floor
  geometry. No mutable model or global geometry cache is introduced.
* Large scenes build geometry in parallel from immutable inputs. Indexed chunk collection preserves
  element/triangle order and ownership exactly. Small scenes use the serial path.
* The software rasterizer projects once per triangle, bins triangles into 32-row bands and renders
  bands in parallel. Each job owns its depth, hit and lighting rows; triangle order within a band
  is stable. Opaque and transparent passes retain their original order. Final pixel shading is
  parallel too. One persistent pool serves both geometry and rendering, with at most eight workers
  and one logical CPU left available when possible. Small images stay serial; thread-creation
  failure falls back to serial work. No model mutations run concurrently.
* An IFC railing with a nonpositive/unmeasurable height is skipped with a diagnostic before a bad
  type enters the library. It no longer invalidates the entire project import.

Regression tests compare parallel geometry and raster depth, picking and smooth-light buffers
with the serial implementation. Existing rendering tests cover transparent materials, textures,
sections, visual styles and theme behavior. Cache tests check actual geometry after edits and undo,
level/framing changes, ceiling mode, mirroring and differing family instance values.

## Recorded baseline (2026-10-22)

Release build on a Ryzen 7 2700X (8 cores / 16 logical CPUs), eight rendering workers,
1280 × 720. Three alternating before/after runs were taken after all builds and tests finished;
each run includes nine changing-camera frames and nine edits. The table uses medians across
runs. [Raw runs, machine details and aggregate metrics](performance/baseline.json).

| Operation, 9,521 elements | Before | After |
|---|---:|---:|
| Plan regeneration, without UI cache | 344 ms | 6.7 ms |
| Fit and draw a plan | 612 ms | 13.7 ms |
| Build 3D scene | 1,002 ms | 312 ms |
| Changing-camera CPU frame | 328 ms | 137 ms |
| Validated edit | 96 ms | 94 ms |
| Full model check | 87 ms | 88 ms |

Plan regeneration improved about 51×, scene building 3.2×, and changing-camera rendering 2.4×.
Validation is unchanged; the small edit-time difference is not claimed as an optimization.
137 ms per changed frame is about 7 frames/s, so this is still far from the 60 Hz target.

The post-test UI probe at 1280 × 800 measured 2.0 ms idle plan frames (2.3 ms while moving the
pointer), and 1.4 ms idle 3D frames. However, 3D hover's p95 is 129 ms (maximum 139 ms) because
crossing into another element invalidates the tinted image. On the processor that stall remains; on the card a changed
tint is a 1.3 ms draw (see below), which is the retained GPU highlighting it needed.

An additional 80-storey probe (38,081 elements) measured approximately 27 ms plan regeneration,
376 ms edits, 380 ms changing-camera frames and 1.23 s scene construction. It ran during the
correctness sweep, so treat those numbers as a scaling diagnostic, not an idle-machine baseline.
This exposes the remaining cost of whole-model validation and processing every instance.

## The reference tower: the baseline of the next core (2026-10-31)

`tower` ([`sample/tower.rs`](../crates/core/src/sample/tower.rs)) is the original reference building of the next core
([plan](spatial-core-plan.md)): 100 storeys at 4 m on an 84 × 60 m plate, **10,017 elements a storey and 195 rooms that
bound**, 1,001,701 elements in all. It is built through the ordinary commands, then stacked.

```sh
cargo build --release -p bimcraft-core --example performance
target/release/examples/performance --fixture tower --storeys 1 --repeat 5 --output dist/tower1 --report dist/tower1.json
target/release/examples/performance --fixture tower --storeys 10 --repeat 3 --rooms 100 --skip ifc,inspect --report dist/tower10.json
target/release/examples/performance --fixture tower --storeys 100 --repeat 3 --rooms 3 --skip ifc,inspect,plan,scene --report dist/tower100.json
```

Release build, Ryzen 7 2700X, idle apart from the probe; one run each. [Raw reports](performance/tower-baseline.json).
A room's boundary is timed over a sample of rooms spread over the storeys; "every room" is that times the rooms.

| Operation | 1 storey (10,018) | 10 storeys (100,171) | 100 storeys (1,001,701) |
|---|---:|---:|---:|
| Build (one storey through commands, then stacked) | 68.8 s | 69.8 s | 90.8 s |
| Full model check | 215 ms | 2.07 s | 21.3 s |
| Validated edit (move a column 10 mm), median | 220 ms | 2.16 s | 21.5 s |
| One room's boundary | 48.5 ms | 1.63 s | 16.3 s |
| Every room's boundary (estimated) | 9.5 s | 53 min | 88 h |
| `inspect` (every room's boundary) | 9.3 s | skipped | skipped |
| Fit and draw the plan of Level 1 | 10.9 s | 322 s | skipped |
| Build the 3D scene | 605 ms | 8.1 s | skipped |
| Changing-camera CPU frame, 1280 × 720 | 270 ms | 2.17 s | skipped |
| Write the project | 29 ms | 283 ms | 3.36 s (366 MB) |
| Read it back | 96 ms | 905 ms | 10.3 s |
| IFC export | 54.7 s | skipped | skipped |
| Memory after load / peak | 1.3 / 1.5 GB | 1.4 / 5.0 GB | 2.2 / 4.4 GB |

What it says:

* **Every edit costs the whole project.** Check and edit grow with the element count: 21 s to move one column of a
  million-element project. The check is most of an edit.
* **Rooms grow faster than the project.** A room's boundary takes its level's bounding walls with their joins, and each
  wall's joins are found among every wall of the project, so one room is levels' walls × all walls: 34× from 1 to 10
  storeys. Every room of the tower would take days.
* **A plan is its rooms.** The plan of one storey is 10.9 s at one storey and 322 s at ten, nearly all of it the room tags'
  areas, each finding its boundary again (the 20-storey highrise has no rooms and draws in 6.7 ms).
* Building one storey through 400-odd commands takes 69 s, because each command checks the whole storey; stacking a
  hundred is 22 s more.
* Reading and writing are linear and fine; memory is 2.2 GB for a million elements.

### Piece 2: wall joins by plan tile, rooms a level at a time

A wall's joins are looked for among the walls reaching the same 6 m plan tile, and one look at the model
(`rooms::Rooms`) arranges each level's room-bounding lines once for all its rooms. Same machine, release, one run each:

| Operation | 1 storey, before | after | 10 storeys, before | after |
|---|---:|---:|---:|---:|
| Fit and draw the plan of Level 1 | 10.9 s | 274 ms | 322 s | 394 ms |
| Redraw it | 10.4 s | 68 ms | 326 s | 151 ms |
| `inspect` (every room's boundary) | 9.3 s | 7.8 ms | 53 min (estimated) | 163 ms |
| One room's boundary, alone | 48.5 ms | 5.7 ms | 1.63 s | 17.2 ms |
| IFC export | 54.7 s | 984 ms | skipped | skipped |
| Check, edit, scene, files | unchanged | | unchanged | |

The plan and the 3D picture of one storey are byte for byte the ones before. Every room at both measures, and every
wall's centre line, caps, outline, layers and prisms, are tested against the way they were found before.

### Pieces 3 to 5: change sets, the incremental check, shared storage

Each command's model knows which elements it added, changed or removed; the engine checks those, the walls they might
join and what points at them (the full check stays where a change reaches levels, views, sheets or settings, and in
tests, after every command, as the oracle). Elements are kept in chunks of 1,024 ids that copies share until they change
them, so the copy a command makes and the snapshot undo keeps cost a pointer a chunk. A storey's check also builds each
family, type and values' shape once instead of each component's.

The 100-storey tower (1,001,701 elements), release, one run each, edit the median of five:

| Operation | Baseline | Piece 2 and the shapes | Pieces 3–5 |
|---|---:|---:|---:|
| Validated edit (move a column 10 mm) | 21.5 s | 1.66 s | **272 ms** |
| Full model check | 21.3 s | 1.19 s | 1.27 s |
| Build the tower | 90.8 s | 10.9 s | 5.6 s |
| `inspect`, every room's boundary (19,500) | ~88 h (estimated) | 9.3 s | 10.1 s |
| Fit and draw the plan of one storey | didn't finish | 1.53 s | 1.68 s |
| One room's boundary, alone | 16.3 s | 128 ms | 140 ms |
| Memory after load / peak | 2.2 / 4.4 GB | 2.2 / 4.4 GB | **1.1 / 3.5 GB** |
| Write / read the project | 3.4 / 10.3 s | 3.2 / 10.1 s | 3.4 / 10.7 s |

At 100,171 elements (ten storeys) an edit went 156 → 29 ms. What is left of an edit at a million is the incremental
check's walk over every element to find what points at what changed: an index of references kept with the model
(piece 6) makes it what changed. A plan of one storey and `inspect` still go through every element of the project:
plans by level (piece 7).

### Piece 6: what elements point at, kept with each model

`derived::Lookup` keeps, with each model, what each element points at and what points at it, the walls by 6 m plan tile
and the dimensions, in chunks behind `Arc`s; a command's copy brings its model's lookup up to date by what it changed.
The edit path asks it, and the change set, instead of going through every element: the incremental check follows
references and finds the walls a changed wall might join in its tiles (overlapping in height); moving finds the walls to
stretch and the openings to slide there; keeping locked dimensions, resolving references and whether the library is in
step look at what changed.

The million-element tower, median of nine, each undone after it:

| Edit | Baseline | Pieces 3–5 | Piece 6, plan tiles | Piece 6, tiles by height | Undo |
|---|---:|---:|---:|---:|---:|
| Move a column 10 mm | 21.5 s | 272 ms | 1.7 ms | **1.65 ms** | 1.0 ms → **under 0.01 ms** |
| Move a corridor wall with 24 doors and the partitions joined to it | | | 23 ms | **8.3 ms** | 14 ms → **under 0.01 ms** |

With plan tiles alone, the wall's neighbours were the same wall of every storey above and below. Each tile now holds its
walls in bands of 1 m of height, by the heights their levels give them (a wall attached to a roof or floor is in every
band, and a level that changes makes the lookup again), so the check and Move ask for the walls near in plan and in
height: the check went from 10 ms to 1.4 ms. The bands sit behind `Arc`s within each tile, so a copy shares what an
edit didn't touch. Undo and redo go back to a model that passed the check and take it as it was: each step remembers
whether its model passed (a model opened and never checked is still checked when undo comes back to it).

## 3D on the graphics card

`view3d::gpu::Mesh` keeps a view's shown triangles for the card (retained geometry, uploaded once for each model), and
`crates/ui-egui/src/gpu.rs` draws them in passes: the opaque surfaces, those seen through (only when the view sees through any),
edges, ground and the blend, then the levels' lines. It is opt-in
(File > Settings > Performance). Release build on Linux, an NVIDIA GeForce RTX 3060 over Vulkan, the 20-storey tower
(427,280 triangles, 1,281,840 vertices) at 1920 × 1080 with the camera turning 3° a frame, median of nine:

| Operation | Processor | Card |
|---|---:|---:|
| Changed-camera frame | 108.1 ms | 1.3 ms |
| First frame of a model (builds the mesh, 67 MB upload) | | 62.3 ms |
| Pick under the cursor, first (builds the hierarchy) | | 124 ms |
| Pick under the cursor, after | | 0.2 ms |
| Mesh built from the scene | | 60 ms |

A changed frame is 83 times faster, and a tint changed by hover or selection is another 1.3 ms draw, not a second rasterization.
`cargo test -p bimcraft-ui-egui --release gpu_speed -- --ignored --nocapture` repeats it. On the sample house at 640 × 480 the card's
picture differs from the processor's by more than six levels of a channel in 358 of 307,200 pixels (0.12%), all on the level lines,
which the card draws as quads and the processor as stamped pixels. Picking through the mesh agrees with the processor's picture at every
sampled pixel.

Then the card's own costs (2026-10-23, an NVIDIA GeForce RTX 2060 over Vulkan, Ryzen 7 2700X, same tower, 1920 × 1080, 3° a frame,
55 frames timed back to back after five). The first measure timed each card frame straight after a processor's picture, which on this
card is 24 ms: the card has gone idle and wakes for it, which a window turning the view doesn't make it do. Back to back it was 1.5 ms.

| Frame, median | Before | After |
|---|---:|---:|
| Nothing selected | 1.47 ms | 1.15 ms |
| Every wall selected (about 2,000 tints) | 2.87 ms | 1.12 ms |
| Of it, the processor's part (nothing selected / every wall) | 0.31 / 1.80 ms | 0.19 / 0.19 ms |
| Textured | 1.56 ms | 1.15 ms |
| Windows seen through (60%) | processor, 224 ms | 1.20 ms |

What changed: a vertex is 44 bytes, its two normals 16-bit fractions (52 before; 56 MB for the tower, not 67); the opaque pass writes
a pixel's plane and element (8 bytes, not 16), the depth is read from the depth target itself, which OpenGL couldn't but the card no
longer runs on; the tint is the picture's, applied once a pixel in the edge pass rather than at each surface drawn over it; the
owners' tints (looked up in a map, not searched for each owner) and the textures' lookups are written again only when they change,
and the buffers' bind group is kept. The p95 of the frames with nothing selected, 12 to 17 ms, is the card coming up to speed after
the mesh is built: the frames after are as quick as the median.

Surfaces seen through are a pass of their own after the opaque ones, into targets of their own: the nearest of them where it is nearer
than the opaque depth, as `Scene::picture`'s second raster finds it; the edge pass blends it over what is behind it by how much the
view lets show (a tinted one at least 35%). Edges and level lines stay the opaque surfaces', as the processor's do. The sample house
with its walls 60% see-through differs from the processor's picture in 255 pixels of 307,200, with a mass in it 192.

Not measured: integrated cards, macOS and Windows, a model of several million triangles (the mesh holds up to 1.6
million and falls back to the processor above that).

## Verification

`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` passed: 850 tests (471 core, 253 UI, 118 updater, 8 CLI).
Plan and 3D screenshots were inspected in both light and dark themes. The original and optimized
1280 × 720 plan/3D PNGs have identical SHA-256 hashes for the same tower and camera.

The generated IFC passed IfcOpenShell validation with EXPRESS rules (zero statements), and
representative geometry from every generated class was built. A 300 × 300 mm, 3.6 m column is
0.324 m³; a 6 m, 200 × 500 mm beam is 0.600 m³, matching the resulting meshes.
All four private inputs completed the final import/render probe; imported element counts were
unchanged by the indexing optimization. Their detailed reports and skip diagnostics stay local.

## Targets and next steps

The long-term interaction targets are 16.7 ms for a changed view (60 Hz) and under 100 ms for an
ordinary validated edit on a mid-range laptop. These are targets, not achieved guarantees. Idle
redraws and changed-camera rendering must be tracked separately. A stable dedicated runner is
needed before a relative timing regression gate can reliably block CI; shared-runner timings
should be recorded as artifacts first. Keep correctness tests mandatory regardless of timings.

The measured next priorities are:

1. Extend the vertical wall/host index with XY neighbour searches and shaft/slab intersections.
   Existing repeated scans remain costly; parallel execution does not remove quadratic work.
2. Cache derived geometry by element revision and dependency, including family instances and wall
   joins; invalidate only affected elements after commands.
3. Move scene construction and rendering off the UI thread, using immutable snapshots, bounded
   latest-request queues and cancellation of obsolete views. The current parallel renderer still
   waits for its workers on the calling thread.
4. Retained GPU geometry for orbit/pan/zoom is done for shaded, opaque views (above), with the processor as the
   reference and the fallback, and for surfaces seen through. Still to do on the card: hidden line and wireframe, then viewport
   culling, level of detail and instancing before pursuing full-size multi-million-triangle scenes.
5. Profile IFC stages and reuse representation/placement decoding. Expand import coverage alongside
   speed work; never count skipped source geometry as a performance improvement.
6. Introduce dependency-aware validation while retaining a full-check oracle and command rollback.
   Keep command mutation ordered; parallelize independent read-only work rather than racing edits.

No matched Revit benchmark was run. Outperforming it requires the same represented model,
operation, view quality, resolution, hardware, warm/cold state and correctness checks in both apps.

The CPU worker implementation uses [Rayon's indexed parallel iterators and disjoint slice chunks](https://docs.rs/rayon/latest/rayon/).
Rayon and rayon-core are MIT OR Apache-2.0 and require Rust 1.80, below the core's Rust 1.90 minimum.
