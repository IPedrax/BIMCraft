# Curtain walls

A curtain wall is a `Wall` whose type holds a pattern (`WallType::curtain`, `curtain.rs`), so rooms, joins, constraints, tags and
filters treat it as any wall. Its panels and mullions are worked out from that pattern and the wall's size by
`curtain::layout`, never stored one by one.

## Types

Panels and mullions are system families: `Types::curtain_panel` (the Generic Panel family: `Glazed` and `Spandrel` to start with;
thickness, material, offset) and `Types::mullion` (Rectangular Mullion, whose starter `50 x 150` stands 50 mm toward the exterior: width, depth, `offset_x`, `offset_y`, material, both offsets
from its centre). `MullionProfile` holds the section, so a drawn profile can be another kind. They are library entries
(`Kind::Type("curtainpanel")`, `Kind::Type("mullion")`), and a pattern's names of them are `CurtainLayout::names_mut`. `type` makes and
edits them. A project of file format 16 gets the starter set when read.

## The pattern

`CurtainLayout` has `columns` and `rows`, each a `Direction`: segments in mm, a `Fit` and a `Justify`. `panels[row][column]` fills a
cell; `vertical[row][column]` is the edge along the line at that column's start, in that row; `horizontal[row][column]` the edge
along the line at that row's bottom, in that column. Each may be empty. `borders` are the wall's own edges. `continuous` says which
mullions run through where they cross. The line that starts a module also closes the one before it.

`type {category: "wall", name, curtain: {columns: {segments}, rows: {segments}, panel?, mullion?, …}}` makes one; `panel` and `mullion`
fill what is left out, and null means none. A curtain wall type's `thickness` is never given: `curtain::sync` makes it what the
panels and mullions the pattern uses reach (`envelope`), after every command.

## The layout

`divide` lays a pattern over a length from where it is justified. A `Span` keeps `index`, its count from the pattern's origin,
which is its name as the wall changes; a part's `Address` is made of those. `layout` then places:

- **Border mullions** stand inside the wall, so it never reaches past its ends, its base or its top.
- **Mullions** sit on their line plus their offsets. A mullion that would run into its neighbour goes, the border winning. Where two
  cross, the bigger runs through and the other is cut back to its faces: see Joins.
- **Panels** sit in their cell plus their offset, cut back to any mullion that reaches them in depth. A panel in front of the
  mullions isn't cut (structural glazing), and with no mullion panels run gapless.
- **Malformed:** a cell left no more than `min_panel` gets no panel. At an end's cut segment that goes in `Layout::malformed`; in a
  whole segment `Model::check` refuses the pattern. Nothing of zero size is ever drawn, and the panel comes back by itself once the
  gap holds one. `CurtainEdits::absorb_columns` and `absorb_rows` (Remove Last Interior Grid) take the short segment into its
  neighbour while it is too small; the wall keeps its length.

## Drawn once

`Layout::shapes` holds each distinct part once (kind, type, size) and `Layout::parts` places them. These shapes are silent: no list or
file shows them. Each piece (see What's drawn and what's picked) is one `Solid::Box` round its own centre (`view3d::curtain_solids`), which sections and elevations read.

On the graphics card, a box of one size in one material is one shape, shared by every wall whichever way it runs, and each part a copy
of it (`view3d::curtain_copies`, `ShapeKey::Part`), as a component is of its family's shape: a wall of 20,000 parts holds 396 vertices
of shapes, and each copy keeps only its place and its faces' plane numbers. A box round its centre is what makes a copy's triangles its
shape's placed to the last bit, which `placements` and `Scene::update` hold it to. An element may be many copies, so both walk a wall's
parts by where each starts among its triangles. A mitred bar, or a part painted or overridden unlike its shape's first, is drawn from
its own triangles. The scene still makes and keeps every part's triangles for the processor and picking, so this saves the card's
memory and draw work, not the scene's.

An unedited part has no id of its own, only its `Address`: an agreed exception to the id rule, since a tower's façades would use up
the six-digit ids and every change of length would rewrite them.

## Fit

`Fit::Fixed` draws a wall in its pattern's lengths: `curtain::snap_length` gives the nearest whole number of segments, at least the
first. `wall` moves each end to it, the next wall going on from there, and says so in `adjusted`; the last wall of a closed chain still
closes it. A height set directly snaps too; one a top level sets can't, and the top row takes up the difference. The desktop's Wall
tool and an end's grip snap the same way (`BimApp::fixed_end`), and background grid snapping stays out of the way while a wall of a
Fixed Grid is drawn (`drawing_fixed_curtain`). `Fit::Flexible` keeps the length given.

Neither changes what `layout` does with a length: at any length, what's left past the last whole segment is a cut segment.

`Fit::Evenly` (Fit Evenly) cuts nothing: `divide` takes the whole number of modules nearest the length, at least one, and stretches or
shrinks each to fill it, its segments in proportion (`evenly`). Justification has nothing left to place, and the length is kept. A shrunk
segment isn't `whole`, so one too small for its panel is reported as a malformed cut one is, and not as a fault of the pattern.

`anchor_rows` (Restart pattern at levels crossing) starts the rows' pattern again at each level a wall passes, a grid line on the level, whatever the
fit: with Fit Evenly, each storey fills with whole rows (`rows_of`). A wall holds where those levels stand (`CurtainEdits::levels`, kept by
`sync` after each command, as `walled` is), so a layout doesn't look through the model and any cache of the wall knows when a level moves. A
storey's rows are numbered `STOREY_ROWS` apart. A short row at the top of a storey is malformed; Remove Last Interior Grid gives it to the row
below, as it does at the wall's end. A level added within a wall renumbers the storeys above it, so a part edited on those rows sleeps
until its row number comes back.

`Justify` lays the pattern from the start, from the end, with whole modules centred (`Centre`) or with a grid line on the middle
(`CentreLine`). What an end leaves over is the cut segment at that end.

## Joins

Where mullions meet, the bigger runs through and the other stops at its faces (`Bar::runs_through`): the deeper, of one depth the
wider. `CurtainLayout.continuous` only fine-tunes a joint of one size, saying which runs through there. One that runs through and ends at the
crossing runs on to the far face of what it crosses (`past`), so a T or a butt corner leaves no notch. A mitre is asked for on a
mullion (`MullionPick.join`) and made only at a corner: exactly one vertical and one horizontal bar on the node, both asking, of the
same width and depth and at the same offset. A T or a crossing stays butt whatever its bars ask for, and so does a corner of unlike
bars. Both bars run out to the corner's outer point, cut on its diagonal; the panels are the same either way. A mitred bar is a
`Shape` with a `face`, four corners round its centre, so shapes are still shared: the 3D scene makes it a mesh (`Shape::tris`) and IFC
a closed `IfcTriangulatedFaceSet`, while a plan draws what bounds it. A bar too short for its two diagonals not to cross stays a box.

A plain wall takes no mitre from a curtain wall (`Arm::butts` in `walls.rs`). Ending where one ends, it ends square at the curtain
wall's far face; ending on one's side, it stops at the near face as on any wall. Rooms still close, as `centre_line` keeps every arm.

The 3D view merges coplanar faces wherever they meet, but each part of a curtain wall keeps its own (`PlaneTable::assign`), so a joint
between two parts in one plane shows a line, and butt and mitre look different from the front.

## One part of one wall

`curtain_part {wall, part, column, row, type | reset}` gives a part another type, or none. It is a `PartEdit` in `Wall.curtain.parts`,
with an id and a GUID of its own (`HasPart`), keyed by its `Address`. An edit whose place leaves the wall sleeps and wakes when the place
returns. `curtain_parts` lists the parts, the edits and the walls in the cells.

`curtain_panel_wall {wall, column, row, type?}` makes a real `Wall` with `panel_of` in place of a panel. `curtain::sync`, after every
command, fits it between the mullions on the grid plane (centre location, unconnected, no room bounding), so what it's told by hand
doesn't last. It joins nothing (`walls::arms`). `CurtainEdits.walled` repeats its cell, so a layout needn't search the model. Delete it to
have the panel back; deleting or copying its host takes it along, and its doors. It goes to IFC as an `IfcWall` beside the host.

A cell that leaves the wall removes an empty wall, with a `warning` in the reply. If the wall holds anything the command is refused, by
`check_panel_wall`. A door is 2100 mm high, so a row must leave at least that clear.

On the desktop, Tab on a curtain wall under the cursor, selected or not (`tab_wall`; only with Modify in hand if it isn't yet), steps
through its corner mullion there, its mullions there each followed by its grid, its cell, and back to the whole wall (`parts_under`,
`next_part`). A plan looks at its cut plane; the 3D view casts the cursor's ray on the grid plane (`on_plane`), which
reaches every row, and a section or an elevation casts its camera's ray the same way (`curtain::parts_on_ray`); a wall seen edge-on offers
none. Tab skips what the view hides (`shown_parts`), though not a grid, which is no part to hide. Properties then shows the part's type,
As in the Pattern and Replace with Wall, and Delete takes the part, never the wall. A mullion's right-click has Select Along Grid
(`Layout::along_grid`, `BimApp::curtain_more`): the mullions along its line picked together, which take a type or are deleted in one
step (`curtain_part {parts: […]}`).

## Corner mullions

The Corner Mullion family (`Types::corner_mullion`, `Kind::Type("cornermullion")`) holds a `MullionType` as the plain family does, plus a
`rotation`, which only it may have: `50 x 150 45°` and `150 x 150` to start with. `CurtainLayout.corner` names the type a wall of that
pattern takes; a project of file format 18 gets the starter types, and its patterns name none until told to. `type {curtain: {corner}}`
fills in the project's first when it isn't given, and null means none.

`meeting` finds where one belongs: exactly two walls ending at the point, both curtain walls, straight, of heights that overlap, and at
an angle. A third wall there, or one running through it, means none. One of the two holds it (`Meeting::holds`): the one ending there, or
of two alike, by their far ends, so no ids are read. It is that wall's part, `Address::Corner {column: 0 at its start, 1 at its end}`,
which an edit on it can retype or take away. Taken away, both walls have their borders again.

`post_at` places it, in the terms of whichever wall asks: Y is from the holder's grid plane and X from the other's, so with both at the
starter's 50 it stands where the two walls' mullions' centre lines meet, whichever way the corner turns. `rotation` turns it from the
holder's run toward the other wall, so at a corner that isn't 90° the 45° type is off the corner's diagonal. `auto_rotation` (Auto Rotation in
Type Properties) adds half the corner's turn to `rotation`: with a rotation of 0, a mullion square to the diagonal at any angle, 30° with 120°
between the walls and 45° at a right angle, whichever wall holds it. What's given is added to that. It stands as high as the two walls reach together.

Each wall loses its border there and any line it would run into, and its panels and cell walls stop square against
it (`Post::reach`, the furthest of it within their depth across the wall), on either side of the wall's own end: glass is never mitred. A
`Part` has a `turn`, radians in plan, which plans, the 3D scene, its card copies and IFC all take. A layout looks at the neighbouring wall,
which `Scene::update` already remakes as what an edit touches. The holder's `Layout::corners` says the place is there even with no mullion in it.

At any angle nothing of the two walls overlaps: where they'd still cross with each stopped at the mullion, which a corner sharper than
about 75° can bring, each also stops clear of the other's glass or mullions (`Post::clear`, `stopped`, `rings_cross`), square, leaving a wedge
between them. From 90° outward the glass closes right up to the mullion. Below 90° on a corner whose exteriors face out, the offsets carry
the mullion away from where the glass meets, and glass stops short of it: about 10 mm at 75°, 37 at 60, 79 at 45, 160 at 30.

Horizontal mullions meet in a mitre round the corner (`Mitre`, `mitre_at`), with or without a corner mullion, which stands through them and hides the cut below it in a plan (every wall's seen parts are drawn before any wall's cut): each
is cut on the line through where the two walls' faces cross, a `Shape` with a `plan`, its outline from above, so a mesh in the 3D scene and
IFC and shared as other shapes are. It takes one of the same size at the same height and offset on the other wall's end (with a manual grid or
an edit taken into account), and at least 30° between the walls (`MIN_MITRE`), past which it would throw a spike beyond the corner. Otherwise
it stops square as above.

## Grids

A pattern's lines are `auto`: `divide` puts them there, and a change of pattern moves them. A wall can have `manual` ones too
(`CurtainEdits.grids`, each a `Grid` with an id and a GUID, `HasPart`), `at` mm from its start or base, which no change of pattern moves. One
past the wall's end sleeps and comes back. `by_hand` lays them over `divide`'s spans: a span's `index` is `MANUAL_LINE` plus its grid's
`number`, so a manual line and what's past it have stable names like the rest, and `past_line`, a border's name, doesn't count them.

`curtain_grid {wall, way, at}` adds one, with the mullion of the pattern's A1 that way (`first_pick`; with none there, the first it has)
for as long as A1 says so; the cell it splits keeps its segment, so both halves hold what it held. `curtain_grid {wall, way, line, at}` moves
one. Moved, an auto line becomes manual: its pattern's line stays away (`Grid.moved`), what lay past it stays what it was (`Grid.seg`), and
its edits and the walls in its cells go with it (`CurtainEdits::renamed`). `reset` takes a manual grid off, and one that was moved goes back
to its pattern's place, taking those with it. It's refused within `min_panel` of another line or an end; a later change of pattern that
brings a line that close leaves no panel there, with the malformed message. `curtain_parts` lists `grids` (way, line, at, kind).

On the desktop, the Modify tab has a Curtain Grid panel with a curtain wall alone selected: Add Vertical Grid and Add Horizontal Grid
(`Tool::CurtainGrid`), which keep the wall selected. Hovering over the wall lights up the place; each click places one; Esc
ends. A plan can place only vertical ones, as it can't say how high, and says so; the tool stays on when a section or the 3D view is opened.
Tab picks a grid (`Address::VerticalGrid`, `HorizontalGrid`). Properties shows `Grid Kind: Auto` or `Manual`, with a button to change it; Move
moves it along its wall, a horizontal one in a section or the 3D view only; Delete takes a manual one off.

A stretch of a line can go by itself, as Revit's Add/Remove Segments: `curtain_segment {wall, parts: [{part, column, row}…], restore?}`. A segment
is named as the mullion on it, and `CurtainEdits.removed` holds those taken off. Its mullion goes and the cells either side make one panel
(`Merged`, worked out in `build`), named and typed by its first cell; a part edited on a covered cell sleeps. A merge must be a rectangle with
nothing left inside it: the command refuses any other shape, or a cell a wall stands in, and takes off what's left inside one. `restore` puts
back the whole line through the panel, so what's left is always rectangles. What a later change of pattern or grid leaves otherwise isn't
merged at all until it's a rectangle again, and a line added through a merged panel divides it. On the desktop, the Curtain Grid panel's
Add/Remove Segments (`Tool::CurtainSegment`) works in a section, an elevation or the 3D view: hovering lights the nearest inner edge of the
cell under the cursor (`Layout::segment_near`), and a click takes it off or puts it back.

## What's drawn and what's picked

The parts are what's picked, typed and scheduled, a stretch of mullion each. What's drawn is `Layout::pieces`: mullions of one line, type and
section that touch end to end are one piece, so a mullion running through its crossings shows no joint in any view (the 3D scene, the card,
sections, and what a plan sees below its cut). A horizontal stopping at a vertical keeps the line there: that's a real joint. A mitred bar stays
a piece by itself, so a joint still shows beside one.

A curtain wall selected isn't tinted, in any view (`curtain::tinted_selection`, `tinted_hover`): only its outline shows, so the part Tab picks can
be seen. Under the cursor, what Tab would go to next is marked in the darker blue (`curtain_aim`), and what's picked in a lighter one
(`picked_part_color`), both filled so a panel shows. A grid line isn't drawn until Tab reaches it. Tab reaches as far as `PICK` pixels at the view's scale, or 60 mm close in.

## The desktop

- **Curtain Wall tool:** the Wall tool drawing the first curtain type, in the Build panel's list and the Tools palette (the ribbon has no
  width to spare); the Wall tool's Type Selector offers the same types. The Options Bar's Location Line is kept where it is a face.
- **Type Properties:** a curtain wall type shows Layout [Edit…] and its thickness, which can't be typed. Duplicate keeps the pattern.
- **Edit Pattern** (`crates/ui-egui/src/curtain.rs`): segments each way, fit, justification, borders, and one module to scale, on a
  sheet of the canvas's own ground with a short hint in it. Click a
  panel or a mullion, Shift to add to the selection, and pick its type, none or how it joins. Each piece is a widget named as tests
  and screen readers find it: `Panel B1`, `Vertical B1`, `Horizontal B1`.
- **Project Browser:** Families > Curtain Panels > Generic Panel and Mullions > Rectangular Mullion; a type's right-click has Edit Type.
- **The message:** `BimApp::curtain_message` looks for walls newly malformed whenever the model changes, so it comes when a change is
  finished and never during a drag, once for a wall while it stays so, one message for many walls, and not for what a project opened
  with. Remove last interior grid sets `absorb_columns` or `absorb_rows`; I will fix it myself leaves the wall, and `inspect` keeps
  warning. Agents get the same words in the `wall` reply's `warning` and in `Model::warnings`.

## Schedules and pens

`curtainpanel` and `mullion` are schedule categories with a row per part (`schedule::curtain_parts`): type, sizes, area or length, and the
`Wall` it is in, which is what its row selects, as a part has no id of its own. Grouped by type and size they count identical parts.

`object_styles` has a row each (`graphics::CURTAIN_PARTS`), which plans draw panels and mullions in. Sections and the 3D view draw a wall's
pens.

## Visibility/Graphics

`curtainpanel` and `mullion` are subcategories of `wall` (`graphics::SUBCATEGORIES`), so hiding Walls hides both, and `graphics` takes them
as it takes any category. A triangle's owner stays its wall, for picking and selection; which part it is goes in two bits of its skin
(`view3d::PART`), so a triangle grows by nothing and a mesh's copies stay shapes. `Look::part` holds the two rows, and `Tri::seen_in`,
`Tri::surface`, `Tri::edge` and `Tri::alpha` ask them wherever the scene asked its owner.

A part's row goes over its wall's in the 3D view: hidden, surface colour, edge colour, transparency, halftone. A plan takes a row's hidden,
line colour, weight, dash and halftone, and a cut mullion's fill; a wall's own line override is laid on after, so there it wins. Sections
and elevations take a row's hidden, and in the part of the picture beyond the line, its edge colour. Not taken yet: a row's fill patterns,
and a section's cut lines, which go by the wall.

## What it can't be yet

`curtain::check` refuses a curved curtain wall, an attached top or base, a location line other than the exterior or interior face, a
wall by face, a wall of over `MAX_CELLS` cells, and a door or window hosted directly in one.
