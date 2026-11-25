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

**Canvas.** `ui/canvas.rs` picks the canvas of the view shown. Plans (and a project with no level) keep the plan canvas exactly as it
was (`ui/plan.rs` is untouched); the kinds with no canvas yet show a card naming the view, so a press is seen to have opened it. Each of
the next pieces replaces its kind there.

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
