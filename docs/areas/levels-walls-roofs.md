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
sections and IFC all draw that outline (`walls::outline`, `walls::prisms`). Room areas are measured at the walls' centre
lines, run on or back to meet at joins (`walls::centre_line`). A room separator (`roomseparator`, made by `room_separator` a line per segment) bounds rooms as a wall's centre line
would and nothing else: it is no edge of a floor, a ceiling or a roof (`Model::bounding_lines` takes it, `wall_lines` and
`face_lines` don't; an end drawn to a wall's face runs on to its centre line there, so the line splits rooms measured at
the centre lines too), it is drawn in plan only, and it goes to IFC as an `IfcVirtualElement` of `ObjectType` `RoomSeparation`
with an `Axis` polyline on its storey, which the import reads back. With `room_area {at: "finish"}` (`Model::room_area`, Revit's Area and
Volume Computations) room areas are measured at the walls' faces instead (`face_lines` and the separators), the floor inside them. There a
wall's own faces close round its body, so a point inside a bounding wall is no room (`Model::wall_holding`): `Model::room_around` is the one
place that says so, for `room_region`, the plan's tag (NOT ENCLOSED) and the Room tool's preview; `room` refuses such a point, and `inspect`
names the wall a room left there lies in. The IFC space
takes that outline, and its `Qto_SpaceBaseQuantities` give both whichever is set, each where it is a room: `GrossFloorArea` and `GrossPerimeter` at
the centre lines, `NetFloorArea` and `NetPerimeter` at the faces, and `Height` up to the level above. Walls, slabs, doors and windows
carry theirs too (`ifc::wall_quantities`): a wall its `Length` on its axis, `Width`, `Height` and side and volume, gross and less the
doors and windows in it (each cut to its height; joins aside), or only `Length` and `Width` when attached, as its height then varies;
a level slab its `Width` (depth), `Perimeter` and area and volume less its openings, a sloped one only its `Width`; a door or window
its `Width`, `Height`, `Perimeter` and `Area`. An attached top or base follows the roof's or floor's plane
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
