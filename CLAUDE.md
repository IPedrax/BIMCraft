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
- **Every element has an id and a GUID** (`ids.rs`), as Revit's ElementId and UniqueId: the id, six digits from `FIRST_ID`
  (100000) up, is what people, agents and schedules use, and may be given anew when an element comes in from another file
  (`insert`, `Elements::insert_as`); the GUID is made once and never changes. Anything that points at an element holds its
  `Guid`, never its `Id`: a new element kind lists its references in `visit_refs!` (`Element::refs_mut`), and new code finds
  elements by either (`Key`, `Model::same`, `Model::guid`). A command takes an element by id or GUID (`Guid` reads a number
  as the id it names, `Model::resolve` swaps it for the GUID after the command, and `Model::check` refuses one left over).
  A copy is a new element: a new id and a new GUID. IFC writes the GUID as the GlobalId and reads it back. The library has
  ids and GUIDs too (`library.rs`): each type, material, picture, family, parameter, view filter and template, phase, phase
  filter, option set and option is an `Entry` of `Model::library`, its id counting down from `LAST_ID` (999999) in the same
  id space; code names entries as it always has, and a file holds their GUIDs (`Model::to_json`, `names_from_file`). So a
  new name reference goes in `Element::lib_refs_mut` or `Model::lib_refs_mut`, a new kind of entry in `Kind` and
  `library_names` (and `copy_definition` for Insert from File), and a new rename calls `Library::renamed`;
  `Model::sync_library` (after every command) gives new entries theirs. Parts hold their own `id` and `guid` (`Layer`,
  `Form`, `Symbol`; the `HasPart` trait, listed in `Library::parts` by `parts_now`): an edit that gives a list again keeps
  them by place (`keep_parts`), the sync gives new ones (and a copy, which holds another's, its own; the one that held it
  before keeps it). A family's types and parameters are entries (`family: name`), so a component's type and instance values
  are GUIDs in a file; inside a family, its parts name its parameters as its formulas do. This is the standard for
  everything new: a new kind of part implements `HasPart` and goes in `parts_now`, `sync_parts` and `forget_parts`.
- **A file is read kindly, a command strictly.** The model's types keep `deny_unknown_fields`, so a misspelled key in a
  command's params is an error. `Model::read` leaves out of a file what this version doesn't know (`lenient.rs` walks it as
  the types would read it) and `open` lists it as `left_out`; a file of a newer `format` is refused. A change to what a file
  holds that an older version would misread (not just a field it can do without) bumps `FORMAT` and adds its step to
  `Model::migrate`; a new element category goes in `lenient::element` too (a test names any missing).
- **Validate in one place.** Commands build elements; `Model::check` validates the whole model after each command and the
  engine rolls back when it fails. New rules go there, with an error that says what to fix.
- **Clean-room.** Never copy Autodesk content: no families (`.rfa`), templates, icons, help text, or anything from inside
  `.rvt` files; a reader may read a user's file, it never bundles or extracts assets for the project. Formats come from
  public specifications (IFC4 is ISO 16739). A format with no public specification (Revit's `.rvt` and `.rfa`) may be read
  only under the fence in [docs/rvt-provenance.md](docs/rvt-provenance.md): the code is `crates/rvt`, Apache-2.0 only
  because it holds code ported from rvt-rs, `bimcraft-core` never depends on it, and every module and format finding
  there names its source. Assets are original or openly licensed; the lettering of drawings is drawn in code (`font.rs`), the one exception being the
  interface font, IBM Plex under the OFL (`assets/fonts`; `NOTICE` and each OFL.txt go in every package).
- **The core has no UI.** `bimcraft-core` must not depend on a UI toolkit, so front ends stay swappable.
- **The UI is thin.** `crates/ui-egui` reads `session.model` and changes it only through `Session::execute`, never by editing
  the model. Logic any front end needs (what a plan shows, what's under the cursor) belongs in the core: the canvas draws
  `render::draw` and picks with `Drawing::pick`. UI behaviour is tested with egui_kittest: clicks at model points
  (`BimApp::screen`), buttons by label, keys (`crates/ui-egui/src/tests.rs`).
- **Only the desktop app phones home, and only through `crates/update`.** The core, the CLI and the MCP server don't depend on it
  (a test reads their manifests), and a `BimApp` starts with `updates: None`, so no UI test reaches the network; `apps/bimcraft`'s
  `main` attaches the updater and starts the check on a thread. What comes from the network is hostile: the reply and the download
  are capped, the installer's address is built from the release's tag and never read from the reply, an asset is taken only if it is
  named exactly for the version and the kind of install (`Target::asset_name`), uploaded, with a `sha256:` digest, and nothing runs that
  failed a check. A new
  check is a pure function with a test, and the network stays behind `Net` (`testing::Fake` in tests; `net::Ureq` is tested against
  a server on this machine, never GitHub).
- **Test first.** Write the failing test, watch it fail, then write the code. Drawings and IFC are tested through their output:
  pixels of the PNG, entity references in the STEP file.
- **Keep the agent docs layered.** This file stays short: the rules, the checks, the conventions and a map of the modules. A
  feature's detail lives in `docs/areas/<area>.md` or the nearest crate's `CLAUDE.md`, never here (see the end of this file).

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
[docs/areas/revit-import.md](docs/areas/revit-import.md); for the page, [docs/areas/web.md](docs/areas/web.md); for the packages, [docs/areas/packaging.md](docs/areas/packaging.md).

## Conventions

Millimetres; x east, y north, z up. A project's units (`Model::units`, `units.rs`) change only what's shown and typed:
drawings, schedules and the desktop format lengths with `Units::length` and areas with `Units::area`, and read typed
lengths with `Units::parse`; files, JSON and every command stay in millimetres. A project's language (`Model::language`,
`language.rs`) is what its drawings print and what new names come in: the core passes the English word through
`Language::word`. The desktop's own words go through `lang::t` and `lang::tf`, so a new label needs a row in `WORDS` in
`crates/ui-egui/src/lang.rs`, with its Portuguese and its Spanish (and its Chinese in `lang/zh.rs`, which may wait: the window shows English
until it has one; the core's words are rows in `language.rs`);
`nothing_a_translated_interface_shows_is_left_in_english` catches a label the table knows that the window forgot to
translate. The core's messages stay English, because agents read them.
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
- `ifc.rs`, `import.rs`, `step.rs`: IFC export, IFC import, the STEP files they use. `lenient.rs`: what a file holds that this version doesn't know, found so it can be left out. `cloud.rs`: point clouds read from PTS, XYZ, PLY and LAS, and E57 (`cloud/e57.rs`) ([point clouds](docs/areas/point-clouds.md)). `insert.rs`, `inquiry.rs`: Insert from File, Select by ID and IDs of Selection. `autosave.rs`: background snapshots. `disk.rs`: the files commands read and write, the computer's or, in a browser, the page's ([the browser build](docs/areas/web.md)). `units.rs`, `language.rs`: units and a project's language. `sample.rs`, `sample/highrise.rs`: the sample house and the benchmark tower. `component_geometry.rs`: family geometry reused within an operation.
- `tests.rs`, `mep_tests.rs`, `structure_tests.rs`, `autosave_tests.rs`: the engine driven through commands, as a front end drives it.

`crates/ui-egui` (`bimcraft-ui-egui`, a thin shell over the core; notes: [crates/ui-egui/CLAUDE.md](crates/ui-egui/CLAUDE.md); area: [the desktop](docs/areas/desktop.md)):

- `lib.rs`: the app: ribbon, menus, tools, tabs, files. `canvas.rs`: the plan canvas (picks, tools, grips). `orbit.rs`: the 3D view; `gpu.rs` and `gpu.wgsl`: drawing it on the graphics card. `section.rs`, `paper.rs`: sections and sheets. `properties.rs`: the Properties palette. `browser.rs`: the Project Browser. `dock.rs`: docking palettes and floating views. `settings.rs`: File > Settings. `lineweights.rs`, `objectstyles.rs`, `linestyles.rs`, `fillpatterns.rs`: Manage > Line Weights, Object Styles, Line Styles and Fill Patterns. `sketch.rs`: sketch mode (Edit Boundary, Finish and Cancel). `draw.rs`: the Draw panel and its Options Bar. `groups.rs`: Create Group, Place Detail Group, Repeating Detail, Ungroup. `start.rs`: the start page. `context.rs`: the right-click menu. `dialog.rs`: the file dialogs, on a thread of their own. `window.rs`: the window's size, kept and fitted to its screen. `control.rs`: the control port, agents working on the window's project.
- `graphics.rs`, `footer.rs`, `phasing.rs`: Visibility/Graphics, the view control bar, Phasing. `inquiry.rs`, `recent.rs`, `autosave.rs`, `updates.rs`, `revit.rs`: Manage > Inquiry, Open Recent, autosave and recovery, the update banner, Revit files.
- `links.rs`: Link Model. `clouds.rs`: Insert > Point Cloud. `section_box.rs`: the 3D view's section box, its Properties and grips. `structure.rs`, `mep.rs`: the Structure tab and the discipline tabs. `editor.rs`, `favorites.rs`: the Family Editor and Favorites. `style.rs`: look and feel, light and dark, floating cards or compact. `ribbon.rs`: the ribbon's panels, their lists, and folding them to fit. `titlebar.rs`: the window's own title bar and frame. `icons.rs`: the icons, drawn in code (`--example icon_catalog` exports them). `lang.rs`: every label's Portuguese and Spanish (`WORDS`), `lang/zh.rs` its Chinese. `tests.rs`, `tests/`: the app driven as a user does. `web/`: in a browser, the file chooser and downloads (`web/dialog.rs`) and no updater (`web/updates.rs`), in place of `dialog.rs` and `updates.rs`.

`crates/update` (the updater, the only code that phones home; notes: [crates/update/CLAUDE.md](crates/update/CLAUDE.md); area: [updates](docs/areas/updates.md)): `updater.rs` the state the banner draws; `net.rs` HTTPS to GitHub; `detect.rs` which install this is; `install.rs` Windows, `appimage.rs` Linux, `macos.rs` macOS; `exec.rs`, `prefs.rs`, `testing.rs`.

`crates/rvt` (the Revit file reader, Apache-2.0 only; notes: [crates/rvt/CLAUDE.md](crates/rvt/CLAUDE.md); area: [Revit files](docs/areas/revit-import.md)): one module for each part of a file (`container`, `gzip`, `records`, `schema`, `inventory`) and for each element kind it imports (`walls`, `floors`, `roofs`, `columns`, `openings`, `rooms`), joined by `import.rs`.

`apps/bimcraft` (the desktop: `main.rs`, `startup.rs`: the log and the graphics fallback), `apps/bimcraft-web` (the same window in a browser page, made by Trunk: [the browser build](docs/areas/web.md)) and `apps/bimcraft-cli` (`main.rs`; `mcp.rs` the MCP server; `window.rs`: `mcp --connect`; `revit.rs`); notes: [apps/CLAUDE.md](apps/CLAUDE.md). More: `packaging/` (installers and packages: [packaging](docs/areas/packaging.md)), `docs/` (design notes), `.github/workflows/` (CI on every pull request, and the release).

## Keep the agent docs layered

This file holds the rules, the checks, the conventions and the map, and stays under 3,000 words (the `agent_docs` test in `crates/core` fails past that). The detail of a feature goes in `docs/areas/<area>.md`, or in the nearest crate's `CLAUDE.md` when it concerns the whole crate, and the map names it. A new feature adds its paragraph there, never here; a new module adds a line to the map; an area's write-up moves with the code it describes. Move text, never delete what is still true.
