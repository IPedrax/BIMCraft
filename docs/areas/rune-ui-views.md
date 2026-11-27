# The Rune UI: the views other than a level's plan

Stage 2's browser and stage 5 of [the Rune UI](rune-ui.md), done on `IPedrax-Test/Rune-GUI-views`: the Project Browser lists every
view the model has, and the window shows the one opened. The egui app (`browser.rs`, `section.rs`, `orbit.rs`, `paper.rs`) is the
reference for behaviour and is never edited. The progress folds into `rune-ui.md` when the branches are merged.

## 1. The browser's views (`views.rs`, `app/open.rs`, `ui/browser.rs`, `ui/canvas.rs`)

**State.** `App::view` is a `ViewKind`: `Plan(key)` (a level, a plan view or a callout of a plan: whatever `render::draw` takes),
`Ceiling(key)`, `Section(id)` (a section, an elevation or a callout of one), `View3D(Option<id>)` (none is the default `{3D}`, which a
project makes when it is first changed), `Sheet(id)`, `Schedule(category)` (the seven every project has, `schedule::VIEWS`),
`Table(id)` (another schedule, a panel schedule, a reinforcement detail) and `Nothing` (a project with no level). `views.rs` holds no
toolkit: `exists`, `keep`, `level_of`, `draw` and `fit` (the plan kinds), `name` and `title`, and `tree`, which makes the browser's
groups from `Model` in the order egui lists them: Floor Plans, Ceiling Plans, 3D Views, Elevations, Sections, Callouts, Schedules,
Panel Schedules, Reinforcement Details, Sheets. The first three and the seven schedules are always there; the others only when the
project has something in them. Egui nests callouts under the view they are a fragment of; here a callout is one row named
`Parent: Callout`. Each level's own plan comes first, then its plan views, as egui's.

**Opening.** `App::open_view(kind)` refuses a view the model lacks or the one already shown, and otherwise drops what belongs to a
view (the pointer, the box, Zoom In Region, the points a tool had picked, Previous Pan/Zoom's history), sets the plan's level when the
view is a plan, frames it and draws it. **The selection stays**: it is the model's, not a view's, as egui's `open_sheet` has it. The
browser's press on a level used to clear the selection and no longer does (`set_level` is `open_view(Plan(level))`). After every
command `settle` keeps the open view while the model has it, and falls back to the first level's plan when it went (a deleted plan
view); undo gives the row back to the browser, not the view to the screen.

**The browser.** One row a view, grouped, a press on a group's name folds it (`Views::toggle`, `expanded` for a screen reader), a press
on a row opens the view, the one on screen is lit. The list is one `memo` keyed by the view shown and `Views::rev`, which counts
the changes of the tree, the folds and the title, so the pointer moving over the window builds nothing. Rows are keyed `level-<id>`
(a level's floor plan, as before), `plan-`, `ceiling-`, `section-`, `view3d-`, `sheet-`, `schedule-<category>` and `table-<id>`.
The status bar says the view's title: a plan by its name (as before), every other kind by its kind and name (`Section: Section 1`).

**Canvas.** `ui/canvas.rs` picks the canvas of the view shown, one for each kind. Plans (and a project with no level) keep the plan
canvas exactly as it was (`ui/plan.rs` is untouched); the others are the sections below. The piece first shipped a card that only named
the kinds with no canvas yet, and each later piece replaced one: none is left.

**Not done in this piece, from egui's browser:** the tabs (All, Views, Schedules, Sheets, Families), the search with its wildcards,
the Families and Linked Models groups, the right-click menus (Duplicate, Delete, Show as Underlay, Create Print Set), a Ctrl or Shift
pick of several views, and the minimap. The right-click menu is the shared `context.rs` item and the Families tree belongs with the
Type Selector, so neither is in this session's area.

## 2. Sections and elevations (`section.rs`, `section/render.rs`, `worker.rs`, `app/section.rs`, `ui/section.rs`)

**On a thread of its own, not the interface's.** The core's picture of a section is a software render of everything the view sees,
behind a `SectionWork` of the whole model (what egui's `section_worker.rs` keeps): seconds on the tower, the very cost that file's
header gives for running it off the interface's thread. So `Cut::ask` hands the newest request to `worker::Latest`, a thread of its own
for the section on screen (started by the first ask, stopped when another kind of view is shown, by `Drop`): one request is made at a time,
a newer one cancels the job being made (the core's `*_cancellable` calls ask `cancelled` between chunks) and replaces any that wait, so
only the newest is answered, and a job that panics is an error and not a crash (`worker/tests.rs`). The window needs no handle on the
thread: `ui::poll` takes in the answer every frame (`App::poll_views`) and asks for a frame 16 ms on while `views_busy`. The renderer
(`section/render.rs`) is egui's, made for one picture a point to a pixel: it keeps the model's work, and the picture of the last view, so
a request that only changes what is selected tints the picture again (`Image::retint_section`) and draws the overlay, with no geometry.
Both copies, egui's and this, move to the core when egui adopts them (extraction 5 of `rune-ui.md`).

**The view.** `Cut` holds a `Camera` (the core's `View`: along the section's line, and up; `camera.rs` pans, zooms and zooms to a
region as it does for a plan), the last `Frame` (the picture, the drawing over it, the request it answers) and the picture as a `Pixels`.
A wheel notch or a pan moves the last picture at once through Rune's live offset and scale, as the plan does under `CameraMode::Live`
(`Cut::glide`; a section is always `Live`, since its exact picture takes a thread to make), and asks the thread for the exact one; picking maps the pointer back into the picture on screen (`drawn_at`). The first
request has no view and is fitted by the core (`section_fit_cancellable`); the camera takes the fit when the picture comes. The canvas
is the core's picture (one image, sized to the canvas, a pixel to a point) under the drawing as Rune shapes (`paint::base`, its ink as it
is on paper: the picture is on the core's paper, not on the dark plan), the selected and hovered elements lit over it (`paint::lit`), and
the box being dragged; the picture and the shapes are in one moved layer.

**Picking.** A click or a hover takes what the drawing has under it (a level, a grid, a note: `Drawing::pick`), else the element the
picture shows there (`Image::pick`, which knows each pixel's element); a box takes both (`Drawing::in_box`, `Image::in_box`): a window
left to right takes what it holds, a crossing right to left what it touches, as in a plan. The selection and the hovered element are
the model's, so a click in a section selects in the plan too (`App::section_release`). What is selected is tinted in the picture
through the request's tints (the plan's accent), which costs the thread a retint and no geometry. The wheel zooms about the pointer, the
middle button pans, Zoom In Region (armed from the top bar) zooms to a box, the zoom buttons and Zoom to Fit work on the section when it
is shown (`App::zoom_by` and `zoom_fit` hand over to `section_zoom_by` and `section_fit`).

**Elevations and callouts of sections** are `ViewKind::Section` too: the same request, `render::section_*` takes the id of any of them.

**Not in this piece, from egui's `section.rs`:** the hover tint in the picture (a hovered element the picture shows is picked and named
in the status bar but not lit; only what the drawing owns is), Tab through what lies under the pointer, the crop and callout grips, the
level grips, the vertical drawing tools (`section/tools.rs`: Dimension, Detail Line, Trim, Move in the drawing plane, which wait for
`rune-tools`' tool state machines), the underlay and the background grid, Co-Work's pointers, Previous Pan/Zoom, and a picture at the screen's
pixel ratio (the picture is a pixel to a point, so it is soft on a high-density screen; the core's picture takes a `ppp`, which a later
change can pass). The status bar shows no coordinates over a section.

## 3. Schedules and sheets (`sched.rs`, `app/schedule.rs`, `ui/schedule.rs`, `paper.rs`, `paper/render.rs`, `app/paper.rs`, `ui/sheet.rs`)

**Schedules.** `sched::make` turns the core's `schedule::Table` (`table_kept`, which the core keeps for the state of the model's elements)
into what a grid shows: each cell as text in the project's units and language (`schedule::shown_to`, headings through `schedule::heading`,
a yes or no through `t`), the elements each row lists (`Table::ids`), the totals, and a note instead of a grid when the core refuses
the schedule, it has no rows (`No rows yet: the room schedule is empty.`) or the view is a panel schedule or a reinforcement detail,
which keep the card that names them until their own slice. It is cut at 50,000 rows and says so. `App::grid` holds it with the
`rune_kit::TableState` (the keyboard cursor, the focus), made again after every command (`follow_view`, from `refresh`) only when the
model is another, and let go when another kind of view is shown. The canvas is `rune_kit::Table`: only the rows on screen are built,
a sticky header, the numeric columns right-aligned and monospaced, zebra rows, the totals in a line under it. **Rows and the
selection:** a row is lit while what it lists is selected (`App::row_lit`, in the table's row hook), and a click on it picks that
(`App::pick_row`, with `How::of(shift, cmd)`: Ctrl adds, Shift takes away), through the model's selection, so the plan and the
section have it selected too. Not in this piece, from egui's schedule: the right-click menu on a cell, sorting by a column header,
grouped rows with frozen headings (the table is as the core grouped it), and editing a schedule (the Schedule Properties dialog is
`rune-dialogs`').

**Sheets.** `sheet::draw` is the core's drawing of the paper (the title block, each view at its scale, schedules, notes), which costs what
the views on it cost, so a sheet has a thread of its own as a section does (`paper::Paper`, the same plan as `section::Cut`:
`worker::Latest`, the old picture moved through the live offset and scale while the thread paints the next). The thread keeps the
drawing for the model and paints a raster of it for the camera (`render::raster`, tiny-skia on the CPU: the whole drawing scaled by
`view.scale / DENSITY`) as one image of the part of the paper the canvas shows, shown as one `Pixels` on the desk, so lettering and line
weights keep to the paper at every zoom and a wheel notch costs the thread a paint of the drawing and no geometry. The camera frames the
whole paper 24 points clear of the edges, as egui's (`paper::framed`). **Picking** is `Drawing::pick` and `in_box` on the sheet's
drawing, which a viewport owns for its view, so a click on a view of the sheet selects that viewport; the selected and the hovered ones
are lit by a frame round their box on the paper (`paper::owner_box`, taken to the canvas through the picture's view), since the raster
has no per-element colour. Zoom to Fit, the zoom buttons and Zoom In Region work on the sheet as on a section.

**Not in this piece, from egui's `paper.rs`:** dragging a view or a note to move it (the `move` command is there, the drag waits for the
tool state machines), a double click that opens a view (the viewport's view and the 3D tab), the Text tool and Paste on the paper, a note's
editor, the export region's band, the right-click menu and the Co-Work pointers. The picture is a point to a pixel, so it is soft on a
screen of high density.

## 4. The 3D view (`orbit.rs`, `orbit/render.rs`, `app/orbit.rs`, `ui/view3d.rs`)

**The processor's picture, on a thread of its own.** The core's `Scene` (every solid of the model found and cut to triangles: the
heavy step, as `Scene::new` is for a section's `SectionWork`) and its software picture (`Scene::render_in`) are made by `Orbit`'s thread
(`worker::Latest`, the plan of `Cut` and `Paper` once more): the first request has no camera and the thread fits the model
(`Scene::fit_in` and `fit_from`) from the default azimuth and elevation; an edit brings the scene it kept up to date
(`Scene::update_cancellable`, as egui's `orbit.rs` does) instead of making it again; a camera that moves or another selection paints the
scene it kept again and builds no geometry. The picture is one `Pixels` of the canvas's size, a pixel to a point, shown on the core's paper
colour; its pixels know which element each shows, so the picture is also what picks.

**Navigation, as egui's header says Revit has it.** The wheel zooms about the point under the pointer (`Camera::zoom`), the middle
button pans (`Camera::pan` from the camera at the press, so the drag does not drift), Shift with the middle button orbits, 0.4 degrees a
point, round the point of the surface under the pointer where the drag began (`Image::point`, else the target; `Camera::orbit_about`).
The primary button picks what the picture shows there (`Image::pick`) or drags out a box (`Image::in_box`: from the left what the picture
shows only inside it, from the right anything of it). The selected elements are tinted in the picture through the request's tints, so a
selection is another picture and no new geometry. Zoom to Fit and the zoom buttons work on it as on the other views (a fit keeps the side
the camera looks from). While the thread paints, a pan or a zoom moves the last picture through the live offset and scale; an orbit changes
the whole look of the picture, so the last one stays as it was until the new one comes, and the view says nothing in between (egui fades it
and says what it waits for).

**Not in this piece, from egui's `orbit.rs`:** perspective (`Camera::lens`; the view is parallel), a camera view's own camera
(the default fit is used for every 3D view), the ViewCube and navigation triad, fly mode, the 3D mouse, the section box and its
grips, the level and window and wall grips and the tools that work here (they wait for the tool state machines), the hover tint, the
wireframe and shaded styles beyond what `Look` carries, a picture at the screen's pixel ratio, and what the model's own `{3D}` view stores.

**What a graphics-card path would need.** egui's `gpu.rs` and `gpu.wgsl` draw `view3d::gpu::Mesh` (the core's triangles for the card,
`Mesh::new(&scene, &look)`, with its own picking, `pick_cut`, and the section box) in a wgpu pass of the window's own device. Rune has
the seam for it: an app-supplied `Compositor` (`rune::compose`, attached with `App::compositor`) takes over frame encoding, so a pass
that draws the mesh into the rectangle of an element keyed `view3d` could run beside Rune's own instances, on the same device and
swap chain. What is missing here is the work, not a hook: the `Compositor` must be handed the mesh and camera from the app's state
(the state is `S`, which the compositor is generic over), the picking must move from `Image::pick` to `Mesh::pick_cut` (a pixel's
triangle comes from the card in egui, from the mesh on the CPU), and the CPU renderer (`RUNE_CPU_FIRST=only`, the path a machine with no
card takes) must still show the processor's picture, as egui's software fallback does, so both stay. The processor's picture here is the
one a machine with no card shows, and the one the measurements of `rune-ui.md` were made without a card for; the card path is a
speed-up for a machine that has one, not a need.

## 5. Export and print (`app/export.rs`, `ui/export.rs`)

**Through the existing command.** `export` is the core's export engine (`export_formats`: PDF, SVG, PNG, IFC, glb, CSV, XLSX and the rest, a
file or a folder of them, the page set up by its `page` and `pdf` parameters; [export](export.md)). The window runs it as every front end
does, `Session::execute("export", ...)`, and never writes a file itself. PDF of a sheet (`pdf.rs`) is `export {format: "pdf", view: <sheet>}`.
Print is that PDF: the window has no printer dialog of its own, as egui's File > Print also writes a PDF, and a print set is
`export {print_set: ...}`.

**The minimal path, for now.** Two buttons on the top bar, PDF and PNG, ask the system's dialog for a file (named after the view, with
what a file name cannot hold made `_` and the braces of `{3D}` left out: `A101 - Ground Floor.pdf`) and run the command for the view
shown with the file's path (`App::export_view`). `App::export_target` names the view as `export` takes it: the id of a plan, a section,
a sheet, a table or a named 3D view; the name of a schedule every project has and of the default 3D view; none for a ceiling plan (its
id is the level's, which the command would take for the floor plan) and for no view. A refusal (a format the view has none in, a
failure the command reports under `failed`) is said in the status bar and writes nothing, and so is a success (`Written to <path>`).
The form for the command's other parameters (page size, scale, colour, a region, PDF/A, several views to one file, a print set) is the
generic command form of `rune-dialogs`, which this branch's base does not have: **when it lands, the buttons become that form's
`export`**, and `export_target` and `export_file_name` are what the form fills `view` and `name` with. Not here: the Publisher and the
Print Sets window (their own dialogs), the page preview, an export on a thread of its own with a Cancel (egui's `export/` writes on one:
a sheet of the tower is long, and this one blocks the window while it writes), and Export Region's band on a plan or a sheet.

## Measuring the views

`ci/measure-all.sh` has a scenario for each view other than a plan on the sample house, twice over as the others are, on the CPU renderer
with no graphics card: `rune-cpu-view-section`, `-sheet`, `-schedule` and `-3d`. The window is started on the view
(`BIMCRAFT_RUNE_VIEW=section|elevation|sheet|schedule|ceiling|3d`, read by `lib.rs`'s `start`, `App::open_named`), and then the script
does what it does to a plan: idle, the pointer moving over it, a wheel notch, a pan step. The process's CPU time counts every thread, so the
figures include what the view's own thread makes of each notch: the picture of the section, the sheet's paint, the 3D view's render; the
interface's thread costs a state update and a live offset. Linux only: the Windows script (`measure-ui.ps1`) has none, so the views have not
been measured on WARP. The numbers go in `rune-ui.md`'s table when a run has them.

## 6. Revit shortcuts (`crates/core/src/shortcuts.rs`, `keys.rs`, `app/keys.rs`, `ui/mod.rs`)

**The table is the core's.** The two-letter shortcuts were the egui window's alone: the commands' own aliases (`WA` Wall, `CR` Section) are
in `commands::ALL`, and the desktop's other 70 (`ZF`, `3D`, `MD`, the snaps), 27 more the command catalog names, the MEP fixtures' 13 letters
and the 26 default chords (`Ctrl+Z`, `F3`) sat in egui's `lib.rs`, `shortcuts.rs` and `mep.rs`. `shortcuts.rs` in the core holds them as data
(`DESKTOP`, `EXTRA`, `FIXTURES`, `CHORDS`) and answers `target(code)` (a core command by id, or one of the desktop's own by its English
label), `begins(prefix)` and `chord(written)`. A code is claimed by the first of the commands' aliases (in the command table's order), the
codes the lists give a command's label, then the desktop's own. **A test reads the egui sources** (`include_str!`, so it costs nothing at
run time) and fails when a list differs from egui's, so neither front end can have a shortcut the other lacks: a shortcut is added to
egui's list and the core's in one change. `crates/ui-egui` is untouched. egui's own registry on top of the table (profiles, user bindings,
the Keyboard Shortcuts dialog, `settings.json`) is not here: the table is the BIMCraft profile's starting set, and the window has no way
to change a binding yet.

**The matcher.** `keys::Typed` is the typed letters and when the last came (the window's clock). A plain letter or digit, Shift allowed and
no other key held, that begins a shortcut waits two seconds for the next (`WAIT`); the second completes one and it runs, or it does not and
the first is dropped and the second waits in its place (`QWA` is `WA`); a letter that begins none is the window's. Escape drops what waits,
time drops it (`ui::poll`, which asks for a frame when it is due), and the status bar shows the letters waiting before its line. With a dialog
open the keys are the dialog's, so Find or a command's form takes `WA` as text. A chord is written the way the table writes it (`Ctrl+Shift+Z`,
`F3`, `Delete`: `keys::written`) and looked up the same.

**What a shortcut does** is done by the methods the buttons use (`app/keys.rs`): a command that is a tool of the palette puts it in hand
(typing it again leaves it there, as Find does), `undo`, `redo` and `delete` act at once, any other command opens the command's form
(`open_command`, the dialogs' generic form of the core's table); of the desktop's own, `3D` opens the 3D view, `MD` puts the tool down, `ZF` (and
`ZE`, `ZA`) is Zoom to Fit, `CS` Create Similar (the tool of the selected element's category) and `SA` Select All Instances (its category and
type). Any other says `<name>: not in this window yet` in the status bar and changes nothing, so a shortcut is never silent. They work in
every view: the plan, a section, a sheet, a schedule and the 3D view (Zoom to Fit hands over to the view shown, as the button does). Chords:
`F3` the 3D view, `F2` the plan, `Ctrl+0` Zoom to Fit, `Ctrl+R`, `Ctrl+M`, `Ctrl+T` and the rest of Modify's tools, `Ctrl+Z`, `Ctrl+Y`,
`Delete`, `Ctrl+K`, and the three that ask for a file, `Ctrl+S`, `Ctrl+Shift+S`, `Ctrl+O` and `Ctrl+P` (PDF), which call the top bar's own
functions (`topbar::open`, `save`, `save_as`, `export::ask`). `Ctrl+N` (a new project needs the unsaved-changes guard) and the palette and
home chords wait.

**Tested:** the core table (the egui parity, two letters, one owner, every alias resolves, every chord names a command or a `ui.` action);
the matcher (`keys/tests.rs`: waiting, the second that makes nothing, the wait that ran out, a clock that is no number); the actions
(`app/keys/tests.rs`: tools, Zoom to Fit, the dialog's letters, every kind of view, and a test that walks **the whole table** and asserts that each entry does
what it names or says it cannot, and that every command's shortcut puts its tool in hand or opens its form); and the window
(`ui/keys_tests.rs`: typed through Rune's `Runtime`, the status bar, the timeout, the chords, `Ctrl+S` and `Ctrl+O` through the file dialogs).

**Not done:** the desktop's own that need what this window lacks (the snaps' one-shot overrides, the Hide and Isolate family, Thin Lines and the
visual styles, Properties and Project Browser toggles, Keyboard Shortcuts, Settings pages): each says so. Letters are read as typed, not as
the physical key (egui reads the key's place, so `WA` works on a Russian layout): on a layout whose letters are not Latin, a shortcut is
typed with the Latin ones, until Rune's key events say which key it was. A tool that takes a typed length (digits and a unit) has no
such typing here, so digits are only ever `3D`'s.

## 7. The Project Browser's leftovers (`views.rs`, `views/find.rs`, `app/browser.rs`, `ui/browser.rs`, `context.rs`)

**Tabs.** All, Views, Schedules, Sheets and Families, as egui's (its Legends tab has nothing to list and is left out). Each group knows its
tab (`Group::tab`); a tab keeps to its groups, All lists every one, and a tab that keeps to a single group shows its rows without the group's
name, as egui's does. Linked Models is listed under All only.

**Families and Linked Models.** The Families group lists every type of the project as a row `Wall: Exterior - 300mm`, by category and name,
from the same table Type Properties reads (`typeprops::all`); it starts folded, being the longest list. A press on a type opens its Type
Properties (the window has no Type Selector, which is what a press does in egui). Component families as such are not listed apart from
their types. Linked Models lists each link (`name`, or `name (not loaded)`); a press selects it. These rows are `Pick::Type` and
`Pick::Select`; a view's row is `Pick::Open`.

**Search.** The words typed, split at spaces, at most 64 characters of them; each must match a row's name or its group's name (a word the
group's name matches is spent, so `ceiling 2` finds Level 2's ceiling plan), with egui's wildcards, `*` for any run of characters and `?` for any
one, case aside (`views/find.rs`, a copy of egui's `spans`, tested the same). A search opens every group it finds something in and lists no
group with nothing in it. Not here: egui's marks on the letters that matched, and a search kept across projects.

**Picking views together.** A plain press opens the view and is the one picked; **Ctrl** toggles one without opening (the view on screen counts
as picked first), **Shift** picks the run from the last one picked to the one pressed in the order the browser lists them now (the tab and the search
apply, folds do not), Shift and Ctrl add the run (`Views::pick`). With more than one picked, the picked rows are lit instead of the view shown.
What is picked is let go of when the model no longer has the view.

**The right-click menu**, by `context::browser_items` and `App::act`, the plan's menu's own mechanism: the same `Action` type, `rune_kit`'s `Menu` at the
point the click came from, one open at a time with the plan's. Over a level's or a plan view's plan: Duplicate, Duplicate with Detailing, Duplicate as
Dependent (`duplicate_view`, the copy opens); Delete over a plan view, a callout, a 3D view other than `{3D}`, a schedule of the project's and a reinforcement
detail (`delete`); Create Print Set over a view (`print_set`, named `Print Set n`, of the views picked together when the click was over one of them, else of the one
clicked; a ceiling plan, which is no element, cannot be in one yet, and the item says so); over a type: Edit Type and Delete (`delete_type`, which the
project refuses while an element uses the type); over a linked model: Reload and Remove (`reload`, `delete`); and Expand All and Collapse All over anything.
Show as Underlay is in it since section 8. Not here, from egui's menu: Publisher (a dialog), Select All Instances over a type, the Delete over a
family's whole group, and the Create Print Set dialog that names the set (the name is made).

**Not done in this slice:** the minimap and the 3D card path, which the brief left out.

## 8. Clipboard, underlay, groups and the work plane (`clip.rs`, `app/clip.rs`, `ui/choose.rs`, `app/underlay.rs`, `app/groups.rs`, `app/workplane.rs`, `ui/workplane.rs`)

The editing and reference pieces around the canvas, each from egui's version (`clipboard.rs`, `underlay.rs`, `groups.rs`, `workplane.rs`), the core doing what it
does for egui (`clipboard`, `underlay`, `groups` and `workplane` modules, the commands). All are items of the plan's right-click menu (`context::items`), through `App::act`.

**Clipboard.** Ctrl+C and Ctrl+X (`copy_to_clipboard`, `cut_to_clipboard` over the selection, in the view shown) and Ctrl+V are read by `ui::on_key`: they are
not in the shortcuts' table, because egui has them as the system's copy, cut and paste events, and a field that has the focus takes them itself. The text a copy
makes is kept by the window (`clip_text`) and queued for the system clipboard (`clip_out`); a paste asks for the system's (`paste_asked`), and both go through `ui::clipboard_io`,
which has the `Tx` (called after a key and from every `poll`, so a menu item reaches it too). The system's text is used when it is BIMCraft's clipboard text,
else the window's own copy, as egui's `pasted_text` does. Paste (the menu's, Ctrl+V): by hand in a plan (the text waits in `pasting`; the next click in the plan puts the middle of the
copy at that point, `paste` mode `manual`; Esc, or opening another kind of view, gives it up), in a section, an elevation or a sheet where the elements were in their view
(`current_view`), elsewhere where they were (`same_place`). The menu also has Paste Aligned to Same Place, to Current View, and to Selected Levels or Selected Views, which open a list
of the levels, or of the views like the ones the elements were copied from (`clip::views_like`), to tick (`ui/choose.rs`, `clip::Choose`). What was pasted is selected and what the
core left out is said. Not here: the copy following the pointer as a preview (egui draws it), by hand in a section, a sheet or a 3D view (a 3D view asks "same place or by hand on
the work plane" in egui), Aligned to Picked Level, the Clipboard panel in the ribbon and the palette, and a family tab's clipboard.

**Underlay.** Show as Underlay and Remove Underlay in the browser's menu over a plan, put under the plan shown (`underlay` `{view, source}` or `source: null`; the engine
keeps it with the view). The plan is drawn with it: `App::redraw` also draws the underlay through the camera (`bimcraft_core::underlay::draw`, one more drawing of the source while
it is on, where egui uses a worker) and the plan canvas lays it under the plan's lines, or over them with `front`, as a layer of its own (`plan-underlay`). Plans only, as the plan shown and as the source: a section as the source would be a section picture drawn on the
window's thread at every redraw (egui's is a worker's), and the underlay of a section or an elevation is not drawn here. Not here: Properties' Underlay group and the Underlay Settings (colour, fills, annotation, front, Move and
Rotate by picks; the `underlay` command's form takes them all), and snapping to the underlay's lines.

**Groups.** Create Group (GP), Ungroup (UG), Edit Group (GE) and, while one is edited, Add, Remove, Finish and Cancel, and Exclude (EM) and Restore All Excluded (RA), in the plan's menu where
the selection allows them (`App::group_items`) and typed as the shortcuts, which act on the selection instead of opening a form of ids. A selection of both kinds asks to be of one;
a selection with some of one kind only makes a group of that kind. Add and Remove act on the selection (select what is to join or leave, then the item), where egui picks them with clicks
on the canvas. Not here: Place Model Group and Place Detail Group, Repeating Detail, the canvas's greyed look and the selection kept to the group in Edit Group, and the question a change
that would break a group asks (the engine's refusal is the status bar's).

**Work plane and reference planes.** The top bar names the 3D view's work plane (`Level : Level 1`, with the offset) while a 3D view is shown, and a press opens the Work Plane dialog:
Current Work Plane, Dissociate (a work plane on a reference plane left where it is), and a name (a level or a named reference plane, `workplane::choices`) with an offset in the project's
units, set by `work_plane`. The Reference Plane tool (RP) is in the Architecture palette: two clicks, `reference_plane {start, end, level}`. Not here: Show (the plane's grid in the 3D
picture), the three picks (a face, a line, three points, which need the 3D view's depth under the pointer), the components' Edit Work Plane, and egui's Snap References (`guide_refs.rs`, the
`Q` marks that lend a guide).
