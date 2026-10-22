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
crossing into another element invalidates the tinted image. Retained GPU highlighting or reuse
of raster results is necessary to remove that stall; a 1.4 ms idle median is not 60 Hz orbiting.

An additional 80-storey probe (38,081 elements) measured approximately 27 ms plan regeneration,
376 ms edits, 380 ms changing-camera frames and 1.23 s scene construction. It ran during the
correctness sweep, so treat those numbers as a scaling diagnostic, not an idle-machine baseline.
This exposes the remaining cost of whole-model validation and processing every instance.

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
4. Use retained GPU geometry for orbit/pan/zoom, with shared picking/depth semantics and a CPU
   export/reference path. Add viewport culling, level of detail and instancing before pursuing
   full-size multi-million-triangle scenes.
5. Profile IFC stages and reuse representation/placement decoding. Expand import coverage alongside
   speed work; never count skipped source geometry as a performance improvement.
6. Introduce dependency-aware validation while retaining a full-check oracle and command rollback.
   Keep command mutation ordered; parallelize independent read-only work rather than racing edits.

No matched Revit benchmark was run. Outperforming it requires the same represented model,
operation, view quality, resolution, hardware, warm/cold state and correctness checks in both apps.

The CPU worker implementation uses [Rayon's indexed parallel iterators and disjoint slice chunks](https://docs.rs/rayon/latest/rayon/).
Rayon and rayon-core are MIT OR Apache-2.0 and require Rust 1.80, below the core's Rust 1.90 minimum.
