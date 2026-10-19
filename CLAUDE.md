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
- **Validate in one place.** Commands build elements; `Model::check` validates the whole model after each command and the
  engine rolls back when it fails. New rules go there, with an error that says what to fix.
- **Clean-room.** Never copy Autodesk content: no families (`.rfa`), templates, icons, help text, or anything from inside
  `.rvt` files. Formats come from public specifications (IFC4 is ISO 16739). Assets are original or openly licensed; the
  lettering is drawn in code (`font.rs`).
- **The core has no UI.** `bimcraft-core` must not depend on a UI toolkit, so front ends stay swappable.
- **The UI is thin.** `crates/ui-egui` reads `session.model` and changes it only through `Session::execute`, never by editing
  the model. Logic any front end needs (what a plan shows, what's under the cursor) belongs in the core: the canvas draws
  `render::draw` and picks with `Drawing::pick`. UI behaviour is tested with egui_kittest: clicks at model points
  (`BimApp::screen`), buttons by label, keys (`crates/ui-egui/src/tests.rs`).
- **Test first.** Write the failing test, watch it fail, then write the code. Drawings and IFC are tested through their output:
  pixels of the PNG, entity references in the STEP file.

## Before you finish

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Looking at the result

- Desktop: `cargo run -p bimcraft -- --sample` (it needs a display; egui 0.36 needs Rust 1.95). After a UI change, look at a
  screenshot of the window, in both themes (View > Dark Mode). Without a display, egui_kittest's `wgpu` feature renders a `Harness` to an image; with no GPU,
  `WGPU_BACKEND=gl` falls back to Mesa's software renderer.
- Plans: `bimcraft-cli run --sample --save /tmp/plan.png`, then read the PNG. After any drawing change, look at it. A level's
  reflected ceiling plan: `render {"view": "Level 1", "ceiling": true, "path": "/tmp/rcp.png"}`.
- 3D: `bimcraft-cli run --sample --cmd 'render {"view": "{3D}", "path": "/tmp/3d.png"}'` (`azimuth` and `elevation`
  turn it). After touching `view3d.rs`, look at it. Sections and elevations render by name the same way: `"view": "Section 1"`.
- Schedules: `bimcraft-cli run --sample --cmd 'schedule {"category": "rooms"}'` returns its columns, rows and totals (and
  `available`, the fields there are); one placed on a sheet prints as a table: render the sheet and look at it.
- Sheets: `bimcraft-cli run --sample --save /tmp/sheets.pdf` prints every sheet; read them back with poppler (`pdfinfo`,
  `pdftoppm -png`) when it's installed. `render {"view": "A101"}` draws one sheet as a PNG.
- IFC: `bimcraft-cli run --sample --save /tmp/house.ifc`. After touching `ifc.rs`, validate with IfcOpenShell:
  `ifcopenshell.validate.validate(f, logger, express_rules=True)` (needs pytest installed), then build every product's shape
  with `ifcopenshell.geom.create_shape` and compare wall and roof volumes with a hand calculation.
- IFC import: after touching `import.rs` or `step.rs`, open the sample's IFC (`run house.ifc --cmd 'elements {}'`) and
  compare it with the sample, then the same file with its bodies turned into triangle meshes by IfcOpenShell, and a
  building written with IfcOpenShell's API. Render what came in and look at it. After touching how phases and design options
  cross (`ifc::PHASING`), do it with a phased sample too: `run --sample --script phased.txt --save p.ifc`, `convert p.ifc
  p.bimcraft`, and compare `phases`, `phase_filters`, `phase_overrides`, `option_sets` and each element's `params` with the project's.

- Packages: `packaging/linux/package.sh` writes the AppImage, deb, rpm and tarball into `dist/release` (build on Ubuntu
  22.04, as CI does, so they run on its glibc and later); `packaging/macos/package.sh` the DMG, on a Mac. After touching
  `packaging/`, run shellcheck on the scripts and the Linux one through, and install the deb in a clean container. The app
  id is `io.github.ipedrax.bimcraft` everywhere (`APP_ID`, the desktop file, AppStream, the bundle); the icons come from
  `packaging/icons.sh`.

## Conventions

Millimetres; x east, y north, z up. A project's units (`Model::units`, `units.rs`) change only what's shown and typed:
drawings, schedules and the desktop format lengths with `Units::length` and areas with `Units::area`, and read typed
lengths with `Units::parse`; files, JSON and every command stay in millimetres. A project's language (`Model::language`,
`language.rs`) is what its drawings print and what new names come in: the core passes the English word through
`Language::word`. The desktop's own words go through `lang::t` and `lang::tf`, so a new label needs a row in `WORDS` in
`crates/ui-egui/src/lang.rs`, with its Portuguese and its Spanish (the core's words are rows in `language.rs`);
`nothing_a_translated_interface_shows_is_left_in_english` catches a label the table knows that the window forgot to
translate. The core's messages stay English, because agents read them.
Levels have elevations, and a level is its plan: its view range (`Model::plan_range`)
cuts 1200 mm above it unless set, and its crop region bounds what the plan shows. Rooms find their walls at 1200 mm whatever
a plan cuts (`Model::bounding`). In a section or an elevation, points run along its line from its start, and up; on a sheet,
they're paper mm from its bottom left corner; notes and spot elevations live in those terms in their view.
A wall's `start` and `end` are on its location line: its centre line unless `location` says otherwise (`walls::faces`).
Its exterior is to the left of start to end, as in Revit, so walls drawn clockwise face out; a compound type lists its
layers from the exterior in, and a floor, roof or ceiling type from the top down (IFC import turns a wall round, or a slab's
layers over, by the way its layer set's usage stacks them). A layer's material is a name in the project's library
(`Model::materials`): its colour shades the layer in 3D, its cut pattern is a drafting pattern (`GAP` px apart on screen and
paper, where the layer is `PATTERN_ROOM` px wide), its surface pattern a model pattern (`MODULE` mm, its real size). A curved wall turns `arc` degrees, counter-clockwise positive (`walls::Axis`). Where walls meet,
their faces meet in mitred caps at any angle, and a wall ending on another stops at its face (`walls::cap`); plans, 3D,
sections and IFC all draw that outline (`walls::outline`, `walls::prisms`). Room areas are measured at the walls' centre
lines, run on or back to meet at joins (`walls::centre_line`). An attached top or base follows the roof's or floor's plane
over each part of the wall (a roof's slopes each over its own part of the plan); a mirrored wall runs back the other way.
A roof by footprint is the straight skeleton of its outline and its `courtyards` (`skeleton::faces`: the outline
counter-clockwise, the courtyards clockwise, the roof to the left of every edge): each edge moves in at speed 1, a gable's at
0, and its slope rises from it, reaching the roof's base `overhang` in from its eaves; a courtyard's edges number on after
the outline's, and every ring keeps its numbers whichever way round it runs, so `gables` stay put through a mirror.
Over the walls (`floor_outline`, which `roof_footprint` grows by the overhang), a loop of them standing free inside the outer
ones, joined to none of them and with no room in it, is a courtyard, open through the floor and the roof; one joined to them,
or with a room in it, is rooms, and both run over it.
`Model::roof_faces` are its slopes, each the loops round it in plan (a slope can close round a courtyard whose far side is a
gable) with the plane of its underside, trimmed where it `join`s another; `Model::roof_pieces` are those less their holes
and its `openings`, convex where cut. A floor's top is at its level plus `offset`,
or with a slope arrow, there at the arrow's tail and rising `slope` degrees toward its head (`Model::floor_top`); it's its
type's thickness across the slope, so plumb it's thicker. Its `openings` are holes inside it, apart, and `Model::floor_slabs`
what's left, convex where cut. A ceiling's `offset` is its underside; its `openings` are holes as a floor's
are, and `Model::ceiling_slabs` what's left. A shaft cuts each floor,
ceiling and roof its span reaches into over its outline, all the way through (`Model::less_shafts`). A stair or a ramp
is runs and landings laid out from its start (`stairs::stair`, `stairs::ramp`): a landing after `at` risers (a ramp's, `at`
mm up), as deep as the runs are wide, the next run turning by its `turn`; a railing on one follows its walk line moved out to
its side (`stairs::railing`), so it goes wherever its host goes. `modify` merges into fields that are objects (JSON Merge Patch), so `{"view_range": {"cut": 1500}}` keeps the rest; an element's own field can't be
called `category`, the tag every element carries.
A project parameter (`Model::parameters`) is a value each element of its categories may hold in its `params`, of its kind;
a component's `params` hold its family's instance values too, so a parameter for components can't take a family
parameter's name, nor any category's own field's (`schedule::own_field`). A schedule's fields are its category's own
(`OWN` in `schedule.rs`, read from `element_json`), the project parameters for it and, for components, the families'
parameters, or calculated: a formula over the row's others, read as the family formula engine reads (lengths in mm, areas in
m²), a field with no value leaving it with none (an unset yes or no is no). Filters compare numbers as the schedule shows
them, so two that show the same are equal. The six schedules every project has (`schedule::VIEWS`) aren't elements
until changed or placed (`schedule::standard`), and a schedule named as one of them, in any language or case, is that one; renaming a parameter renames it in schedules, but a formula naming it holds
the rename back.
A point picked in a plan lands where `snap::snap` says, the core's so every front end agrees: on the end, middle or centre of
what's drawn (`Scene::of`), where two straight lines (walls, grids; not arcs) cross, the foot of the perpendicular from the last point
to one, a guide through that point at
a multiple of `Snaps::angle` from east or from the line before it (where the guide crosses a wall or grid it lands there, and a
grid point lying on it beats a rounded length), a wall's centre line, else the nearest 10 mm. The desktop's `BimApp::snap` gives
it the reach in px (`SNAP`, `PICK`) and the snaps in force (Manage > Snaps, kept while the window is open, or for one point
a shortcut's override: `snap_once`, `Snaps::only`, `Snaps::none`); `found` and
`show_snap` draw what a pick would land on, and a point that moves with the cursor goes through them. A selected wall's ends are
grips (`grip_drag`): a drag runs `move` with `end`, Ctrl keeps the length; the angle beside each end runs `rotate` about it. A
wall joined at the end that moves keeps its joint, which slides along the wall it meets, so lengths change.
A click is made where the button was, not where the hand has got to. egui reads a click's position, and what lay under it, from the
pointer's last position in the frame, and counts two clicks in a frame as one, so a hand that moves on in the time between frames
(fast drawing) would click elsewhere, or once: `BimApp::pace` (run by `raw_input_hook`, with `show` asking for the frame that takes
the rest) cuts a window's events after each button press or release, and tests hand events over through `deliver`. egui also takes a
press and release more than six points apart for no click (a drag, if a frame falls between), which a hand clicking on the move
makes, so for every tool but Modify that is a click where the button went down (`BimApp::clicked_at`, where the plan, sections and
sheets take their clicks), and what lay under it is read there (`under_at`), not where the hand has got to. In Modify a drag is its
move and its box, so only a press let go within `QUICK` (0.2 s) is a click wherever the hand went: the drag it began is not
finished (`Canvas::quick`, `BimApp::finish_drag`: grips, crop regions, a sheet's views, the plan's move and box) and the selection
that drag's start replaced comes back (`BimApp::grab`) for the click to pick from. A slower press is the drag it is. In the 3D view,
where every drag orbits, a quick press is a click whatever the tool, and the camera the hand turned meanwhile goes back
(`Orbit::before`).
A family (`Model::families`, `family.rs`) is parameters, types that set them and forms in its own axes: x along a
component's angle, y to its left, z up from its base; a form is a profile in a sketch plane (`xy` swept up, `xz` along y,
`yz` along x), its sizes numbers or formulas, or triangles as a file brings them. A component places a type;
`Model::component_pieces` are its forms where it stands. Plans draw them from above, a form's edges hidden under a higher
form of the same component; sections show them beyond their line and never cut them (`Solid::Mesh` has no `across`); the
3D view takes a form's triangles that share an edge and turn under 20° there as one curved surface, so a curve draws no
edge between its facets (`view3d::facets`) and shades smoothly across them, each corner lit by the angle-weighted mean of
its own surface's normals there, so a crease keeps its step (`corner_normals`); IFC writes each form as a closed face set,
triangles turned out, and reads one family per family name the file gives.
A view's graphics (`graphics.rs`; `Level`, `Section` and `View3D` each hold `graphics`) hide and override by category, by view
filter (`Model::view_filters`: categories and a schedule filter's rules, `schedule::picks`) and by element: an element's own beats
the filters' (the first applied prevailing), which beat the category's, and hiding by any of them hides. `Look::of` resolves a
view to what it hides and how it paints the rest; the drawing code asks `look.shows(id)` wherever it loops over elements (plans,
sections, `cut_outlines`, `seen_below`, `view3d::Scene::picture`) and `look.dress` recolours the finished `Drawing` by owner and
by `Part` (`Drawing::part` marks what is drawn as projection, cut, lettering or hatching (and the lines between layers), so a
new drawing of what a view cuts sets `Part::Cut`; a hidden door's opening is drawn owned by none, the wall's; the cut walls' union fill has no owner and takes its colour at the draw site, `cut_unions`). A view template
(`Model::view_templates`) controls groups of the views that follow it (categories, filters, a plan's view range through
`Model::view_range_of`, a phase with its phase filter, design options), and they can't change those (`graphics` and `modify`
refuse). Snaps, Tag All, the desktop's Select All
Instances and the framing of sections and 3D views take what `Look` shows too. A level's ceiling plan is its plan's view;
transparency shows in 3D only.
Phases (`Model::phases`, in order) and design options (`Model::option_sets`, `phasing.rs`) are values an element holds among its
`params`, under names no project parameter or family parameter may take (`phasing::CREATED`, `DEMOLISHED`, `OPTION`; `Phase Created` is the first
phase when it has no value, `Design Option` is `set: option`; `open` renames an older file's project parameter of such a name), so `modify`, copies, schedules (`From::Param` in `schedule::OWN`)
and view filters reach them as any other. `phasing::status` says what an element is at a phase's place in the order (new,
existing, demolished, temporary, or not there); a door or window with none of its own follows its wall (`phasing::value`), and
annotation, which holds no params, belongs to a view. A view's `Graphics` hold `phase` (the project's last unless it says),
`phase_filter` (every status by category unless it says; `Model::phase_filters` show each status by category, overridden or
not at all, and `Model::phase_overrides` draw the overridden ones, never hidden or see-through) and `options` (`{set: option}`,
a set's primary unless it says); `Look::of` hides what a phase, a filter or an option leaves out and paints what a filter
overrides, below the view's own overrides. `Session::phase` and `Session::option` are what commands make (`Session::stamp`
gives each new element its `Phase Created` (none for the first phase, but a door or window is named, or it would follow its
wall) and its `Design Option` where it holds none; a copy takes the session's and loses its `Phase Demolished`; `split`, undo,
redo and the commands that replace the project stamp nothing); a refused command leaves them as they were; they
belong to the session, not the model, and the desktop sets them before each command (`BimApp::made_in`): the phase of the view
on screen when it has a phase or a phase filter of its own, and the active design option (`activate`, which shows it in the
view too). What was made in the first phase holds no value, so a phase put in before the first, or the first deleted, names it first
(`phasing::pin_first`; a door or window follows its wall, which is named). Deleting or combining a phase moves what was in it to
`into` (`phasing::move_phase`), and renaming a phase, an option or a set takes the elements, the views and the rules of filters
and schedules along; accepting a set keeps the primary's elements and deletes the rest.
IFC carries them in property sets named `ifc::PHASING`, `BIMCraft_Phasing`. An element that holds a phase or an option gets one:
`Phase Created` (always: the first phase is named too, with `ifc::IMPLIED` as its description where the element holds none, but a
door or window holds only what it says, or one with no phase would stop following its wall), `Phase Demolished` and `Design
Option`, as text. The project gets one when an element holds a phase or an option, or it changed its phases, filters or overrides,
or has an option set: `Phases`, a list of the names in order, and, where it changed them, `Phase Filters`, `Phase Overrides` and
`Design Option Sets` as the JSON a project file holds them in; a project that uses none of it writes none, and the file is as it
was. `import.rs` reads them beside the project parameters: what the project can't have (a phase or an option it hasn't, a
demolition before the creation, a list or a JSON that breaks `phasing::check_project`) is left out and listed, and the element
stays (it is the property, not the element, that breaks the rule); an implied `Phase Created` is no value; no other set is read, so
one another tool writes for phases, `Phasing` say, changes nothing.
A schedule sorts and groups `Phase Created` and `Phase Demolished` by the phase's place in `Model::phases`, not by its name, no
`Phase Demolished` coming after every phase (`schedule::sorts_as`); its filters still compare the names. The desktop's Demolish
tool (DW, `BimApp::demolish`, in the Modify tab's Phasing panel) runs `modify {params: {Phase Demolished}}` on the element clicked in a
plan, a section or the 3D view: the phase of the view on screen (the project's last unless it has one), or null when the element
already comes down in it, one undo step a click. A door or window that only follows its wall gets a note instead, and an element that
holds no `params` (annotation, levels, grids, shafts) says so and changes nothing.
