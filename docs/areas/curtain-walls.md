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
file shows them. They are what lets identical parts share geometry when the 3D instancing of PR #188 arrives; today each part is
still one `Solid::Box` (`view3d::curtain_solids`), and sections and elevations read those solids.

An unedited part has no id of its own, only its `Address`: an agreed exception to the id rule, since a tower's façades would use up
the six-digit ids and every change of length would rewrite them.

## Fit

`Fit::Fixed` draws a wall in its pattern's lengths: `curtain::snap_length` gives the nearest whole number of segments, at least the
first. `wall` moves each end to it, the next wall going on from there, and says so in `adjusted`; the last wall of a closed chain still
closes it. A height set directly snaps too; one a top level sets can't, and the top row takes up the difference. The desktop's Wall
tool and an end's grip snap the same way (`BimApp::fixed_end`), and background grid snapping stays out of the way while a wall of a
Fixed Grid is drawn (`drawing_fixed_curtain`). `Fit::Flexible` keeps the length given.

Fit doesn't change what `layout` does with a length: at any length, what's left past the last whole segment is a cut segment.

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

The 3D view merges coplanar faces wherever they meet, so a joint between two mullions of one material shows no line: butt and mitre
look alike from the front. That is the view's rule for all elements, left for the 3D work of PR #188.

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

On the desktop, Tab on the selected curtain wall under the cursor steps through its mullions there, its cell, and back to the whole wall
(`parts_under`, `next_part`). A plan looks at its cut plane; the 3D view casts the cursor's ray on the grid plane (`on_plane`), which
reaches every row. Sections and elevations don't offer it yet. Properties then shows the part's type, As in the Pattern and Replace with
Wall, and Delete takes the part, never the wall.

## The desktop

- **Curtain Wall tool:** the Wall tool drawing the first curtain type, in the Build panel's list and the Tools palette (the ribbon has no
  width to spare); the Wall tool's Type Selector offers the same types. The Options Bar's Location Line is kept where it is a face.
- **Type Properties:** a curtain wall type shows Layout [Edit…] and its thickness, which can't be typed. Duplicate keeps the pattern.
- **Edit Pattern** (`crates/ui-egui/src/curtain.rs`): segments each way, fit, justification, borders, and one module to scale. Click a
  panel or a mullion, Shift to add to the selection, and pick its type, none or how it joins. Each piece is a widget named as tests
  and screen readers find it: `Panel B1`, `Vertical B1`, `Horizontal B1`.
- **Project Browser:** Families > Curtain Panels > Generic Panel and Mullions > Rectangular Mullion; a type's right-click has Edit Type.
- **The message:** `BimApp::curtain_message` looks for walls newly malformed whenever the model changes, so it comes when a change is
  finished and never during a drag, once for a wall while it stays so, one message for many walls, and not for what a project opened
  with. Remove last interior grid sets `absorb_columns` or `absorb_rows`; I will fix it myself leaves the wall, and `inspect` keeps
  warning. Agents get the same words in the `wall` reply's `warning` and in `Model::warnings`.

## What it can't be yet

`curtain::check` refuses a curved curtain wall, an attached top or base, a location line other than the exterior or interior face, a
wall by face, a wall of over `MAX_CELLS` cells, and a door or window hosted directly in one. Corner mullions are held for their own discussion.
