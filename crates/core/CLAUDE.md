# bimcraft-core

The model, the commands, and everything that draws or exports it: plans, the 3D scene, sheets, schedules, IFC. It has no UI
toolkit (a rule in the root `CLAUDE.md`), so a front end is a thin shell over [`Session`](src/engine.rs). The map of the modules
is in the root `CLAUDE.md`; the write-ups by area are in `docs/areas/`, which this file points at by task.

## Where a change goes

- **A feature is a command** (a new one, or a parameter of one): an id after Revit's tool name, a Revit-style shortcut, a params doc
  written for agents, a JSON `run`, in `commands.rs` (`ALL`). Its test drives it through `Session::execute`, as the CLI, scripts
  and MCP do. A command that makes an element returns what it made; one that changes the project goes through `Session::model_mut`.
- **A rule on the model** is a check in `Model::check` (`model.rs`), with an error that says what to fix. The engine rolls back
  when it fails, so a command never needs its own undo. Several commands that are one action go in `Session::transaction`: all or
  nothing, and one undo step (`script_atomic` is a script in one).
- **A new kind of element** is a variant of `Element` and its arms in the places that match on it (`category`, `params`,
  `level_of`, `element_json`, copy and delete, IFC, import, the schedules' `OWN` table): the compiler lists them.
- **References hold GUIDs**, never ids (`ids.rs`, the root rules); library entries are named in memory and held by GUID in a file.

## Read by task

| Task | Read |
|---|---|
| Levels, walls, floors, roofs, ceilings, shafts, stairs | [levels, walls and roofs](../../docs/areas/levels-walls-roofs.md) |
| Plans, sections, 3D, visibility and graphics, templates, scale and detail | [views and graphics](../../docs/areas/views-and-graphics.md), [performance](../../docs/performance.md) |
| Schedules and project parameters | [schedules](../../docs/areas/schedules.md) |
| Phases and design options | [phasing](../../docs/areas/phasing.md) |
| Masses and what they make | [masses](../../docs/areas/massing.md) |
| Families, forms, materials, textures, paint, favorites | [families](../../docs/areas/families.md) |
| Beams, columns, footings, pipes, conduits, fixtures | [structure and MEP](../../docs/areas/structure-and-mep.md), [disciplines](../../docs/disciplines.md) |
| IFC export or import | [looking at the result](../../docs/areas/looking-at-results.md): the IfcOpenShell checks |
| Autosave and recovery | [autosave](../../docs/autosave.md) |

## Testing

- Filter while you work: `cargo test -p bimcraft-core <part of a name>`. A module's own tests carry its path, so `schedule::`,
  `view3d::`, `phasing::`, `graphics::`, `render::`, `library::`, `walls::` and `snap::` each run one area; the engine tests in
  `tests.rs`, `mep_tests.rs` and `structure_tests.rs` are named for what they check (`cargo test -p bimcraft-core tags`).
- The quick tier is every test but one: `cargo test --workspace -- --skip hostile_params_never_panic_or_break_the_model`. The core's
  share runs in about 25 seconds, the whole workspace's in a couple of minutes once built.
- That one, `hostile_params_never_panic_or_break_the_model` (`tests.rs`), sweeps every command with every hostile input and takes about a
  minute on twelve cores; its cost grows with the commands and the payloads. Run it before you finish (`cargo test --workspace`); CI
  always does. A new command gets payloads with its real keys and a target in the sweep's base project (the detailing commands' are
  `detail_tests::sweep_payloads`, which also makes the base's detail lines, regions, group and Linework).
- Drawings and IFC are tested through their output (pixels of the PNG, entity references in the STEP file), not through the
  structures that make them.

## Working in the big files

`commands.rs`, `model.rs`, `import.rs` and the two `tests.rs` files are thousands of lines: find what you need with a search
(`edit("name"`, `fn name`), read the part, and anchor a scripted edit on text that is unique in the file: a doc comment is often
repeated. A new area's tests go in their own file or module, not on the end of `tests.rs`.

## Shared geometry definitions

`Model::types`, `materials` and `families` use `shared::Shared`: ordinary reads and mutable map/type access work through
`Deref`/`DerefMut`; assigning a newly constructed plain value uses `.into()`. Cloning a model shares these definitions.
A retained allocation identity detaches on mutable access, so a derived geometry key can detect library-only edits,
including edits made directly to a mutable model. `Shared` serializes transparently; it introduces no file-format field.
`Shared::identity()` supplies a weak token that retains no definition data; it also detaches when a unique strong
owner is mutated. `SectionWork` checks elements, these weak identities and language before reuse, without holding
an `Arc<Model>` or whole old library maps. Cached level/model extents also check CAD bulk and point-cloud bounds
using names and numeric metadata, retaining none of their entities/points. Authoritative model snapshots still
share definition ownership.

The cache regression target is `crates/core/tests/spatial_cache.rs`. It covers direct type/family edits, shared snapshot
isolation, undo/redo branches and serialization compatibility. Persistent geometry fragments and their byte-budgeted
eviction remain separate work; a correct reusable-work key alone does not complete spatial-plan piece 6.

## Runtime revisions and conservative bounds

`Elements::revision_snapshot` retains only copy-on-write stamp columns, with no old elements or model ownership.
Compare snapshots across skipped edits, undo and branches with `changed_since`; mutable access conservatively
revises an element, while mutable iterators revise only yielded items. These stamps never enter serialized files
or semantic equality. `tests/revision_metadata.rs` covers the contract. Deletion's indexed production path is
compared with its retained full-scan implementation in unit tests; avoid mutable sweeps for read-only filtering.

`bounds::BoundsCache` offers lazy column/footing, straight-run and supported component bounds under a configurable
byte budget (16 MiB by default), including conservative tree-root/node charges. It checks the requested element,
its levels and used type dimensions without holding a model snapshot or old library data. Components also check
a weak family-map identity; narrowing this to individual family definitions is pending. `Family::conservative_bounds`
handles swept profiles and imported meshes without triangulation. Voids and revolved forms currently return unknown.
`Unknown` means a query must retain the object; only `Empty` means the id is absent. These conservative boxes must
not replace exact framing extrema. Hosted geometry, spatial discovery and renderer integration remain further work.
Tests compare bounds with fresh solid/family vertices, including mirrored and rotated instance placement.

`render::SectionCache` is an optional, bounded first fragment cache (128 MiB by default). It retains isolated
column/footing and run geometry with exact framing extrema, and shares geometry with completed `SectionWork`.
Run appearance is applied from the current snapshot, including material ordinals used for texture ids. Eviction
releases cache ownership while an active work object remains valid. `stats()` distinguishes primitive hits/builds,
uncached preparation, and all elements visited; assembly still scans the model, and unsupported categories still
use the full path. Desktop adoption and the complete spatial dependency contract are pending. The output target
is `tests/primitive_fragments.rs`, with fresh pixels, picking, faces, coverage and overlays as the oracle.

`SectionWork::new_deferred[_cancellable]` is a separate opt-in core path: supported components begin with form
envelopes, and fitting/datum queries resolve only extrema that can affect an exact endpoint. Unsupported families
keep full preparation. `new` remains the full oracle. `preparation_stats()` counts deferred/resolved extrema,
independently of visible mesh generation. `tests/component_preparation.rs` compares exact framing, datum ends,
pixels, picking, face names and coverage, including tower views. Lazy fitting, datum and extent queries are cancellable
(`section_fit_cancellable`, `SectionOverlay::new_cancellable`): cancelled, a component's extrema are whole or unresolved
and the model's extent stays unset, so the same work goes on to the exact answer; the tests sweep the checkpoints. The
section worker uses this path. It saves memory, not time: see the measurements in `docs/spatial-core-review.md`. The cells
lie beside `placed` by place, since framing asks of every element and a hash per element cost more than deferring saved.

A model that has not built its lookup answers `Model::levels` and similar by scanning every element, which makes
`SectionWork` quadratic (116 s against 2 s at 100 storeys, with the bare model from `sample::tower::drawn`). The `tower`
command's model, through a `Session`, was fast; other routes to a worker, such as `open`, have not been measured. When
measuring, go through a `Session` or call `m.lookup()` first.
