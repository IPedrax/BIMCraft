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
