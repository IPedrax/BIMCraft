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
  The detail is below.
- **A change to what a file holds** that an older version would misread (not just a field it can do without) bumps `FORMAT` and
  adds its step to `Model::migrate`; a new element category goes in `lenient::element` too (a test names any missing).

## Ids and GUIDs

As Revit's ElementId and UniqueId. The id, six digits from `FIRST_ID` (100000) up, is what people, agents and schedules use, and
may be given anew when an element comes in from another file (`insert`, `Elements::insert_as`); the GUID is made once and never
changes. A copy is a new element: a new id and a new GUID. IFC writes the GUID as the GlobalId and reads it back.

- **Elements.** Anything that points at an element holds its `Guid`, never its `Id`: a new element kind lists its references in
  `visit_refs!` (`Element::refs_mut`), and new code finds elements by either (`Key`, `Model::same`, `Model::guid`). A command
  takes an element by id or GUID: `Guid` reads a number as the id it names, `Model::resolve` swaps it for the GUID after the
  command, and `Model::check` refuses one left over.
- **The library** (`library.rs`): each type, material, picture, family, parameter, view filter and template, phase, phase
  filter, option set and option is an `Entry` of `Model::library`, its id counting down from `LAST_ID` (999999) in the same id
  space. Code names entries as it always has; a file holds their GUIDs (`Model::to_json`, `names_from_file`). A new name
  reference goes in `Element::lib_refs_mut` or `Model::lib_refs_mut`; a new kind of entry in `Kind` and `library_names` (and
  `copy_definition` for Insert from File); a new rename calls `Library::renamed`. `Model::sync_library` (after every command)
  gives new entries theirs. A family's types and parameters are entries (`family: name`), so a component's type and instance
  values are GUIDs in a file; inside a family, its parts name its parameters as its formulas do.
- **Parts** hold their own `id` and `guid` (`Layer`, `Form`, `Symbol`; the `HasPart` trait, listed in `Library::parts` by
  `parts_now`). An edit that gives a list again keeps them by place (`keep_parts`); the sync gives new ones (and a copy, which
  holds another's, its own; the one that held it before keeps it). A new kind of part implements `HasPart` and goes in
  `parts_now`, `sync_parts` and `forget_parts`.

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
- That one, `hostile_params_never_panic_or_break_the_model` (`tests.rs`), sweeps every command with every hostile input and takes about seven
  minutes; its cost grows with the commands. Run it before you finish (`cargo test --workspace`); CI always does.
- Drawings and IFC are tested through their output (pixels of the PNG, entity references in the STEP file), not through the
  structures that make them.

## Working in the big files

`commands.rs`, `model.rs`, `import.rs` and the two `tests.rs` files are thousands of lines: find what you need with a search
(`edit("name"`, `fn name`), read the part, and anchor a scripted edit on text that is unique in the file: a doc comment is often
repeated. A new area's tests go in their own file or module, not on the end of `tests.rs`.
