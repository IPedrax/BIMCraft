# Large projects: opening, saving and checking (to-do)

Found 2026-11-13 while opening the 100-storey reference tower (`tower {"storeys": 100}`, 1,001,701 elements) in the
desktop app. Numbers are from a release build on the author's machine (debug is roughly ten times slower), measured first
with a throwaway timing test that read the file stage by stage, as `open` does, then (2026-11-14) in the window itself.

## In the window (2026-11-14)

The headless numbers below were true and misleading: the window was far slower than the CLI. Measured with `bimcraft --bench`
([the desktop](areas/desktop.md#benchmarking-the-window)) on the annotated tower (`tower {"storeys": 100, "annotation": true}`,
1.2 million elements, 455 MB of pretty JSON), 1920 × 1080, RTX 2060, 16 threads, the main thread sampled with `eu-stack`:

| | Before | After | What it was |
|---|---:|---:|---|
| Opening, in one frame | 25.4 s | 16.6 s, then on a thread with a progress box and Cancel | the read on one core |
| The first frame of the plan | 27.3 s (minutes before the fix below) | 17.9 s | each run's end scanned every element (O(n²)): `runs::arms` without an index; the canvas fitted the plan by drawing it |
| Every frame after, idle or moving | 830 to 990 ms | 95 to 165 ms | the equality control and the padlocks of dimensions, the Properties palette and the browser each walked every element, every frame; each dimension's stations were found, wall outlines and all |
| Opening the 3D view | out of memory: 35 GB and climbing at 3 minutes, of 46 | streamed (below) | the scene keeps every triangle of every copy in full, about 93 million |

What changed:

- `Model::wall_index` keeps the walls' and runs' tile index with the model, by its elements' version and its types, so a run's
  joins never scan the project.
- The canvas's per-frame passes over dimensions, the Properties palette's `default_3d`, the browser's 3D views and panel
  schedules ask the lookup (`Lookup::dimensions`, `Lookup::rare`) rather than walk every element.
- `render::plan_fit` stands a plan's view without drawing the plan.
- `open` of a file of 16 MB or more runs on a thread of its own (`ui-egui/src/opening.rs`), with the core's `Progress` (stage,
  elements read, Cancel through the read's `Budget`).
- The file is compact JSON, read element by element on every core; Compressed save (an option) zips it (`bimc.rs`), deflated and
  inflated on every core.
- A 3D view of a model bigger than a quarter of the memory that's free (at most 8 GB) holds the building's fabric and the detail of the
  storeys round where it looks (`view3d::stream`), and streams other storeys' as the camera moves to them.
- A banner says when the machine starts to swap (`memory::SwapWatch`, `ui-egui/src/swap.rs`).

After (same tower, zipped, `bimcraft --bench`, the window's frames):

| | Now |
|---|---:|
| Opening | 17 s on a thread, the window at 60 fps throughout (its longest frame 0.9 s, when the model is put in place) |
| A frame with nothing to do | 16.7 ms |
| Zooming (10 notches) | 38 ms a frame, lines sharp (`motion.rs`) |
| Panning | 33 ms a frame |
| The 3D view | built in 27 s on its worker, 12.7 GB at the peak, orbiting at 37 ms a frame (measured with a build running) |

Still to do, measured on the tower's open (about 16 s): `migrate`, `names_from_file` and `sync_library` on one core (about
5 s); `Model::check` twice, once by `open` and again by the engine after it (about 3 s each, the per-element part now on every
core); the 3D view's streamed scene holds one storey's detail in 8 GB, as the annotated tower's fabric alone takes most of it,
and is made afresh, not brought up to date, on an edit.

## What happened

1. `bimcraft-cli run --cmd 'tower {"storeys": 100}' --save tower.bimcraft` wrote a **367 MB** file.
2. The desktop refused to open it: projects are capped at **256 MiB** (`MAX_FILE`, `commands.rs`, 268,435,456 bytes).
3. The file is pretty-printed JSON (13.7 million lines). Re-writing it compact (`python3 -I -c "import json;
   d=json.load(open('IN'));json.dump(d,open('OUT','w'),separators=(',',':'),ensure_ascii=False)"`; not `jq`, which rounds
   the project's 64-bit `uid`) gave **260.5 MB**, which opens: all 1,001,701 elements, checked with `bimcraft-cli info`.
   Saving it again from BIMCraft writes it pretty, over the cap again.
4. Opening it, the window runs on one core (100%) and doesn't answer until it is done.

## Where the time goes

`open` of the compact tower, about 15 to 17 s, every stage on one thread (`bimcraft-cli run FILE`: 16.9 s, 3 GB peak):

| Stage | Time | Where |
|---|---|---|
| Read the file | 0.2 s | `open_file` |
| Text → `serde_json::Value` | 2.7 s | `Model::read` |
| Lenient walk of every element (each cloned, walked field by field, a `String` path built per key) | 2.7 s | `Model::read`, `lenient::element` |
| `Value` → `Model` (`from_value`) | 5.6 s | `Model::read` |
| `lenient::unknown::<Model>` of the rest, `migrate`, `names_from_file` | ~1.3 s | `Model::read` |
| `adopt_materials`, `sync_library` | 0.2 s | `open_file` |
| `Model::check` | 1.5 s | `open_file` |
| The lookup, made on first use | 0.9 s | `Model::lookup` |

For comparison: `serde_json::from_str::<Model>` straight from the text took **5.4 s**, against 11.5 s for `Model::read`.
`to_json` of the same model took 4.3 s.

Other costs found on the way:

- `inspect` took **28 s** more: `Model::warnings` works out every room's boundary (`Rooms::bound`, 19,500 rooms) to report the
  unenclosed ones. The desktop doesn't call it; the CLI, scripts and agents (MCP `inspect_model`) do.
- **Autosave** serializes the whole model (`to_json`, 4.3 s for this one) and refuses a snapshot at or over `MAX_FILE`
  ("the recovery snapshot exceeds the project file size limit"), so a project this big gets no recovery copy.
- **Snapping** (`snap::Scene::of`) walks every element on every cursor move: 3.4 ms debug for the 20-storey highrise's 9,521
  elements, so about 0.3 ms release; linear in the project, so a few ms on the tower. Fine for now, the next thing after this list.

## To do, best first

1. **[Done, 2026-11-14] A fast path for reading.** Each element is read strictly on every core, and only one that fails is looked into the lenient way (`Model::read_on_every_core`, with `read_whole`, the way it was, as the oracle); the top and the elements are scanned once, and the elements put together at once (`Elements::of_read`). In `Model::read`, for a file of the current `FORMAT`, read the text straight into the model
   (`serde_json::from_str::<Model>`); only if that fails (an unknown field, which `deny_unknown_fields` refuses) fall back
   to today's lenient path, which still lists what it leaves out. Saves about 6 s of the 11.5. Keep the lenient tests and
   add one that a file with an unknown field still opens through the fallback.
2. **[Partly done] Use every core.** Reading the elements, and the per-element half of `Model::check` (`tally_on_every_core`, the one-by-one check its oracle), run on every core; `migrate`, `names_from_file` and `sync_library` don't yet (about 5 s of the tower's open). Elements are independent: the lenient walk, the element part of the conversion to `Model`, and the
   per-element half of `Model::check` can run in parallel with rayon (already a dependency of the core; see
   `view3d.rs`'s pool). Keep the order of ids and the first error reported the same as the serial path, and keep the
   serial one as the oracle in a test.
3. **[Done] Open off the interface thread.** `BimApp::open_path` runs `open` in the frame, so the window freezes. Read and check the
   model on a worker (as `section_worker.rs` and `view_worker.rs` do for views), show a progress box, and install the
   model when it is done; a failure leaves the open project as it was.
4. **[Done] Write compact JSON, and raise the cap.** Compact: 455 MB → 316 MB. Compressed save, an option (File > Settings >
   Files, the File menu; `save {compress: true}`), zips it (`bimc.rs`): 44 MB, slower to save and open. `open` reads either.
   `MAX_FILE` is 1 GiB of JSON where nothing measures memory. `Model::to_json` writes pretty JSON: compact is about 30% smaller (367 →
   260 MB) and faster to write and read. Then size `MAX_FILE` for the towers the core is measured on (the reference
   tower must save and open again), keeping a cap: files are hostile. An older BIMCraft reads compact JSON as it is, so
   no `FORMAT` bump.
5. **Warnings without every room.** `Model::warnings` could take each level's rooms once (`rooms.rs` already finds a
   level's boundaries together) and in parallel, or `inspect` could say how many warnings there are only when asked.
6. **[Done] Autosave of a big project.** It was on its worker already; it is compact now, under the raised cap. Serialize the snapshot on the autosave worker (it may already be; check), compact,
   and under the raised cap, so the tower gets a recovery copy.
7. **Snapping by the lookup.** `snap::Scene::of` takes candidates from `Model::lookup` near the cursor instead of walking
   every element (the rule [building codes](areas/building-codes.md#every-check-is-fast) sets for code checks holds here too).

Measure each on the tower before and after (`tower {"storeys": 100}` then `save`, or the compact file), and record the
numbers in [performance](performance.md).
