# The plan tools of the Rune UI

Stage 3 of [the Rune UI](rune-ui.md), the part after snapping: what the points a tool in hand has had picked become. Written on the
branch `IPedrax-Test/Rune-GUI-tools` (cut from `IPedrax-Test/Rune-GUI`); the progress folds into `rune-ui.md` when the branches merge.
The egui canvas (`crates/ui-egui/src/canvas.rs`, `draw.rs`, `structure.rs`) is the reference and stays untouched: where it holds logic
this crate wants, it is read, never edited, and what is copied goes into the core as a new module with its own tests (none yet).

## The model: a table and a pure step

`App::tool` is the id of a palette tool, `App::picked` the points it has had picked (model mm, the click that has just been made
last). `crates/ui-rune/src/tooling.rs` knows no window and no widget:

- `SPECS` has one `Spec { id, command, kind }` for each drawing tool of `tools::TOOLS`: the core command it ends in and a `Kind`
  that says how the points become that command's params. A test keeps the two tables the same and every `command` a command the
  core has (`engine::find`).
- `advance(spec, ctx, points) -> Step` is the whole state machine, a pure function of the points picked so far, the level, the
  element under the pointer and the Draw panel's shape. `finish` is the same for Enter. A `Step` is `More` (wants more points),
  `Ignore` (the click is not kept), `Done` (finished, makes nothing), `Run { commands, keep }` or `Refuse(why)`.
- `crates/ui-rune/src/app/tooling.rs` carries a `Step` out: `Run` goes through `Session::execute` (several commands as one
  `Session::transaction`, one undo step), a refusal of the core is `App::message` (the status bar) and the click that was refused
  is given back, `keep` says what the points become (`Chain::No` none, `Last` the last, `Whole` all of them).

How a tool finishes: the click that completes it; the last point clicked again (a double click is two clicks at one place, and the
snap lands the second on the end of what the first made, so it is the same place); or Enter (`App::finish_tool`, a path's only way).
Backspace gives the last point back (`App::back_point`). Escape is `App::escape` as before: the points, then the tool, then the selection.
A key the tool has no use for (Enter or Backspace with nothing picked) goes on, as `ui::on_key` returns false.

## The kinds

| Kind | Tools | Params |
|---|---|---|
| `Point { key }` | door, window, column, structural column, footing (`at`), room, ceiling (`point`) | `{key: [x, y], level}` on the first click |
| `Anywhere` | floor, roof | `{level}` (the roof's is the level above, as egui's), no point sent: the core finds the walls |
| `Span { from, to }` | stair, ramp | `{start, end, level}` on the second click; the same place twice is ignored |
| `Run { chain, shapes, curtain }` | wall, curtain wall (chain kept whole, closes on its first point), beam, strip footing, pipe, conduit, cable tray (chain from the last point) | `{points: [prev, last], level}` for each click after the first, as egui makes a wall per click |
| `Path { key, min, host }` | railing | points until Enter or the last point again: `{path, level}`; a first click on a stair or a ramp makes `{host}` instead |
| `Family { kinds }` | component, plumbing fixture, drain, outlet, switch, lighting fixture | `component {family, type, at, level}` of the first family (by name) of those kinds that has a type |

The Wall tool takes the Draw panel's shapes (`draw::Shape`): a row of buttons over the plan's top left (`ui/draw_bar.rs`, there only
while the tool takes a shape) picks Line, Rectangle, Inscribed and Circumscribed Polygon, Circle, Start-End-Radius Arc or Center-ends
Arc. The shape's clicks are `Shape::clicks(&Options)`; when they are all in, `draw::make` gives the curves and `draw::wall_segments`
the walls, a run of segments end to end that turn alike one `wall` command (a rectangle is one closed command of four walls, a circle
one closed command of two half circles). While the clicks are made, `App::shape_preview` is `draw::preview` of the points and the
landing, drawn by `paint::ghost` as a layer of its own (`plan-ghost`); a Line has no ghost, as `paint::over` already draws the chain's
segment to the pointer. Changing the shape gives up the points. The polygon's sides (`Options::sides`, 6) and a radius have no control yet.

## The Modify tools

The same machine, in the same table (`SPECS` holds the tools of `tools::MODIFY` too), on what is selected or picked. A tool that needs
the selection and finds it empty refuses on its first click and says so in the status bar. The pointer lands with the snap and the
guides as for any point (the chain's first point is the base).

| Tool | Command | Clicks |
|---|---|---|
| Move, Copy | `move`, `copy` | a base point, then the destination: `{ids, by}`. A copy leaves the copies selected; the tool is put down after |
| Rotate | `rotate` | the centre, a point on the first ray, a point on the second: the angle between them, counter-clockwise (`angle_between`) |
| Mirror | `mirror` | the two points of the axis, with Copy on as Revit's default |
| Split | `split` | a click on a wall or a detail line, at the snapped point; stays in hand |
| Trim | `corner` | Trim/Extend to Corner: each of the two elements picked on the part that stays (the core's `corner` takes both walls and detail lines) |
| Align | `align` | a click on the element whose line is the reference (`align::nearest` within the pointer's reach), then on the element to bring to it |
| Offset | `copy` for a straight wall, `offset` for a detail line | the element, then a point that says how far and to which side; a copy is made |

What a Modify tool keeps between clicks besides the points is `Tooling::held` (the element or the line its first pick named); the
first click overwrites it, so Escape, Backspace and a change of tool leave nothing stale. `Step::Hold` is how the machine sets it.
Move, Copy, Rotate and Mirror are put down once they have worked, as in the egui window (which goes back to Modify); Split, Trim, Align
and Offset stay for the next pick, as they do there.

**Not done:** Array (the core has no command for it, and it needs a count), Scale (the core's `scale_elements` takes a factor and only
scales annotation, CAD drawings, images and PDFs; egui types the length), Offset's typed distance and its Copy switch (a copy is
always made, by the distance of the second point), Move and Copy's Multiple and Constrain, Rotate's Copy and a typed angle, Mirror's
pick-an-axis (the axis is two points), Trim/Extend to a single boundary and to many (`trim_extend`), Match Properties, Pin. Egui's
smaller things in a move (a door sliding to the middle of its wall, `move_point`) are not here either.

## The right-click menu

A right click on the plan opens `rune_kit`'s `Menu` at the window point (`on_secondary_press` on the plan, `Tooling::menu`, `ui/menu.rs`;
its elements go in a layer mounted over the window by `rune_kit::root`, which wraps the window only while the menu is open, so the plan is
as it was when it is closed). The items are `context::items(model, selection, tool, finishable)`, groups of `Item { action, enabled }`:
Finish Drawing (a tool with points picked) and Cancel; Delete; Pin and Unpin; Disallow Join and Allow Join (each offered when some
selected wall or run would change); Zoom to Fit, In and Out. `App::act` does the item and closes the menu (Cancel is `escape`, Finish is
`finish_tool`, Pin and the joins are `pin`, `unpin` and `modify`), and Escape or a press outside closes it without doing anything.
`context.rs` is a copy of the part of the egui window's `context_items` and `Action` that is about the plan and the selection, **a
duplicate to fold into the core when egui adopts it**. Not in it from there: Edit Family and Type, Select All Instances, the groups'
items, Hide Element, Hide Category and Override Graphics (a view's), Reload CAD, Create Circuit, Edit Dimension References and the
clipboard's Cut, Copy and Paste. The Revit shortcuts the egui menu shows beside an item wait for the shared registry; Esc, Enter and Del
are shown.

## Grips

The wall, run (a beam, a pipe) or grid selected alone with no tool in hand shows a square at each end (`paint/handles.rs`, the layer
`plan-grips`, there only then). A primary press within eight points of an end takes it (`App::grip_down`, asked first by the plan's
drag handler), the drag follows the pointer with the snap and the guides from the end that stays (`Snapper::land` with that end as the
chain; Shift squares it; Ctrl keeps the length and turns the element about the fixed end), the line the element would take is drawn with a
reading beside the end (`plan-grip-tag`, the ruler's length and angle), and the release is one `move {ids, by, end}` (the walls joined
there follow). Escape, which cancels a Rune drag, puts it back. The pure part, which end and where it lands, is `grips.rs`, tested alone;
it is **a duplicate of egui's `gripped`, `grip_ends`, `grip_target` and `grip_drag` to fold into the core when egui adopts it**. Not here
from there: the angle to click and type beside a wall's end, a Fixed Grid wall's stay in its pattern's lengths, the element's outline as
it would stand while dragged (the line is drawn), the ruler's graduations, and the grips of a stair, a section, a callout and a shape.

## Temporary dimensions

The wall, door, window, column, structural column, footing or component selected alone, with no tool in hand and no end being dragged,
shows what it measures to (`tempdim.rs`, from the core's `wall_clearances`, `clearances` and `opening_span`): a straight wall its length
and its distance from the nearest parallel wall or grid on each side, an opening the two stretches of its wall either side of its middle,
a standing element its distance to the nearest wall centre line or grid east, north, west and south. `paint/dims.rs` places each (its line
parallel to what it measures and a little off it on its side, extension lines and ticks; none under 24 points) and `ui/tempdim.rs` draws
the lines as one inert layer (`plan-dims`) and a `rune_kit::TextInput` at each line's middle (`plan-dim-<n>`) on the number being typed
(`App::temp`, made with the Properties rows in `App::refresh`). Enter commits it (`App::commit_dim`, `tempdim::apply`): a wall's length is
`wall_length`, a clearance moves the element toward the line by the difference (`move`), an opening slides (`modify offset`); a number that
is no length, or past the wall, is the status bar's message. The pure part is **a duplicate of egui's `temporary_dimension`,
`wall_dimensions`, `opening_dimensions` and `standing_dimensions` to fold into the core when egui adopts it**. Not here from there: a
curved wall's arc length, Make Dimension Permanent, the side of a wall the line is drawn on (the wall's left here, away from the middle
of the view there), and a dimension's locks and equality.

## Not here yet

- **The Options Bar.** Types, heights, the wall's location line, a beam's justification and offset, a ceiling's height, a roof's
  slope, a stair's width, Chain on and off: every tool runs with the command's own defaults (and a chain is always on for walls and
  runs). The curtain wall tool picks the first curtain type there is; a family tool the first family and type of its kind. The Type
  Selector, the thing that would choose, belongs with the forms.
- **What egui adds to a click:** a door's or window's centring on its stretch of wall and its hand and facing (the click sends the
  point only; the core finds the wall), a wall's fixed-grid ends, a stair's or ramp's path with landings and winders (a stair is its
  two ends), the rafters of a roof by extrusion, dormers, shafts, slope arrows, mass, detail lines, grids, reference planes, sections
  and cameras, the Revit shortcuts (they wait for the shared registry), and the typed length and angle (`draw::reading`, `typed`).
- **Shapes that pick elements**: Fillet Arc, Pick Lines, Pick Faces and Tangent End Arc (which needs the wall before it) are left out
  of the bar; the machine refuses them if a test sets them. Open-ended shapes (spline, polyline) are not walls in the core.
- **Previews of what stands:** a ghost of the wall's thickness, of a door in its wall, of a column. The pointer's glyph and the
  chain are all a point tool shows.
- **After a click:** egui selects the roof or floor it made and goes back to Modify; here the tool stays in hand and nothing is selected.
- **A drag with a tool in hand** still pans on the primary button (the placeholder of stage 3, slice 4); a Modify tool takes the two
  clicks of a base point and a destination, not the press, drag and release of one.

## Where the tests are

- `src/tooling/tests.rs`: the machine as a pure function, kind by kind, the shapes, hostile points, the cap.
- `src/app/tooling_tests.rs`: through `App` with no window: a wall chain click by click and undo, Backspace and Enter, a point tool and
  the core's refusal in the message, a path and a stair, a shape as one undo step with its ghost.
- `src/ui/tooling_tests.rs`: through Rune's headless `Runtime`: taps, Enter and Backspace as keys, the shape bar and the ghost layer.
- `src/paint/preview.rs`: the ghost's strokes. `src/paint/handles.rs`: the grips' squares.
- `src/context/tests.rs`, `src/app/menu_tests.rs`: the menu's items and what each does; the menu through the window is in `ui/tooling_tests.rs`.
- `src/tempdim/tests.rs`, `src/app/tempdim_tests.rs`: the dimensions of the sample house's elements and what a typed length does; `src/paint/dims.rs`:
  where they are placed and their strokes; the dimensions through the window are in `ui/tooling_tests.rs`.
- `src/grips/tests.rs`, `src/app/grips_tests.rs`: the grips' pure part, then the press, the drag and the move through the app; the drag through
  the window is in `ui/tooling_tests.rs`.
