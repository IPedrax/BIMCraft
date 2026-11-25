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
