# Levels, walls, roofs, floors, ceilings, shafts, stairs

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

Levels have elevations, and each is a plane over its `extent` (`Model::level_extent`; without one, `Model::model_extent`):
sections and elevations draw a level over as much of its extent as they see, and the 3D view outlines it, depth-tested and
picked as the level; unselected, its two edges nearer the camera are left out. Its name goes on a `view3d::LevelHead`, an
unseen vertical plane past the right end of its far edges, where an elevation marker family will stand. Selected alone, a
level's ends in a section are grips (`Model::level_extent_along`) and so are its plane's edge middles in 3D
(`geom::edge_grips`, `Camera::on_plane`); both run `modify {extent}`.
A section or an elevation cropped shows its line's length and its crop's heights; uncropped, everything along its line
(`render::crop_region` is none, and there are no grips).
A level is also its plan: its view range (`Model::plan_range`)
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
sections and IFC all draw that outline (`walls::outline`, `walls::prisms`). A wall's `join` (on unless turned off, written in
a file only when it is off) says it joins at its ends, Revit's Allow Join: a wall refusing to join neither mitres against a
neighbour nor stretches with one whose end is dragged (`walls::arms` leaves it out of both; `relocate` passes it by), and a
footing under such a wall stops at its face as if it were a wall too. The desktop's right-click menu offers Disallow Join and
Allow Join for the walls and runs selected (`context::Action`, a `modify {join}` command).

At a square corner of two compound walls, one core runs through and the other stops on it (`walls/compound.rs`). It takes
two walls and nothing else joined there, ending at the same point square to each other, straight, on their centre lines,
unattached, with the same base and top; each type three layers, finish, structure and finish, each with a material and a
thickness and none variable; on each face of the corner the same finish material in both walls; each wall longer than twice
the thicker one's thickness; and no door or window within that thickness of the corner. Any other corner keeps the mitred
cap. The wall with the lower GUID runs through: its core reaches the far face of the other's, whose core stops at the near
face of it, and the finishes on each face meet on the diagonal, as in a mitred cap. By GUID, not id, since a GUID never
changes: a corner brought in by Insert from File, whose ids can come in the other way round, keeps the same core through.
A copy is new walls with new GUIDs, so a copied corner can run the other core through. Where the cores' materials differ the return draws a seam along its core's end; where they are the same the
core reads as one. The cap is then five points, a step round the core (`walls::cap`), and plans and sections draw each
layer to where it ends (`walls::layer_geometry`, `LayerGeometry::band`, `LayerGeometry::interfaces`).

Room areas are measured at the walls' centre
lines, run on or back to meet at joins (`walls::centre_line`). A room separator (`roomseparator`, made by `room_separator` a line per segment) bounds rooms as a wall's centre line
would and nothing else: it is no edge of a floor, a ceiling or a roof (`Model::bounding_lines` takes it, `wall_lines` and
`face_lines` don't; an end drawn to a wall's face runs on to its centre line there, so the line splits rooms measured at
the centre lines too), it is drawn in plan only, and it goes to IFC as an `IfcVirtualElement` of `ObjectType` `RoomSeparation`
with an `Axis` polyline on its storey, which the import reads back. The desktop's Room Separator, beside Room, lays one a
click from the point before until Esc. A room's height is Revit's: from `base_offset` over its level up to `limit_offset` over
its Upper Limit, `top_level` (none: Not Bounded, the offset then over its own level). A new room reaches 300 mm under the
level above (`ROOM_BELOW_ABOVE`), or with none above is Not Bounded and 2500 mm tall; a room read from a file without them is
Not Bounded, 2500 mm. `Model::room_span` and `room_height` give it, `elements` its `unbounded_height` and `volume_m3` (the area times that height, not cut by floors or roofs), and the IFC
space is swept that tall. A room's colour on plans (`Room::colour`) styles that solid in IFC as other elements' colours do: an
`IfcStyledItem` on the swept solid, its unnamed `IfcSurfaceStyle` shading in the colour (`IfcSurfaceStyleShading`, which IFC4's
rendering style extends; `Step::surface_style`, which a material's style shares), so a viewer shades the space in it. The import
reads it back from the colour the space's body is styled in (`import::item_colours`), so a round trip keeps it and a space
another program styled comes in filled in its colour; a room that isn't enclosed has no body, so no colour, in IFC. A frozen room (`frozen`, an instance parameter: `room {frozen}` or `modify {id, frozen}`) keeps the
boundary it has when frozen, both ways it can be measured (`Room::kept`, given after the command that froze it by
`Model::keep_rooms`, which the engine calls after every command, and let go when it is thawed): `room_region_at` returns it
and the plan draws it without finding the level's lines, so the walls are never searched for it again, which saves the time
of many rooms; moved, rotated or mirrored, its boundary goes with it, and only a bounded room can be frozen. Walls and columns (and structural columns) bound
rooms while their Room Bounding is on (`Wall::room_bounding`, `Column::room_bounding`, on unless set; `Model::room_bounding`,
`bounding_columns`, `room_lines`): a column's outline either way rooms are measured, a free one an island. A wall with it off
still bounds floors and roofs picked by their walls. Walls and columns bound a level's rooms where its
Computation Height (`Level::computation_height`, 1200 mm unless set) cuts them; room separation lines bound them whatever
it is. Two rooms can't share a space (`Model::room_bound`, `Unbound`): the one placed first (the lower id) keeps it, the
other is unbounded, and `room` and `inspect` warn. A room can be placed where nothing encloses it: unbounded, it covers a
`UNBOUNDED_ROOM` square round its point (`Model::room_outline`), as Revit's. A plan labels it NOT ENCLOSED, or REDUNDANT where another
room has its space. Its reference, a cross from the point it was placed at out along the diagonals to its boundary
(`geom::ray_hit`), drawn as the room's own lines (a click on it picks the room), is the subcategory `roomreference` under
Rooms, off unless shown (`graphics {show: ["roomreference"]}`), beside `roomseparator`, the separation lines.
Subcategories are general (`graphics::SUBCATEGORIES`: a key, its category, a name, whether it is off unless shown):
a view holds them as categories, with `Override::shown` for the ones off unless shown, `Graphics::hides` answers for
them (a category hidden hides its subcategories), and Visibility/Graphics folds them under their category. In a 3D
view rooms are off until shown (`graphics::OFF_IN_3D`, `Graphics::shows_in_3d`; `graphics {view: "{3D}", show: [rooms]}`,
or ticked in its Visibility/Graphics), every new 3D view so; shown, each is drawn as below, fainter than a selected one,
and a selected room is seen whatever the view says: a see-through solid in the selection's colour from its bottom to its top
(`orbit::room_solid`, drawn over the picture, so it shows through walls). With `room_area {at: "finish"}` (`Model::room_area`, Revit's Area and
Volume Computations) room areas are measured at the walls' faces instead (`face_lines` and the separators), the floor inside them; the IFC space
takes that outline, and its `Qto_SpaceBaseQuantities` give both whichever is set: `GrossFloorArea` and `GrossPerimeter` at
the centre lines, `NetFloorArea` and `NetPerimeter` at the faces, `Height` (the room's Unbounded Height) and `GrossVolume` and `NetVolume`. Walls, slabs, doors and windows
carry theirs too (`ifc::wall_quantities`): a wall its `Length` on its axis, `Width`, `Height` and side and volume, gross and less the
doors and windows in it (each cut to its height; joins aside), or only `Length` and `Width` when attached, as its height then varies;
a level slab its `Width` (depth), `Perimeter` and area and volume less its openings, a sloped one only its `Width`; a door or window
its `Width`, `Height`, `Perimeter` and `Area`. An attached top or base follows the roof's or floor's plane
over each part of the wall (a roof's slopes each over its own part of the plan); a mirrored wall runs back the other way.
A wall's top is `Model::own_top`, and 3D, sections, plans, IFC and what a wall is attached to all read it there (`walls::heights`
takes it, or what the wall is attached to): its `top_level`'s elevation plus its `top_offset` (file format 15, Revit's Top Offset,
mm, negative below the level), or its base plus its `height` when it has no top level, which an offset does not touch.
`Model::check` refuses a wall whose top does not come out above its base, saying how tall it comes out and by how much to raise it.
The Properties palette shows Top Offset under Top Constraint for a wall that has one.
A door or a window stands in its wall from its `sill` up its type's height (issue #215): `Model::check` keeps its width within the
wall's length, its sill at or over the wall's base and under its top all across it (`walls::top_at`, which a roof the wall is
attached to may raise; within `TOL` of the top is at it), and the openings in one wall apart; its head may rise past the top, as
Revit lets it. The wall is then cut up to its top, a notch open there: `walls::prisms` leaves no part over the opening, so 3D,
sections and elevations draw the notch, and the door or window at its full height, past the top. A plan cutting through the
wall opens it at the opening as ever; one cutting over the wall's top cuts neither, and sees the notch's sides across the wall
from above (`walls::notches`). IFC writes the opening's void at its full height (a boolean takes off only what is in the wall)
and the door or window at its own size, and the wall's net quantities take off only the part of each opening in the wall; the
IFC import brings such an opening in, and leaves out with a note naming the file's wall one whose sill is at or over its top.
A roof by footprint is the straight skeleton of its outline and its `courtyards` (`skeleton::faces`: the outline
counter-clockwise, the courtyards clockwise, the roof to the left of every edge): each edge moves in at speed 1, a gable's at
0, and its slope rises from it, reaching the roof's base `overhang` in from its eaves; a courtyard's edges number on after
the outline's, and every ring keeps its numbers whichever way round it runs, so `gables` stay put through a mirror.
`Roof::slopes` (file format 15) gives each edge a slope of its own, in degrees, numbered as `gables` are: 0 is the roof's `slope`,
and an empty list, every edge. `Model::roof_faces` then moves an edge in at the run over the rise of its slope, scaled so the
shallowest moves at 1 (every edge at one slope still moves at exactly 1), so edges of different slopes reach the same height
together and the slopes meet along their hips; the eaves stay level, the roof's base `overhang` in from them lowered by the roof's
`slope`, whatever the edges' own. A gable stays upright whatever its slope, and a flat roof (`slope` 0) keeps its `slopes` as it keeps
its `gables`, unused. `Model::check` refuses a list of the wrong length, saying how many edges there are, and a slope outside 0
to 75°, saying which edge. A sketch line may carry a `slope` (`SketchLine::slope`; the Slope row of Properties, with one line of a
roof's sketch picked, sets it): the roof keeps it as the `slopes` of the edges along that line (`sketch::along`), never in its
`sketch`, which `sketch::check` refuses to hold one. `edit_boundary` keeps the slope of an edge that stays or that a new
line cuts in two (an edge lying along an old one), the lines' own `slope` (0 sets the roof's) winning over it, and without
`slope` on the lines it keeps what the edges had; `edit_boundary` without a sketch gives the lines with the slope their edges share.
Over the walls (`floor_outline`, which `roof_footprint` grows by the overhang), a loop of them standing free inside the outer
ones, joined to none of them and with no room in it, is a courtyard, open through the floor and the roof; one joined to them,
or with a room in it, is rooms, and both run over it.
`Model::roof_faces` are its slopes, each the loops round it in plan (a slope can close round a courtyard whose far side is a
gable) with the plane of its underside, trimmed where it `join`s another; `Model::roof_pieces` are those less their holes
and its `openings`, convex where cut. A floor's top is at its level plus `offset`,
or with a slope arrow, there at the arrow's tail and rising `slope` degrees toward its head (`Model::floor_top`); it's its
type's thickness across the slope, so plumb it's thicker. Its `openings` are holes inside it, apart, and `Model::floor_slabs`
what's left, convex where cut. A floor's `shape` (IDEAS MOD-16, [design](../shape-editing.md)) folds its top into planes:
points `z` mm above (negative, below) where the top would be without them, and split lines it folds along, each a part
with an id and a GUID (`ShapePoint`, `SplitLine`). `Model::shape_planes` triangulates its outline, openings, points and
lines (`triangulation::constrained`), lifts each corner by the point there (a corner with none stays at 0) and merges
neighbours in one plane; `Model::floor_faces` are those planes in convex pieces, or the unshaped top, and `floor_slabs`,
the 3D view, sections and the element's details take them. In IFC a shaped slab is one closed `IfcFacetedBrep`
(`Model::shape_solid`: its top's planes, the same planes under them, plumb sides round its outline and openings, each
corner written once), and a sloped or shaped slab gives `GrossVolume` and `NetVolume`, its plan through its depth plumb;
every floor's details give `volume_m3`, what's left of it in plan (shafts too) through that depth. A shaped floor is its type's thickness plumb, so its planes meet
underneath too, and has no slope arrow (`Model::check`, `shape_ok`, refuses one, and points outside the outline or in an
opening, points within a millimetre of each other, a split line's end that is no point and no corner, and lines that
cross). A wall attached to one follows `Model::shape_triangles`, its top over the whole outline as if its openings were
closed, as a slope arrow's plane runs on over them. Plans draw each plane's edges as thin as the outline. `edit_shape`
gives the shape anew or says what it is, `add_point`, `add_split_line` and `reset_shape` change it a piece at a time (an
end of a line that is no point becomes one, at the height the top has there), `edit_boundary` keeps what stays inside the
new outline and its openings, cuts a split line where it leaves them, and gives each new end and each new corner a point at
the height the top had there, so a hole cut through a shaped top leaves it as it was round the hole; it names what it
`dropped`, `cut` and `added`. Moves and mirrors take the shape along, and snaps find its points. A flat roof by
footprint (`slope` 0, no extrusion, joining none) takes a shape too (`Roof::shape`, `Roof::unshapeable` says why another
can't): the commands, checks and both views' tools see its top as a floor's (`Model::shapeable`, `Model::roof_as_floor`:
its footprint, its courtyards and openings as openings, at its top), and `roof_faces` lays each plane's underside its
thickness below, plumb (`roof_rise`), so its slabs, IFC, sections and the walls attached under it follow its falls; plans
draw its creases thin. Under its type's variable layer a shaped roof's underside stays level at its base
(`Model::roof_under`, `roof_prisms`), as a floor's does: its layers in 3D and sections share `Variable::bounds` through
`Model::variable_under`, walls attached under it stop at that underside, IFC writes it as one closed faceted Brep
(`Model::roof_solid`), and `Model::check` refuses a point that leaves the layer thinner than nothing. `falls` (`falls.rs`) gives a floor or a flat roof its shape from its drains and the least fall:
catchments nearest each drain, ridges between them at the top's height, valleys from each drain to its catchment's corners,
each drain as deep as the fall takes its farthest corner; round an opening the valleys turn at its corners, which lie on
the drain's cone, and ridges drawn part the top first. A slab or flat roof whose top isn't one plane comes in from IFC
shaped (`import::shaped_top`), its variable layer marked by its `Description` or, under a level underside, guessed
(`import::guess_variable`); and a spot slope (`Spot::slope`) reads a floor's or roof's fall in a plan. A layer of a floor or roof type may be `variable` (one a type at most; never a wall's or a ceiling's): under a
shaped floor of that type, the underside stays level where the unshaped floor's was (`Model::floor_under`), the layers
above the variable one follow the top, those below it stay level, and the variable one takes up the difference
(`Variable::bounds`, which the 3D view's layers and sections share). Its pieces are then prisms from that level underside
(`Model::floor_prisms`), its volume is less where its top falls (`Model::floor_volume`, `model::volume` for IFC), a wall
attached under it stops at the level underside, and `Model::check` refuses a point that would leave the variable layer
thinner than nothing, saying by how much to raise it. Reinforcement arms a shaped floor only when a variable layer lies
over all its structure: the bars keep to the layers under it, as on a level floor that thick. Without one it refuses,
saying why. In the desktop, a floor selected shows Shape Editing in its contextual tab: Slope Arrow, Modify Sub Elements (a point, a corner or an edge's two ends,
`Floor::shape_pick`, to the Options Bar's Elevation), Add Point (a
click on the top, at the Options Bar's Elevation, above the unshaped top or, with Absolute, above the level; empty, the
top's height there), Add Split Line (two clicks, snapping to points, corners and edges) and Reset Shape, each a command.
While one is on, each point's height is labelled beside it (`Model::shape_labels`); under Modify Sub Elements a point's
grip drags it in plan at its height, its split lines with it (`Shape::moved`), in one `edit_shape`. In the 3D view the
same tools work on the floor's top: each point's and corner's grip drags its height up or down, and clicks land on the top
(`view3d::shape_handles`, `on_floor_top`, `shape_pick_3d`); both views run `shape.rs`. While a grip is held, either
view draws the creases its release will leave (`Model::shape_creases` of the floor with the shape it would have). Type Properties ticks a floor's or a roof's variable layer. A ceiling's `offset` is its underside; its `openings` are holes as a floor's
are, and `Model::ceiling_slabs` what's left. A shaft cuts each floor,
ceiling and roof its span reaches into over its outline, all the way through (`Model::less_shafts`). A stair or a ramp
is runs and landings laid out from its start (`stairs::stair`, `stairs::ramp`): a landing after `at` risers (a ramp's, `at`
mm up), as deep as the runs are wide, the next run turning by its `turn`; a railing on one follows its walk line moved out to
its side (`stairs::railing`), so it goes wherever its host goes. A stair's `start` is on its location line
(`line`: its left edge, its middle or its right edge, and `offset` from it; `Stair::foot` is the middle of its first riser), so
the line stays when the width changes. A turn is straight on, back beside the run, or any angle from 5 to 135 degrees (the runs then share the turn's inside
corner). A landing has a `length` of its own (0: as deep as the stair is wide), and a turn may
wind instead (`winders` treads fanned round the turn's inside corner, `stairs::wind`; they are `Layout::pads` too, the last
`Layout::winders` of a turn's). With `balance`, the winders run that many treads into the flight on each side and all of the
turn's steps are equal along the walk line and the inside string (`stairs::balanced`), so none comes to a point. A `multistorey` stair with a top level several levels up is the stair of its first storey (`Model::flight`, `stairs::first`)
standing again on each level on the way (`Model::storeys`, `stairs::storeys`; `stairs::stair` is all of them), each storey with
its own count of risers. A stair's type (`Model::types.stair`, `StairType`; none for a stair solid to its level) is what it is built of: `stairs::parts`
gives its solids, a slab of the type's `waist` under the steps or treads, risers and stringers standing free. A stair with a `sketch` is drawn by hand: a line for each riser and no runs (`stairs::sketched`), each tread the ground to the
next riser or its own outline; `stair_sketch` (`stairs::as_sketch`) makes one of a stair made by runs and turns. A stair with a `spiral` (`radius` to its middle line, signed for the side its centre is on, and `sweep`) has no runs: its
treads are pieces of a ring (`stairs::spiral`), all of them `Layout::pads`. A stair type's `construction` (`Construction`: monolithic, stringers, a centre
stringer or treads fixed in a wall; none, read from its sizes by `StairType::built`) says what carries its treads, and
`landing_stringers` runs a stringed stair's stringers round its landings' free edges; `stairs::parts_tagged` names each solid's
part (`graphics::StairPart`: runs, risers, landings, supports, finishes, headroom), which a view's stair subcategories hide and
override (Visibility/Graphics, `graphics::STAIR_SUBS`; Headroom, `stairs::clearance`, off until shown). A finish's front edge
takes the type's `finish_profile` (`stairs::profiled`); its plan symbol, its `arrow`, `walk_start`, `cut_mark` and
`walk_rounded`; a plan view may show every stair its own way (`stair_display`, `Graphics::stairs`). A stair's plan edges are
references a dimension measures to (`Face::Edge`, `stairs::reference_edges`). IFC writes a stair's flights, landings, stringers
(`IfcMember .STRINGER.`) and finishes as its type builds them, with `Pset_StairCommon`, `Pset_StairFlightCommon`, the flights'
lengths and `BIMCraft_Stair` (the stair and its type as JSON), which an import makes a stair again. `stair_storeys` makes a
multistorey stair a stair for each storey; `stair_sketch {back}` makes a converted stair the stair it was (`Stair::unsketched`).
A project's **building code** is the rules its stairs are held to (the codes, their sources, a project's own, XML, and the
rule that every check is fast and spatial: [building codes](building-codes.md)): a new stair starts with its riser, going and
landing length, `landings: "auto"` places the landings due, and `stair_check` (and `code_check`, `code_findings` on a stair)
lists what a stair is outside of, with the number and the fix.
`flip` on a stair makes it climb the other way over the same ground (`stairs::flipped`). A stair's grips, where they are and
what a drag of one makes of the stair, are `stairs::grips` and `stairs::dragged`; the desktop's `stairgrips.rs` draws them on
the plan and runs `modify` on release.
`stairs::headroom` finds the floors standing over a stair's steps with less than the code's headroom, which `stair_check` reports
and `stair_opening` cuts open with a shaft through each (`stairs::united` is the outline round the steps). `stair_solve` (`codes::solve`) proposes the stairs nearest one that fit the rules, least change first, and makes one with `apply`.
Nothing is ever refused for breaking a rule: winders under a
code without them are built and reported. The desktop's is Manage > Building Codes (`ui-egui/src/codes.rs`), apart from
Technical Standards, which is about drafting. Where stairs are going, and why: [an advanced stair
tool](../stair-tool-plan.md). `modify` merges into fields that are objects (JSON Merge Patch), so `{"view_range": {"cut": 1500}}` keeps the rest; an element's own field can't be
called `category`, the tag every element carries.

Sketches (`sketch.rs`) are how an outline is drawn, the one solver everything with an outline and holes uses: floors,
ceilings, roofs by footprint, shafts and filled regions now, topography next. A sketch is `SketchLine`s, straight or
arcs (a point `through`), in any order, either way round; `sketch::solve` cuts arcs into chords no more than 5° round,
welds ends within `TOL`, then names every problem at once with the lines it's about, what to fix first first
(`Problem::rank`: a line too short or out of range; two lines crossing, or one ending part way along another; two along
each other; three or more at a point; an end no other meets; a loop with no area), and otherwise walks the loops and nests
them by how many others each is inside: even, an outline (counter-clockwise), odd, a hole (clockwise) of the innermost loop
round it, so an island in a hole is an outline again. `solve_one` wants one outline (a floor's, a roof's). The element
keeps the sketch it was drawn or last edited by (`sketch` on floors, ceilings, roofs and shafts; empty when it was made
over the walls or read from a file): each line is a part with an id and a GUID of its own (`HasPart`, listed in
`parts_now` by the element's GUID), going with the element when it moves and new on a copy; part ids are not given
again once a line is gone, for as long as the project is open (the library remembers the lowest it has held; a file holds the
ids in use, so one a line had before it was deleted and the file saved may be given again after the file is reopened). Commands take `sketch` (lines `[[x,y],[x,y]]`, arcs `[[x,y],[x,y],[x,y]]`, or
`{start, end, through?, id?}`) in place of `boundary` and its holes; `edit_boundary` without one gives the lines to edit
(the kept sketch while it still draws the outline, `sketch::draws`, else the outline's edges as new lines) and with one
redraws it, a line sent back with its `id` keeping it (keeping a roof's gables on edges that stay); the `sketch` command
solves without making anything. Messages name a line by its id, or its place in the list while it has none. A boundary
given as points is cleaned (a repeated point dropped, and one within a millimetre of the last), and every outline a command
makes or changes, whichever command (`floor`, `modify`, `edit_boundary`, `move`…), is judged as the sketch of its edges
(`Model::check_edits`, asked by the engine after each command but Insert from File): a floor's, ceiling's or roof's boundary,
an opening or a courtyard, a shaft's boundary, that crosses itself or lies along itself, has an edge ending part way along
another, or a spike or keyhole slit, is refused, saying which edges and where, and what to do: redraw it with `edit_boundary`; give
`boundary` or `sketch` (with the walls that touch at a corner named only when it was taken from walls), or simple `openings` or
`courtyards` for one of those rings, for one a command was making;
redraw the mass whose outline it is, for one made by face or by `mass_floors` (a mass itself may still have a bow tie outline,
a spike or a corner touching an edge, so the message says the outline isn't simple, not that it crosses itself);
and, for a roof's opening, which `edit_boundary` does not edit, give `modify` a simple `openings`, and from `dormer` change the
dormer's width, height or position. That is a change from the base, which let
`modify` and the commands that weren't given points set such an outline. `Model::check` doesn't judge the whole project by
it: a file, an IFC import or a Revit import with a keyhole outline still opens, and what leaves its outline alone (`modify`
of its offset, `delete`, `edit_boundary` with a sketch that is simple) still works, and so does what moves, copies, turns or mirrors
it, which makes it no worse (a copy is judged against the elements the command was given, so a new floor drawn with a bow tie
the file also holds is refused); what changes its shape is asked to redraw it first. One line that crosses itself (a closed polyline round a bow tie, a spline with a loop) is found as two lines
crossing are, and a sketch is refused at once past 200,000 straight pieces (a circle is up to 8,193 of them), or when its
pieces lie so close together that telling which cross would take six million steps.

The desktop's sketch mode (`crates/ui-egui/src/sketch.rs`) holds the lines until Finish Edit Mode, which solves them
first and says what's wrong for whoever drew it (`sketch::friendly`: what to fix, kind by kind, where, in the project's
units, and the lines by id) with those lines in the error colour; solved, it runs the command. Draw has every shape of
the Draw panel (`draw.rs`; lines, rectangles, polygons, circles, the arcs, Fillet Arc on two sketch lines, splines, ellipses,
polylines, Pick Lines: see [views and graphics](views-and-graphics.md)) and Pick Walls (a line along each wall's location
line, an arc along an arc wall); a sketch line is any curve, so a circle or an ellipse is a loop of its own. Modify picks
sketch lines (a click, a window box from the left, a crossing box from the right), a picked line's handles (its corners, a
spline's points, a circle's centre) are grips that take every one joined there with them, and Delete removes them; Ctrl+Z takes back a line; the plan's snaps take the
sketch's own ends, middles, crossings and curves as well as visible view-specific detail (`snap::in_sketch`). Floor, Ceiling, Roof and Shaft each start a new one (Boundary
Line, Sketch Ceiling); Edit Boundary redraws one selected. The Mode panel (Finish, Cancel; Edit Boundary) stands at the
ribbon's right end.

What the line tools hold to, and don't (`curves.rs`, `draw.rs`, `sketch.rs`). A curve is drawn as chords none further than `FLAT`
(0.25 mm) from it: a polyline's arcs, a circle, a spline and a fitted spline (a span of a spline is cut by the greatest
second derivative on it, so a ten metre arch is hundreds of chords), up to `MAX_DRAWN` extra points in all; a polyline whose
arcs would take more is refused, and a circle past about 3.4 km of radius (`MAX_DRAWN` chords stray `FLAT` from it there) is drawn coarser than `FLAT` rather than in more, 0.37 mm off at 5 km and 2 mm off at 27 km.
An arc, a circle or an ellipse reaches no further than `MAX_COORD` from the origin, as a wall's arc must; a spline lies inside
its control points, and a fitted spline is held to its points only, a cubic through them straying a little past. A curve's
start and end are the points it was given. Offset moves a polyline's lines along themselves and its arcs about their own
centres and makes each corner where the moved sides cross; it gives nothing where a side would vanish or turn round, or a
corner is so sharp its mitre would be 50 offsets long, and it does not trim a fold it makes in a concave shape (Revit's
does); an ellipse or a spline offsets to a fitted spline within 0.05 mm. Fillet on walls keeps their doors and windows
where they stand and refuses to cut one off, but does not stretch a third wall that ended on the corner. A detail line with
no style gets Thin Lines by the project's name, else the English one, else the thin black solid style. Of the line tools,
Revit has default shortcuts for Trim/Extend to Corner (TR), Offset (OF), Fillet (FL), Split (SL) and Detail Line (DL), which
`corner`, `offset`, `fillet`, `split` and `detail_line` have; it has none for Trim/Extend to a boundary, vertices, Convert
Spline, Edit Boundary, line patterns and styles or fill patterns, which have none here. Explode Polyline is EX, ours (PX is
the desktop's Plumbing Fixture).
