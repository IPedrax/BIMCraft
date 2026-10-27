# BIMCraft: instructions for agents

BIMCraft is a clean-room, pure-Rust BIM application following the Revit workflow, modeled on
[storytold/cadcraft](https://github.com/storytold/cadcraft) (the AutoCAD one). Milestone 1, the headless engine with the CLI
and MCP server, and milestone 2, the egui desktop app, are done. What works and what's missing: [README.md](README.md).

## Rules

- **Never crash.** No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `unreachable!` outside tests (the workspace
  lints deny them), and no `unsafe`. Parameters, files and MCP input are hostile: check that numbers are finite and in range,
  and cap sizes (`MAX_POINTS`, `MAX_TEXT`, `MAX_COORD`, `MAX_FILE`). `Session::execute` runs every command under
  `catch_unwind` and restores the model on any failure; `hostile_params_never_panic_or_break_the_model` sweeps every command.
- **Everything is a command** (`ALL` in `crates/core/src/commands.rs`): an id after Revit's tool name, a Revit-style shortcut,
  a params doc written for agents, and a JSON `run`. The CLI, scripts and MCP all go through `Session::execute`, so new
  behaviour is a new command or parameter, with a test.
- **Every element has an id and a GUID** (`ids.rs`), as Revit's ElementId and UniqueId: the id (six digits from 100000) is for
  people and may change when an element comes in from another file; the GUID never changes, and anything that points at an
  element holds it. Library entries (`library.rs`: types, materials, families, phases, …; ids down from 999999) and parts
  (`HasPart`) have both too. Everything new follows this standard: what to register where is in
  [crates/core/CLAUDE.md](crates/core/CLAUDE.md#ids-and-guids).
- **A file is read kindly, a command strictly.** The model's types keep `deny_unknown_fields`, so a misspelled key in a
  command's params is an error. `Model::read` leaves out of a file what this version doesn't know (`lenient.rs`) and `open`
  lists it as `left_out`; a file of a newer `format` is refused. A change an older version would misread bumps `FORMAT` and
  adds a step to `Model::migrate`.
- **Validate in one place.** Commands build elements; `Model::check` validates the whole model after each command and the
  engine rolls back when it fails. New rules go there, with an error that says what to fix.
- **Clean-room.** Never copy Autodesk content: no families (`.rfa`), templates, icons, help text, or anything from inside
  `.rvt` files. Formats come from public specifications (IFC4 is ISO 16739); one with no public specification (`.rvt`, `.rfa`)
  is read only under the fence in [docs/rvt-provenance.md](docs/rvt-provenance.md), in `crates/rvt` (Apache-2.0, which
  `bimcraft-core` never depends on). Assets are original or openly licensed; the lettering of drawings is drawn in code
  (`font.rs`), and the interface font is IBM Plex under the OFL (`assets/fonts`; `NOTICE` and each OFL.txt go in every package).
- **The core has no UI.** `bimcraft-core` must not depend on a UI toolkit, so front ends stay swappable.
- **The UI is thin.** `crates/ui-egui` reads `session.model` and changes it only through `Session::execute`. Logic any front
  end needs belongs in the core: the canvas draws `render::draw` and picks with `Drawing::pick`. UI behaviour is tested with
  egui_kittest (`crates/ui-egui/src/tests.rs`).
- **Only the desktop app phones home, and only through `crates/update`**; what comes from the network is hostile. A `BimApp`
  starts with `updates: None`, so no test reaches the network. The checks are in
  [crates/update/CLAUDE.md](crates/update/CLAUDE.md).
- **Test first.** Write the failing test, watch it fail, then write the code. Drawings and IFC are tested through their output:
  pixels of the PNG, entity references in the STEP file.
- **Keep the agent docs layered.** This file stays short: the rules, the checks, the conventions and a map of the modules. A
  feature's detail lives in `docs/areas/<area>.md` or the nearest crate's `CLAUDE.md`, never here (see the end of this file).

## Working economically

Let the tools do the reading and checking; read only what you will change.

- **Find before you read.** Locate a definition with `rg -n "(fn|struct|enum|trait|type) NAME" crates apps`, outline a file with
  `rg -n "^\s*(pub.*)?(fn|struct|enum|trait|impl|mod) " FILE`, then read the line range around the hit. Never read a whole file
  over ~300 lines (`commands.rs`, `model.rs` and the `tests.rs` files are thousands). Don't open `target/` or `Cargo.lock`.
- **Keep output short.** `cargo check -q -p <crate> --message-format=short 2>&1 | head -40`, the same for clippy; fix from the
  top and rerun. Tests: `cargo test -q -p <crate> <filter> 2>&1 | tail -40`. Bound every command's output with `head`/`tail`.
- **Test narrowly, then once broadly**: the filtered crate while working, the checks below once at the end. Don't re-read a file
  you just edited; `cargo check` it. No `--release` builds unless asked.

## Before you finish

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```


## Looking at the result

After a change, look at what it makes, as the area says. How to do it for each kind of result is in
[docs/areas/looking-at-results.md](docs/areas/looking-at-results.md): the desktop in its three looks (a screenshot of the window,
rendered without a display), plans and ceiling plans, the 3D view, scale, detail level and style, schedules, sheets as PDF,
IFC with IfcOpenShell, structure and the discipline tabs, IFC import. For Revit files see
[docs/areas/revit-import.md](docs/areas/revit-import.md); for the packages, [docs/areas/packaging.md](docs/areas/packaging.md).

## Conventions

Millimetres; x east, y north, z up. A project's units (`Model::units`, `units.rs`) change only what's shown and typed:
drawings, schedules and the desktop format lengths with `Units::length` and areas with `Units::area`, and read typed
lengths with `Units::parse`; files, JSON and every command stay in millimetres. Words a person reads are translated:
`Language::word` (`language.rs`) for what a project's drawings print and the names it makes, `lang::t` and `lang::tf` with a row
in `WORDS` for the desktop (how: [crates/ui-egui/CLAUDE.md](crates/ui-egui/CLAUDE.md)). The core's messages stay English,
because agents read them.
The selection is the model's, not a view's: opening any view (a plan, a section, the 3D view, a sheet, a schedule) keeps it
(`BimApp::open_sheet`), each view shows it, and a schedule's rows (`schedule::Table::ids`, `elements` in the command's output)
highlight what's selected and pick what they list.
Every view has the right-click menu of `context.rs`: an item is an `Action`, offered by `context_items` and done by
`act`.

## Module map

One line per module: what it owns. Read the module's file and, for the areas it names, the write-up in `docs/areas/`.

`crates/core` (`bimcraft-core`; its own notes: [crates/core/CLAUDE.md](crates/core/CLAUDE.md)):

- `commands.rs`: every command (`ALL`): what the CLI, scripts and MCP run. `engine.rs`: `Session`, which runs them (rollback, undo). `bridge.rs`: the control port's protocol, which the window and `mcp --connect` share.
- `model.rs`: the model: elements, types, `Model::check` and the queries on it. `ids.rs`: element ids and GUIDs. `library.rs`: types, materials and families as entries with ids and GUIDs.
- `walls.rs` (`walls/index.rs`): wall axes, faces, mitred caps, outlines. `skeleton.rs`: the straight skeleton (roofs by footprint). `stairs.rs`: stairs, ramps, railings. `runs.rs`: beams, pipes, conduits and the other straight runs. `geom.rs`: plan geometry. `snap.rs`: where a picked point lands. `sketch.rs`: sketches, lines solved into outlines and holes for floors, ceilings, roofs and shafts. `curves.rs`: the shapes a line takes (polylines, circles, ellipses, splines), split and joined. `draw.rs`: the Draw panel's shapes, what clicks make. Areas: [levels, walls and roofs](docs/areas/levels-walls-roofs.md), [structure and MEP](docs/areas/structure-and-mep.md).
- `render.rs`: plans (a display list drawn to SVG and PNG, and picking); `render/regions.rs`: region and model foreground compositing. `view3d.rs`: the 3D scene, its software rasterizer and picking; `view3d/gpu.rs`: its mesh and picking for the graphics card. `sheet.rs`, `pdf.rs`: sheets and their PDF. `font.rs`: our own lettering. Area: [views and graphics](docs/areas/views-and-graphics.md).
- `graphics.rs`: Visibility/Graphics, view filters and templates (`Look`). `pen.rs`: line weights as pens and the project's Line Weights, widths on paper (paper space and model space: [views and graphics](docs/areas/views-and-graphics.md)). `styles.rs`: Object Styles, each model category's pens cut and seen. `lines.rs`: line patterns, line styles and detail lines. `fills.rs`: fill patterns, filled region types and filled regions; `pat.rs`: PAT files. `groups.rs`: detail groups and repeating details. `schedule.rs`: schedules ([schedules](docs/areas/schedules.md)). `phasing.rs`: phases and design options ([phasing](docs/areas/phasing.md)). `massing.rs`: what an element made from a mass remembers ([masses](docs/areas/massing.md)).
- `link.rs`: linked models, the project with other projects' built elements drawn in and never edited ([views and graphics](docs/areas/views-and-graphics.md)).
- `family.rs`, `csg.rs`, `fixtures.rs`, `favorites.rs`: families, solids cut from solids, the starter fixtures, the user's favorites. `texture.rs`, `paint.rs`: textures and paint. Area: [families](docs/areas/families.md).
- `ifc.rs`, `import.rs`, `step.rs`: IFC export, IFC import, the STEP files they use. `lenient.rs`: what a file holds that this version doesn't know, found so it can be left out. `cloud.rs`: point clouds read from PTS, XYZ, PLY and LAS, and E57 (`cloud/e57.rs`) ([point clouds](docs/areas/point-clouds.md)). `insert.rs`, `inquiry.rs`: Insert from File, Select by ID and IDs of Selection. `autosave.rs`: background snapshots. `units.rs`, `language.rs`: units and a project's language. `sample.rs`, `sample/highrise.rs`: the sample house and the benchmark tower. `component_geometry.rs`: family geometry reused within an operation.
- `tests.rs`, `mep_tests.rs`, `structure_tests.rs`, `autosave_tests.rs`: the engine driven through commands, as a front end drives it.

`crates/ui-egui` (`bimcraft-ui-egui`, a thin shell over the core; notes: [crates/ui-egui/CLAUDE.md](crates/ui-egui/CLAUDE.md); area: [the desktop](docs/areas/desktop.md)):

- `lib.rs`: the app: ribbon, menus, tools, tabs, files. `canvas.rs`: the plan canvas (picks, tools, grips). `orbit.rs`: the 3D view; `gpu.rs` and `gpu.wgsl`: drawing it on the graphics card. `section.rs`, `paper.rs`: sections and sheets. `properties.rs`: the Properties palette. `browser.rs`: the Project Browser. `dock.rs`: docking palettes and floating views. `settings.rs`: File > Settings. `lineweights.rs`, `objectstyles.rs`, `linestyles.rs`, `fillpatterns.rs`: Manage > Line Weights, Object Styles, Line Styles and Fill Patterns. `sketch.rs`: sketch mode (Edit Boundary, Finish and Cancel). `draw.rs`: the Draw panel and its Options Bar. `groups.rs`: Create Group, Place Detail Group, Repeating Detail, Ungroup. `start.rs`: the start page. `context.rs`: the right-click menu. `dialog.rs`: the file dialogs, on a thread of their own. `window.rs`: the window's size, kept and fitted to its screen. `control.rs`: the control port, agents working on the window's project.
- `graphics.rs`, `footer.rs`, `phasing.rs`: Visibility/Graphics, the view control bar, Phasing. `inquiry.rs`, `recent.rs`, `autosave.rs`, `updates.rs`, `revit.rs`: Manage > Inquiry, Open Recent, autosave and recovery, the update banner, Revit files.
- `links.rs`: Link Model. `clouds.rs`: Insert > Point Cloud. `structure.rs`, `mep.rs`: the Structure tab and the discipline tabs. `editor.rs`, `favorites.rs`: the Family Editor and Favorites. `style.rs`: look and feel, light and dark, floating cards or compact. `ribbon.rs`: the ribbon's panels, their lists, and folding them to fit. `titlebar.rs`: the window's own title bar and frame. `icons.rs`: the icons, drawn in code (`--example icon_catalog` exports them). `lang.rs`: every label's Portuguese and Spanish (`WORDS`), `lang/zh.rs` its Chinese. `tests.rs`, `tests/`: the app driven as a user does.

`crates/update` (the updater, the only code that phones home; notes: [crates/update/CLAUDE.md](crates/update/CLAUDE.md); area: [updates](docs/areas/updates.md)): `updater.rs` the state the banner draws; `net.rs` HTTPS to GitHub; `detect.rs` which install this is; `install.rs` Windows, `appimage.rs` Linux, `macos.rs` macOS; `exec.rs`, `prefs.rs`, `testing.rs`.

`crates/rvt` (the Revit file reader, Apache-2.0 only; notes: [crates/rvt/CLAUDE.md](crates/rvt/CLAUDE.md); area: [Revit files](docs/areas/revit-import.md)): one module for each part of a file (`container`, `gzip`, `records`, `schema`, `inventory`) and for each element kind it imports (`walls`, `floors`, `roofs`, `columns`, `openings`, `rooms`), joined by `import.rs`.

`apps/bimcraft` (the desktop: `main.rs`, `startup.rs`: the log and the graphics fallback) and `apps/bimcraft-cli` (`main.rs`; `mcp.rs` the MCP server; `window.rs`: `mcp --connect`; `revit.rs`); notes: [apps/CLAUDE.md](apps/CLAUDE.md). More: `packaging/` (installers and packages: [packaging](docs/areas/packaging.md)), `docs/` (design notes), `.github/workflows/` (CI on every pull request, and the release).

## Keep the agent docs layered

This file holds the rules, the checks, the conventions and the map, and stays under 3,000 words (the `agent_docs` test in `crates/core` fails past that). The detail of a feature goes in `docs/areas/<area>.md`, or in the nearest crate's `CLAUDE.md` when it concerns the whole crate, and the map names it. A new feature adds its paragraph there, never here; a new module adds a line to the map; an area's write-up moves with the code it describes. Move text, never delete what is still true.
