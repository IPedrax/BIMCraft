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
  `.rvt` files; a reader may read a user's file, it never bundles or extracts assets for the project. Formats come from
  public specifications (IFC4 is ISO 16739). A format with no public specification (Revit's `.rvt` and `.rfa`) may be read
  only under the fence in [docs/rvt-provenance.md](docs/rvt-provenance.md): the code is `crates/rvt`, Apache-2.0 only
  because it holds code ported from rvt-rs, `bimcraft-core` never depends on it, and every module and format finding
  there names its source. Assets are original or openly licensed; the lettering of drawings is drawn in code (`font.rs`), the one exception being
  IBM Plex under the OFL, the interface's font and one text types letter notes in (`assets/fonts`, read by `text.rs`;
  `NOTICE` and each OFL.txt go in every package); a text type may also name a font the user's system has, read where it is
  and never bundled or copied into a project.
- **The core has no UI.** `bimcraft-core` must not depend on a UI toolkit, so front ends stay swappable.
- **The UI is thin.** `crates/ui-egui` reads `session.model` and changes it only through `Session::execute`, never by editing
  the model. Logic any front end needs (what a plan shows, what's under the cursor) belongs in the core: the canvas draws
  `render::draw` and picks with `Drawing::pick`. UI behaviour is tested with egui_kittest: clicks at model points
  (`BimApp::screen`), buttons by label, keys (`crates/ui-egui/src/tests.rs`).
- **Only the desktop app phones home, and only through `crates/update`**; what comes from the network is hostile. A `BimApp`
  starts with `updates: None`, so no test reaches the network. The checks are in
  [crates/update/CLAUDE.md](crates/update/CLAUDE.md).
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
[docs/areas/revit-import.md](docs/areas/revit-import.md); for the packages, [docs/areas/packaging.md](docs/areas/packaging.md).

## Conventions

Millimetres; x east, y north, z up. A project's units (`Model::units`, `units.rs`) change only what's shown and typed:
drawings, schedules and the desktop format lengths with `Units::length` and areas with `Units::area`, and read typed
lengths with `Units::parse`; files, JSON and every command stay in millimetres. A project's language (`Model::language`,
`language.rs`) is what its drawings print and what new names come in: the core passes the English word through
`Language::word`. The desktop's words go through `lang::t`, `lang::tf` and `lang::tc`. Translations live in
`translations/project/*.po` and `translations/ui/*.po`, compiled into the app with English fallback. Mark indirectly
selected English messages with `bimcraft_i18n::msg!` (`ui_msg!` for core-owned interface labels), and run `cargo i18n update`;
see [translations](docs/areas/translations.md). The core's messages stay English, because agents read them.
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
- `walls.rs` (`walls/index.rs`): wall axes, faces, mitred caps, outlines. `skeleton.rs`: the straight skeleton (roofs by footprint). `stairs.rs`: stairs, ramps, railings. `rebar.rs`: reinforcing bars, their lengths, weights and tubes. `reinforce.rs`: bars by rules for a beam, a column, a footing and a floor (`reinforce/norm.rs`: NBR 6118's usual limits, as notes). `runs.rs`: beams, pipes, conduits and the other straight runs. `geom.rs`: plan geometry. `align.rs`: pickable alignment edges and the motion between lines. `alignment.rs`: the temporary guides a point being drawn lines up with (smart snapping). `standards.rs`: technical standards (NBR, ISO, ANSI, EU, JIS): line weights, sheets, object styles; their numbers are the contributor's and unchecked. `snap.rs`: where a picked point lands. `sketch.rs`: sketches, lines solved into outlines and holes for floors, ceilings, roofs and shafts. `curves.rs`: the shapes a line takes (polylines, circles, ellipses, splines), split and joined. `draw.rs`: the Draw panel's shapes, what clicks make. Areas: [levels, walls and roofs](docs/areas/levels-walls-roofs.md), [structure and MEP](docs/areas/structure-and-mep.md). `triangulation.rs`: constrained Delaunay triangulation in plan, exact on a 0.01 mm grid (shaped floors' tops: [shape editing](docs/shape-editing.md)).
- `background_grid.rs`: view-owned canvas lattices, rotated drawing and grid snapping ([the desktop](docs/areas/desktop.md)).
- `workplane.rs`: what the 3D view draws on: its work plane (a level, a reference plane, a surface or three points, at any angle) and the face under the cursor. `triad.rs`: the 3D view's axis triad, its picks and turns. `hosting.rs`: the project's reference planes, components hosted on them or on faces (Place on Face), and where a component's family axes are ([families](docs/areas/families.md)).
- `render.rs`: plans (a display list drawn to SVG and PNG, and picking); `render/regions.rs`: region and model foreground compositing; `render/dimensions.rs`: dimensions, as their types say. `dims.rs`: dimension types, and how the Dimension tool lays one. `dims3d.rs`: dimensions in 3D views. `constraints.rs`: locked and equal dimensions kept after each command. `view3d.rs`: the 3D scene, its software rasterizer and picking; `view3d/gpu.rs`: its mesh and picking for the graphics card. `gltf.rs`: the scene as glTF 2.0 binary (.glb). `sheet.rs`, `pdf.rs`: sheets and their PDF. `font.rs`: our own lettering. `text.rs`: text types, their fonts (the system's, IBM Plex and our lettering), a note's formatting runs and its layout; `text/system.rs`: the system's fonts, listed from their headers. Area: [views and graphics](docs/areas/views-and-graphics.md).
- `graphics.rs`: Visibility/Graphics, view filters and templates (`Look`). `pen.rs`: line weights as pens and the project's Line Weights, widths on paper (paper space and model space: [views and graphics](docs/areas/views-and-graphics.md)). `styles.rs`: Object Styles, each model category's pens cut and seen. `lines.rs`: line patterns, line styles and detail lines. `fills.rs`: fill patterns, filled region types and filled regions; `fills/make.rs`: a pattern made of lines drawn; `pat.rs`: PAT files. `groups.rs`: detail groups and repeating details. `schedule.rs`: schedules ([schedules](docs/areas/schedules.md)); `xlsx.rs`: a one-sheet workbook they export to. `selection.rs`: Filter on a selection, by category or a parameter they share. `phasing.rs`: phases and design options ([phasing](docs/areas/phasing.md)). `massing.rs`: what an element made from a mass remembers ([masses](docs/areas/massing.md)).
- `link.rs`: linked models, the project with other projects' built elements drawn in and never edited ([views and graphics](docs/areas/views-and-graphics.md)).
- `dxf.rs`: DXF drawings read from the public reference. `cad.rs`: the drawings a project holds, and how an import colours, pens and dashes them ([CAD import](docs/areas/cad-import.md)).
- `family.rs`, `family/validate.rs`, `family/planes.rs` (reference planes and lines, forms' hosts), `family/options.rs` (Family Category and Parameters' options), `family/views.rs` (a family's plans and elevations), `family/sketch.rs` (forms' sketches), `family/constraints.rs` (its dimensions, labels, locks and EQ, solved as a type flexes), `family/nested.rs` (nested families and their instances, built into the host), `csg.rs`, `fixtures.rs`, `favorites.rs`, `shared_parameters.rs`: families, what's wrong with one and what's worth knowing about it (its formulas, its checks, its forms' boxes), solids cut from solids, the starter fixtures, the user's favorites. `texture.rs`, `paint.rs`: textures and paint. Area: [families](docs/areas/families.md).
- `adaptive/`: adaptive families: frames, forms, the evaluator, modelling ([adaptive families](docs/areas/adaptive-families.md)).
- `ifc.rs`, `import.rs`, `step.rs`: IFC export, IFC import, the STEP files they use; `import/shapes.rs`: turned and swept solids and advanced B-reps ([IFC import](docs/areas/ifc-import.md)); `ifc/classes.rs`: IFC4's element classes, which a generic family from IFC goes back out as ([families](docs/areas/families.md)). `lenient.rs`: what a file holds that this version doesn't know, found so it can be left out. `cloud.rs`: point clouds read from PTS, XYZ, PLY and LAS, and E57 (`cloud/e57.rs`) ([point clouds](docs/areas/point-clouds.md)). `insert.rs`, `inquiry.rs`: Insert from File, Select by ID and IDs of Selection. `autosave.rs`: background snapshots. `units.rs`, `language.rs`: units and a project's language. `sample.rs`, `sample/highrise.rs`, `sample/tower.rs`: the sample house and the benchmark towers. `component_geometry.rs`: family geometry reused within an operation.
- The spatial core ([plan](docs/spatial-core-plan.md); each module in [crates/core/CLAUDE.md](crates/core/CLAUDE.md)): `derived.rs`, `rooms.rs`, `shared.rs`, `bounds.rs`, `family/bounds.rs`, `ids/revisions.rs`, `view3d/update.rs`, `view3d/fragments.rs`, `oracle_tests.rs`, `crosscheck.rs`.
- `symbol_set.rs`: symbol sets, families of plan symbols loaded on demand by the `symbol_set` command (the AREI/RGIE's, `data/arei-symbols.json`; [families](docs/areas/families.md)).
- `hydrant.rs`: the `hydrant_size` command, hydrants and hose reels by IT 22/2025 (Tabelas 1 to 3, Hazen-Williams).
- `loads.rs`: the `load_points` command, a dwelling's lighting and outlets by room by NBR 5410 item 9.5.2.
- `dr.rs`: the `dr_check` command, which circuits NBR 5410 asks a 30 mA residual-current device for.
- `demand.rs`: the `demand` command, an installation's demand and service category by a utility's own standard (CPFL's GED 13; Enel's ET 1569 by installed load).
- `dps.rs`: the `dps_check` command, surge protection of a power line by NBR 5410 (when, where, connection, currents).
- `water.rs`, `reservoir.rs`: `water_size`, cold water by NBR 5626, and `reservoir_size`, its reservation.
- `storm.rs`: the `storm_size` command, storm drains by NBR 10844 (Manning, the downpipe abacuses).
- `drainage.rs`: the `drain_size` and `vent_size` commands, sanitary drains and vents by NBR 8160's tables.
- `conduit.rs`: the `conduit_fill` command, a conduit's fill by NBR 5410 (makers' catalogues in `conduit/tables.rs`).
- `electrical.rs`: the electrical settings, the project's voltages, distribution systems and wire types (the `voltage`, `distribution_system` and `wire_type` commands; starter set in `data/electrical.json`; [structure and MEP](docs/areas/structure-and-mep.md)).
- `nbr5410.rs`: the `circuit` command's sizing of a circuit by NBR 5410 (tables in `nbr5410/tables.rs`; [structure and MEP](docs/areas/structure-and-mep.md)).
- `tests.rs`, `mep_tests.rs`, `structure_tests.rs`, `eberick_tests.rs`, `reinforce_tests.rs`, `autosave_tests.rs`: the engine driven through commands, as a front end drives it.

`crates/ui-egui` (`bimcraft-ui-egui`, a thin shell over the core; notes: [crates/ui-egui/CLAUDE.md](crates/ui-egui/CLAUDE.md); area: [the desktop](docs/areas/desktop.md)):

- `lib.rs`: the app: ribbon, menus, tools, tabs, files. `canvas.rs`: the plan canvas (picks, tools, grips); `retained.rs`: its triangles kept on the card. `orbit.rs`: the 3D view; `gpu.rs` and `gpu.wgsl`: drawing it on the graphics card. `section.rs`, `paper.rs`: sections and sheets; `section_worker.rs`, `view_worker.rs`: views made off the interface's thread. `properties.rs`: the Properties palette. `browser.rs`: the Project Browser. `dock.rs`: docking palettes and floating views. `settings.rs`: File > Settings. `lineweights.rs`, `objectstyles.rs`, `linestyles.rs`, `fillpatterns.rs`: Manage > Line Weights, Object Styles, Line Styles and Fill Patterns. `sketch.rs`: sketch mode (Edit Boundary, Finish and Cancel); `sketch3d.rs`: the sketch in the 3D view, on its element's plane. `standards.rs`: Manage > Technical Standards. `shape.rs`: Shape Editing's actions, from a plan or the 3D view. `electrical.rs`: Manage > Electrical Settings. `notes.rs`: text notes written on the canvas, and Text Type Properties. `dimtypes.rs`: Dimension Type Properties. `typeprops.rs`: the look every Type Properties dialog shares. `draw.rs`: the Draw panel and its Options Bar. `groups.rs`: Create Group, Place Detail Group, Repeating Detail, Ungroup. `start.rs`: the start page. `context.rs`: the right-click menu. `find.rs`: Find (Ctrl+K), every tool and view by what is typed. `presets.rs`: the project's scale, the tools left out (Edit Tools) and named presets of the layout. `dialog.rs`: the file dialogs, on a thread of their own. `popup.rs`: movable dialog frames, scrolling forms and fixed bottom actions. `window.rs`: the window's size, kept and fitted to its screen. `control.rs`: the control port, agents working on the window's project.
- `background_grid.rs`: the view/template grid editor and canvas layer.
- `minimap.rs`: the Navigator's minimap. `display.rs`: Graphic Display Options (GD), the 3D view's edges and ambient shadows.
- `graphics.rs`, `templates.rs`, `footer.rs`, `phasing.rs`: Visibility/Graphics, View Templates (and Assign View Template), the view control bar, Phasing. `filter.rs`: Filter on the selection. `inquiry.rs`, `recent.rs`, `autosave.rs`, `updates.rs`, `revit.rs`: Manage > Inquiry, Open Recent, autosave and recovery, the update banner, Revit files.
- `links.rs`: Link Model. `clouds.rs`: Insert > Point Cloud. `cad.rs`: Insert > Import CAD and Link CAD. `section_box.rs`: the 3D view's section box, its Properties and grips, and Linked 3D View. `triad.rs`: the 3D view's navigation triad. `camera.rs`: a 3D view's camera in Properties, bound to the view the 3D tab shows or one picked on a plan (perspective, lens, eye, and a camera view's name, eye and target). `structure.rs`, `mep.rs`: the Structure tab and the discipline tabs. `reinforce.rs`: the Reinforce dialog. `editor.rs`, `editor/document.rs`, `editor/association.rs`, `editor/extrusion.rs`, `editor/exists.rs`, `editor/types_dialog.rs`, `editor/temp_hide.rs`, `editor/views.rs`, `editor/sketch.rs`, `editor/dimensions.rs`, `editor/work_plane.rs`, `editor/options.rs`, `editor/nested.rs`, `favorites.rs`: family drafts, the family document every family opens as, parameter associations, a selected form's Properties, Family Already Exists, Family Types, the document's Temporary Hide/Isolate, its plans and elevations, sketch mode, their dimensions and Align, its Work Plane dialog and Go To View, the family's options (Family Category and Parameters, Properties' Other), its nested families (Load Family, Component, Edit Family) and Favorites; `adaptive_*.rs`: adaptive families. `style.rs`: look and feel, the BIMCraft and Rev profiles, light and dark, theme presets and the user's accent, floating cards or compact. `ribbon.rs`: the ribbon's panels, their lists, and folding them to fit. `titlebar.rs`: the window's own title bar and frame. `icons.rs`: the icons, drawn in code (`--example icon_catalog` exports them). `lang.rs`: language selection and the interface PO catalogs; `lang/font.rs`: Chinese and Korean font support. `tests.rs`, `tests/`: the app driven as a user does.

`crates/i18n` (`bimcraft-i18n`; [notes](crates/i18n/CLAUDE.md)): embedded lookup, build-time PO compilation and the
`cargo i18n` extraction, merge and validation tools. Area: [translations](docs/areas/translations.md).

`crates/update` (the updater, the only code that phones home; notes: [crates/update/CLAUDE.md](crates/update/CLAUDE.md); area: [updates](docs/areas/updates.md)): `updater.rs` the state the banner draws; `net.rs` HTTPS to GitHub; `detect.rs` which install this is; `install.rs` Windows, `appimage.rs` Linux, `macos.rs` macOS; `exec.rs`, `prefs.rs`, `testing.rs`.

`crates/dwg` (`bimcraft-dwg`, the DWG bridge, which the core never depends on; notes: [crates/dwg/CLAUDE.md](crates/dwg/CLAUDE.md); area: [CAD import](docs/areas/cad-import.md)): a DWG as the DXF the core reads, through acadrust (MPL-2.0, unmodified), in a child process of the app (`in_child`, `child_main`); the apps hand a `Session` its `reader`.

`crates/rvt` (the Revit file reader, Apache-2.0 only; notes: [crates/rvt/CLAUDE.md](crates/rvt/CLAUDE.md); area: [Revit files](docs/areas/revit-import.md)): one module for each part of a file (`container`, `gzip`, `records`, `schema`, `inventory`) and for each element kind it imports (`walls`, `floors`, `roofs`, `columns`, `openings`, `rooms`, `pipes`, `ducts`, `framing`, `generic`), joined by `import.rs`.

`apps/bimcraft` (the desktop: `main.rs`, `startup.rs`: the log and the graphics fallback) and `apps/bimcraft-cli` (`main.rs`; `mcp.rs` the MCP server; `window.rs`: `mcp --connect`; `revit.rs`); notes: [apps/CLAUDE.md](apps/CLAUDE.md). More: `packaging/` (installers and packages: [packaging](docs/areas/packaging.md)), `docs/` (design notes), `.github/workflows/` (CI on every pull request, and the release).

## Keep the agent docs layered

This file holds the rules, the checks, the conventions and the map, and stays under 3,000 words (the `agent_docs` test in `crates/core` fails past that). The detail of a feature goes in `docs/areas/<area>.md`, or in the nearest crate's `CLAUDE.md` when it concerns the whole crate, and the map names it. A new feature adds its paragraph there, never here; a new module adds a line to the map; an area's write-up moves with the code it describes. Move text, never delete what is still true.
