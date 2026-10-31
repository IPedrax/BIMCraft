# Views: scale, detail and style, and visibility and graphics

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

A view's scale, detail level and visual style are in its `Graphics` (`scale`, `detail`, `style`; the `scale`, `detail` and `visual_style`
commands, which the view control bar's menus, Properties' View Scale, Scale Value 1:, Detail Level and Visual Style, and WF, HL and SD run). A
view template controls them as groups of their own (`Group::Scale`, `Detail`, `Style`, in `Graphics::resolved`, `check_followed` and the three
commands, which refuse what a template controls); `render::scale_of`, `detail_of` and `Look::with` read the value in force, never the view's own, and
the bar and Properties show it greyed out, the template named on hover (`controlled_note`). A scale is the n of 1 : n (`graphics::SCALES`);
`render::annotation` turns it and the zoom into `Drawing::k`, the factor annotation is drawn at (`Drawing::px`, `Drawing::grow`: annotation is
laid out one pixel to the pixel and grown about its anchor), and 1 on a sheet (`draw_at`, `section_at`), where the viewport's scale already
sets paper size. Every plan, section and elevation has a scale: one with none of its own is at its project units' default,
1 : 100 or 1/8" = 1'-0" (`Units::default_scale`, `Graphics::scale_n`, `graphics::scale_of`), and `scale` refuses null. A viewport
placed with no scale takes the view's. The units name the scales and list the ones offered (`Units::scale`, `scales`,
`parse_scale`): metric 1 : n; in inches or feet the architect's (12" = 1'-0" to 1/32" = 1'-0") and the engineer's (1" = 10'-0" to
1" = 200'-0"), and 1 : n for any other. The Scale menu, Properties, templates and sheets' titles name them so, and a scale is typed
as n, 1 : n, or two lengths (`1/4" = 1'-0"`).

The view control bar's High Contrast Point Clouds (`footer.rs`, wherever the project or a link holds a scan, in a plan, a section, an elevation
and the 3D view) fills the view's `pointcloud` category with one colour, the user's, in one `graphics` command; how it behaves is in
[point clouds](point-clouds.md).

Paper space and model space. A view's geometry is model millimetres; its annotation and its line weights are millimetres of the paper the
view stands for, 1 : n of the model. A sheet is that paper ([`sheet::DENSITY`] px to the mm). On screen, or in an image of a view, `k`
says how big the paper is at the zoom: a paper mm is `DENSITY * k` px. A line's weight is a pen (`pen.rs`): 1 to 16, each a width on
paper at 1 : 100 from 0.10 to 2 mm (our own steps, not Revit's: what's seen, pen 4, 0.18 mm, and what's cut, pen 10, 0.35 mm), and 0, a hairline, which every output draws as thin as it can whatever the scale
and the zoom (a width of 0 in the display list: tiny-skia's hairline, PDF's `0 w`, an SVG pixel with `vector-effect="non-scaling-stroke"`,
a pixel on screen). What a pen draws is the project's line weights (`pen::LineWeights`, `Model::line_weights`, the `line_weights`
command, Manage > Line Weights in the desktop, `lineweights.rs`): the model's pens 1 to 16 at each of a list of scales, a view taking
the column of the scale nearest its own (`LineWeights::at`), and the annotation's at any scale. A project that hasn't set them has
our own start for its units (`default_for`: metric or imperial columns, the model's pens wider at finer scales, by the fourth root,
and at 1 : 100 the pens' own widths, `Pen::mm`). A drawing keeps each line's pen (`Drawing::pens`, `stroke`), an override sets the pen
(`Look::dress`), and the last step of drawing a plan, a section, a sketch or a sheet's own lines sizes each line (`Drawing::weigh`),
as annotation where its owner is a dimension, tag, text, spot elevation, grid, level, room or mark (`annotates`), else as the model;
a view on a sheet takes the line weights at its viewport's scale. Cut outlines are inked over the poché: `cut_edges` keeps of each
outline what isn't against another's on its outer side, so joins stay clean. Where two runs meet (two walls at a corner), `turn_corners` carries each a pixel on round the corner into the other, so the corner is a join inside a line, mitred square, not two ends with a notch between them; an outline nothing lies against is one closed line (`ink_cut`), no corner of it an end. PNG, SVG and PDF join a pen's lines mitred and lettering's round (`Part::mitred`), so a V's point doesn't stick out. A pen is as wide as on the paper however far the view is zoomed (`paper_of`, not clamped as lettering's size is), and a tool's preview draws its pens as the plan it is in does. The desktop draws a line its pixels at the zoom, at least one (`canvas::on_screen`); Thin Lines draws every line one. A
file from before pens (format 3) held weights in pixels of a sheet's drawing, and opens with each the nearest pen (`pens_from_pixels`).
Which pen each kind of line draws with is named once, in `pen.rs` (`Pen::CUT`, `Pen::PROJECTION`, `Pen::HEAVY` and the rest), never a number at the draw site: a cut is `Pen::CUT` in plan and in section alike. A category's own lines are its Object Styles'
(`styles.rs`, `Model::object_styles`, the `object_styles` command, Manage > Object Styles in the desktop, `objectstyles.rs`): the
draw site marks them `BY_CATEGORY`, and `Drawing::by_category` gives each the cut or the projection pen of its owner's category, by
its `Part`, before `Look::dress` puts a view's overrides over them. Marked: what's cut (`cut_edges`, plan and section), what's
seen (floors, walls and columns below, roofs, ceilings, runs, components, shafts, masses, stairs' and ramps' edges, railings), a
door's or window's family forms and symbols, a door's leaf and swing. The rest keep their own pens, as Revit's subcategories
would: a compound element's layer lines, an opening's jambs and glass, a stair's cut line and walk line, a floor's holes. A project
holds only the categories set apart from `styles::default_of` (which draws as before: cut pen 10, seen pen 4). What's next on this
foundation: annotation categories' object styles and subcategories, viewports as windows onto their views, and printing.

Detail lines and their line styles (`lines.rs`): a `LinePattern` is a freely ordered sequence of 1–32 graphic segments, each length mm
on paper, so it reads the same at any scale (`Solid` is none, a name no pattern takes); a `LineStyle` is a pen, a colour and a
pattern; both are library entries (`Kind::LinePattern`, `Kind::LineStyle`), so a style names its pattern and a detail line its style
by GUID in a file, and a rename (`rename_entry`) follows everywhere. A project starts with ours, named in its language
(`lines::patterns`, `lines::styles`); one in use can't be deleted. A `DetailLine` (category `line`, Lines in Visibility/Graphics) is
drawn in one view, as a text note is: a level's plan (not its ceiling plan), a section or an elevation, or a sheet, in the view's own
terms, and goes with it. `render::detail_line` draws it in its style's pen with the model's line weights, its colour and its
pattern's dashes in pixels at the view's paper (`LinePattern::dash`); a repeat under two pixels draws solid. A primitive's dash is
any list of on and off lengths (`Prim::Line::dash`), which SVG, PDF, the PNG and the canvas all take. The desktop has Annotate >
Detail Line (DL; a line a click, as walls are, chained until Enter or the first point again; the Options Bar's Line Style and
Chain; the Draw panel's other shapes, below) and Manage > Line Styles (`linestyles.rs`: styles and patterns on two tabs of one
draft, applied as one undo step). Patterns > New Pattern or Edit opens the separate Line Pattern Creator
(`linestyles/creator.rs`), with a live straight/curved preview and segment controls, add, reorder and delete. Save Pattern
updates the library draft; Cancel Pattern discards just the creator, and the parent Cancel discards the whole draft.
Segments are Gap (`space`), Straight (`dash`), Dot (`dot_length`, its reserved length, with a circle exactly the pen width),
Offset (`length`, `count` 2–9, `distance` between adjacent lines symmetrically about the centerline), Wave and Zig Zag
(`length`, `frequency` in cycles per segment, `amplitude` from centerline), and Text (`length`, `size`, `text`). Text uses
bundled IBM Plex Sans on Linux, Windows and macOS, with the annotation font fallback for missing characters. Its size is
maximum capital height; it scales uniformly down when its reserved length is shorter than the text. Text is centered on
the axis and each glyph rotates with its own curve tangent. `lines/pattern.rs` makes shared stroke/fill geometry for
preview, canvas, PNG, SVG and PDF; old alternating dash/space/dot patterns retain native dashes. On canvas and paper,
pattern lettering uses the same non-zero contour tessellation as notes, keeping counters transparent during selection
and hover as well as ordinary drawing; dots retain their pen-sized polygon rendering. Tiny or excessively dense
patterns draw solid with bounded work. Region profiles, snapping, alignment and dimensions use the original centerline.
Format 9 adds these segments; older dash/space/dot patterns open unchanged. Next: patterns made from lines drawn in a view.

A detail line and a sketch line are any `curves::Curve`: a polyline (straight segments and arcs, each arc's turn in degrees
counter-clockwise as a wall's; a line or an arc is one of two points), a circle, an ellipse whole or partial, a spline by its
control points (a clamped cubic B-spline, uniform knots unless it holds its own), or a spline through points (a cubic by the
distance along them, each end free or leaving along a tangent). In a file and in a command a curve is flat, the fields of its
shape (`curves::Fields`; `curve_struct!` makes a type that holds one among its own fields, since serde's `flatten` won't take
`deny_unknown_fields`), so an older file's `points` and `closed` read as they did. `Curve::points` draws it with chords no
further than `FLAT` from it; `nearest`, `split`, `map` (a move, a turn or a mirror), `handles` and `moved` (grips) and
`check` are the rest. Splitting is exact but for a spline through points: a polyline's segment or arc divides, an ellipse's
span does, and a control-point spline gets the knot inserted to the degree's multiplicity (Boehm's algorithm) so its halves'
control points are the original's; a spline through points becomes two through the original's points on each side and the
cut, leaving it along the original's tangent, with points of the original added where it strays most until it is within
`FIT_TOL` (QCAD turns the halves into control-point splines instead; Revit won't split a spline at all). A closed curve opens
where it's cut. Commands: `split` (detail lines as walls), `join_polyline` (PJ), `explode` (EX), `fillet` (FL), `corner`
(TR, two detail lines as two walls), `trim_extend` (detail lines to a boundary: Revit's Trim/Extend Single and Multiple
Elements) and `offset` (OF; `draw::offset` keeps arcs concentric and mitres a polyline's corners). `draw::meet` finds where two
lines or arcs cross, run on along their lines or circles, and `draw::trim_to` brings one to it, keeping the part picked.
`draw.rs` is Revit's Draw panel as data: each `Shape` (Line, Rectangle, Inscribed and Circumscribed Polygon, Circle,
Start-End-Radius, Center-ends and Tangent End Arc, Fillet Arc, Spline, Spline Through Points, Ellipse, Partial Ellipse,
Polyline, Pick Lines), how many clicks it takes, what `make` builds from them with the Options Bar's Sides, Radius and Offset,
and the `preview` with the cursor as the next point; `wall_segments` turns lines, arcs and circles into walls and
`element_curve` is what Pick Lines copies. The desktop's `draw.rs` keeps each tool's shape (detail lines, sketches, walls; the
walls' panel offers only lines and arcs), gathers the points in the tool, and makes the lines, sketch lines or walls as one undo
step; Join into Polyline and Explode Polyline sit on Modify | Lines and work on picked sketch lines too. A detail line
selected alone shows its handles as grips (corners, control points, points passed through, a circle's or an ellipse's centre;
a spline's frame dashed), dragged and snapped, then `modify` with the moved curve's fields (the other shapes' set to null).
Modify's Split, Trim/Extend to Corner, Trim/Extend Single and Multiple Elements and Offset take detail lines as they take
walls, and while sketching they change the sketch's lines (`sketch_modify`).

Vertices come and go (`vertex`; Modify | Lines' Add Vertex and Delete Vertex, on sketch lines too): `Curve::with_vertex`
cuts a polyline's segment, inserts a knot in a control-point spline (one control point more, the curve exactly as it was)
or a point in a spline through points; `without_vertex` takes a corner, a control point or a point out. `convert_spline`
(Convert Spline) turns a spline through points into the same curve by control points, exactly (`fit_to_control`: knots at
the points' distances along, a tridiagonal system for the control points with the fitted spline's end conditions), and
one by control points into one through points within `FIT_TOL` (`refit_with`); Revit converts neither way.

Revit's Insulation is a detail line with an `insulation` width: `DetailLine::drawn` gives `lines::batt`, a prolate cycloid
along its curve (loops the whole width across, `BULGE` times it apart), so it splits, offsets, joins and takes grips as any
line does, and follows arcs and splines, which Revit's doesn't. Annotate > Detail > Insulation draws it, its Width on the
Options Bar; Properties shows Insulation Width. A revision cloud is the same idea: a detail line with a `cloud` arc length (and
a `revision`, any text) that `drawn` turns into scallops (`lines::cloud`, arcs of `SCALLOP` degrees bulging out of a loop
whichever way round it was drawn, to the left of an open line). A scallop ends at each corner where the way turns by more
than about 25 degrees (so a rectangle's scallops never cut a corner) and, past 1,024 scallops or more corners than that
allows, the scallops are even and fewer, each drawn within its share of `MAX_DRAWN` points. A line is a cloud or insulation
(`DetailLine::check` refuses both). `revision` is a label: it is kept with the cloud and `elements` lists it, and no
schedule reads it yet. Annotate > Detail > Revision Cloud draws one, a rectangle by
default; with it a rectangle or a polygon is one cloud round (`curves::join`), and Line is one cloud through the points
(`one_line`). `centre_mark` is Revit's Center Mark Visible: a 3 mm cross at each of `Curve::centres`, the Options Bar's Show
Centre for new arcs and circles.

Linework (`linework`, LW) is Revit's: an edge of a model element drawn in a line style in one plan, or hidden there. The
plan's `Graphics::linework` holds each as the element's GUID, the edge's two ends and a style (`None` hidden).
`render::edge_at` finds the edge picked: the plan drawn again around the point without its linework, the nearest
segment of the element's lines within `LINEWORK_REACH`. `render::linework` restyles, after `Look::dress`, the parts of the
element's lines that lie along that edge (collinear within a pixel, over its length), so one face of a wall changes and the
rest of its outline doesn't. Picked again, an edge's entry is replaced; `remove` takes out the one nearest the point (the desktop's
`<By Category>`), not every edge within reach: both faces of a thin wall are. An entry on an edge the element no longer draws (a
wall across a styled face cut it in two) is stale: the drawing still restyles what it overlaps, and where the edge picked has
no entry, `remove` takes the nearest stale entry within `LINEWORK_REACH`, one a call, never one on another edge that is
drawn (`render::edges_at` lists the edges drawn). A view's linework is checked (`Graphics::check`):
at most `MAX_LINEWORK` (2,000) edges, ends within `MAX_COORD`, and a line style the project has. The drawing splits an element's
lines along all its edges in one pass (`render::linework`: the cost is the prims plus the edges, not a pass over every prim for
each edge). A line style a view's linework uses can't be deleted, and an element's entries go with it.

Text notes and text types (`text.rs`). A `Note` is drawn in one view, as a detail line is, and is of a text type
(`Model::text_types`, library entries of `Kind::TextType`, so a file names a note's type by GUID and `rename_entry` follows a
rename): Revit's font, size (its capitals' height on paper, mm), bold, italic, underline, width factor, colour, and a border
`border_offset` mm on paper round its text in pen `border_pen`, from whose side nearer its end a leader starts. A project starts
with 2.5mm, 3.5mm and 5mm in Arial; `text_type` makes, renames and deletes them (none in use). A type's font is a name:
any font the system has, or one we carry (IBM Plex Sans, IBM Plex Mono, BIMCraft Lettering: `font.rs`'s single-stroke
capitals). `text/system.rs` lists the system's fonts once (`system::index`): it looks through the folders Linux, Windows
and macOS keep them in and reads each file only as far as its OpenType table directory and its `name`, `OS/2` and `head`
tables (family, weight, italic, whether it varies its weight), from the public specification, every offset checked; a face
is read whole (`system::load`, kept, up to `MAX_FONT`) only when a note is lettered in it. `text::resolve` turns a type's font
and a character's bold and italic into a `Face`: ours by name; else the family; else its stand-ins (`system::substitutes`:
Arial by Liberation Sans, Arimo, Helvetica, Nimbus Sans, DejaVu Sans…, Times New Roman and Courier New likewise); else Plex
Sans. So Arial, the default, letters on any system, and a file naming a font this one hasn't opens (`TextType::check`
takes any name) and draws with what stands in; the `text_type` command refuses a font the system can't letter, unless the
type has it already. A family's own bold and italic faces are taken where it has them; else a variable face is set to
`wght` 700, any other is made bold by its outline drawn round heavier (our lettering by heavier strokes), and italic by
slanting it. A character a face hasn't is lettered from Plex Sans, then Plex Mono, then the system's wide fonts
(`system::FALLBACKS`: Noto Sans CJK, Microsoft YaHei, PingFang…), else drawn as the lettering's question mark.
`text::shape` reads a glyph's outline with `ttf-parser` once and keeps it. `text_type` without a name lists the fonts
(`text::families`) and what letters each type here (`drawn_with`); Type Properties shows "drawn with" when that differs. A note may override bold, italic and underline on parts of itself: `override_format`
lets its `runs` (character ranges, a line break one character) count, each setting any of the three either way, so a part
stays bold, or not, whatever type the note gets. `modify` with new `text` alone keeps the runs on what didn't change
(`text::remap`: the common start and end keep theirs, what came in takes the format before it). `text::layout` lays a note out
in mm on paper (lines `LINE_PITCH` sizes apart, each character's place and width), which `render::note` draws (each letter a
`Prim::Fill` of rings filled by the non-zero rule, our lettering and Plex Mono's bold as lines, underlines as lines; `Part::Text`)
and which the desktop's editor reads for its caret, its selection and a click's place (`Layout::caret`, `boxes`, `index_at`;
`Drawing::note_frame` turns layout mm into the drawing's pixels and back). The canvas fills a letter, holes and all, from
`text::triangles` (strips between every corner's and crossing's height), kept by its points for the frames that draw it again.
A file of format 7 has notes with a `size`: reading it gives each a type of that size (`Model::typed_notes`), and `Model::check`
refuses a `size` left on one. A note holds up to `MAX_NOTE` (4,000) characters and `MAX_RUNS` (1,000) runs.

Detail groups (`groups.rs`) are Revit's: `create_group` (GP) makes the detail lines, filled regions and text notes given,
all of one view, into a group type (`Model::detail_groups`) holding them as `Member`s about the middle of them, in no
view, and puts one `DetailGroup` element in their place. Members are parts (`HasPart`, `parts_now`): each has its own id
and GUID, and their line styles and region types are library references like an element's (`lib_refs_mut`), so renames
reach them; `Model::check` holds each to what the same element in a view must be (`annotation_ok`). A placed group is an
origin, an angle and `mirrored`; `groups::placed` gives its members as elements of its view (through
`commands::transform`, as Move and Rotate take elements), which `render::detail_group` draws, regions under lines and notes,
all owned by the group, so it picks, moves, rotates and mirrors as one. `ungroup` (UG) puts those elements back in the view;
`group_type` renames a type (its groups follow) or deletes it while nothing is of it. A `RepeatingDetail` places a group
along a line every `spacing` from its start, each turned along it (at most `MAX_REPEATS`), as Revit's Repeating Detail
Component does with a detail component. A view draws every member of every group and repeat in it each time, so
`Model::check` caps what one view places: a group counts its type's members, a repeating detail its members once for each
repeat, and `MAX_PLACED` (10,000) is the most in a view (measured with the probe `probe_the_cost_of_the_most_hatched_regions_a_view_may_place`,
10,000 hatched regions take about 0.1 s to draw at 3 m across and 0.5 s at 30 m across, 2.8 million primitives, on a machine that
is not busy, in a dev build (the core is built at opt-level 2 there) and in release alike, and up to 0.29 s and 1.10 s in the dev
build on a busy one; the 200,000 it once allowed took 6 s and 30 million primitives). A placed member that is a huge circle costs
more (the probe `probe_the_cost_of_placed_members_that_are_huge_circles`: 8,192 points each, about 0.2 ms to draw and 3 to 5 ms to
rasterize, 8 ms on a busy machine), so a view at the cap of them can take a minute or more to render; the cap stays. A member note keeps its own angle under a pure mirror
(it still reads the right way round) and turns with the group's angle. The file holds a group's members as elements, and
`Model::read` leaves out of them what this build doesn't know, as it does for any element
(`/detail_groups/<name>/members/<i>/element/...`). Deleting a line style that a filled region or a group's member is drawn in
is refused with the region's or the group's name. On the desktop, Annotate > Detail has Detail Group, Repeating Detail and
Create Group, and the right-click menu has Create Group and Ungroup.

A project with any of this (line styles and patterns, fill patterns, region types, detail lines, regions, groups, repeating
details, sketches) needs file format 6, and every file is saved as 6 now, with or without any of it: a build that reads up to 5
would fail on one with a serde error, so 6 makes it say that the file is from a newer BIMCraft (even for a plain project). Nothing
in a file of 5 changes, so `Model::migrate` has no step for 5 to 6.

The Options Bar's Offset puts a line, an arc or a polyline beside the points clicked; a rectangle, a polygon or a circle grows
by it as a whole (`grown`, its corners joined), and a spline or an ellipse takes none (`Shape::offsets`). Modify's Offset takes
ellipses and splines too, as a spline through points within `FIT_TOL` of the true offset (`curves::offset_fit`), refused where
it would fold over itself. While a shape is drawn its numbers can be typed (`draw::numbers`, `reading`, `typed`): a rectangle's
width and height (Tab between them, the canvas taking the Tab from egui's focus), a circle's or a polygon's radius, the third
point's radius, angle or reach; anything else typed is a length toward the cursor, as a line's, in sketch mode too. The canvas
labels them as they read (`shape_dimensions`). A spline or a polyline finishes with Enter, a double click or the right-click
menu's Finish Drawing; Fillet Arc previews the arc nearest the cursor and its radius, Offset a dashed ghost of the copy, and
Trim/Extend and Fillet's picks so far show in the selection's colour. Trim/Extend to Corner with two lines or walls selected (or
two sketch lines picked) joins them at once, each keeping the part toward its middle.

`Drawing::detail` is the level the plan or the section is drawn at: coarse leaves out a compound element's layers and an opening's
symbols, medium its patterns (`cut_layer`, a floor's `surface`), fine draws as before. In the 3D view `Look::style` picks how
`Scene::picture` draws: shaded as ever, hidden line as a section's picture is (paper faces, edges), wireframe by `Scene::edges` (where
surfaces meet at an angle or end, found once) drawn through the faces; a pixel within two of a line picks its element.

A view's graphics (`graphics.rs`; `Level`, `Section` and `View3D` each hold `graphics`) hide and override by category, by view
filter (`Model::view_filters`: categories and a schedule filter's rules, `schedule::picks`) and by element: an element's own beats
the filters' (the first applied prevailing), which beat the category's, and hiding by any of them hides. `Look::of` resolves a
view to what it hides and how it paints the rest; the drawing code asks `look.shows(id)` wherever it loops over elements (plans,
sections, `cut_outlines`, `seen_below`, `view3d::Scene::picture`) and `look.dress` recolours the finished `Drawing` by owner and
by `Part` (`Drawing::part` marks what is drawn as projection, cut, lettering or hatching (and the lines between layers), so a
new drawing of what a view cuts sets `Part::Cut`; a hidden door's opening is drawn owned by none, the wall's; the cut walls' union fill has no owner and takes its colour at the draw site, `cut_unions`). A view template
(`Model::view_templates`) controls groups of the views that follow it (categories, filters, a plan's view range through
`Model::view_range_of`, a phase with its phase filter, design options, scale, detail level, visual style), and they can't change those
(`graphics`, `scale`, `detail`, `visual_style` and `modify` refuse). Snaps, Tag All, the desktop's Select All
Instances and the framing of sections and 3D views take what `Look` shows too. A level's ceiling plan is its plan's view;
transparency shows in 3D only. A surface is seen through as much as the greater of its material's `transparency` (`Tri::alpha`, in
plain views too) and the view's override for its element (not in a plain view); both painters take it from `Tri::alpha`.

A 3D view may be cut to a section box (fork issue 1), as Revit's Section Box: `View3D::section_box`, a `SectionBox` of six planes
square to the axes from `min` to `max`, `on` or off (off, it keeps its extents), set by the `section_box` command (which makes the
default 3D view, as `graphics` does, and with no extents fits the box round `Model::model_box`, `SECTION_BOX_ROOM` clear of it), and
checked in `Model::check` (usable numbers, `MIN_SECTION_BOX` between the planes on each axis). `Look::of` carries a view's box while
it's on (`Look::section_box`; `with_section_box` puts another in, for a box being dragged), and `Scene::picture` cuts to it: the view
is square on, so `Boxed` turns a pixel and its depth into a model point with one sum, and `raster` keeps a pixel only where that point
is in the box (a triangle wholly past one plane is passed over first), `dots` a scan's point only where it is, and the wireframe each
edge as far as it runs inside (`within`, Liang and Barsky's clipping). What it cuts away isn't drawn, so it isn't picked; the edge
where a cut face meets paper is drawn as any edge is, and cut faces aren't capped yet. A level whose plane is above or below the box is
left out, and the rest are cut to its sides. A section's picture has its own clip and ignores the box. The card doesn't cut yet:
`Mesh::new` refuses a boxed view, and the processor draws it. Its grips are the core's: `box_grips` (the middle of each face: west,
east, south, north, bottom, top), `box_edges`, `move_box_face` (a face moved along its axis to 10 mm, stopping short of the face across
from it) and `Camera::along` (how far, mm, a drag on the picture moves a point along an axis; none for an axis pointing at the camera).
A plan may follow the box (`section_box` on a `Level` or a `PlanView`, set by `modify`; a dependent view follows its parent's,
`Model::plan_box`; Duplicate View copies it): while the default 3D view's box is on, the plan shows what the box keeps, its range
from the box's bottom up to its top, where it cuts (`Model::shown_range`), and its crop the box's sides, within its own crop if it has
one (`Model::shown_crop`). What a plan draws and snaps to reads those; `plan_range` and `crop_of` stay the plan's own, which commands
(dimensions, joins, Duplicate View) and Properties use. A ceiling plan keeps its own range.
Visibility/Graphics lists each tab's categories alphabetically by their translated display names. Projection and cut pattern
cells edit independent foreground and background overrides, using the project's fill patterns (including imported PAT patterns),
colours and a combined preview. Unchecked layers inherit; `<None>` explicitly hides a layer. OK/Apply submits the draft as one
`graphics` command; Clear Overrides clears only that cell. Annotation categories expose projection patterns for filled regions.
Styles hold `foreground` and `background` as optional `{pattern: name | null, color: [r,g,b]}` layers, alongside the existing
`fill` colour. Element, filter and category precedence resolves each layer separately. Plans, reflected ceiling plans, section
cuts and regions draw the layers with the same bounded pattern renderer as filled region types; the pattern's drafting/model
target sets its scale. Pattern references follow library renames, save as GUIDs, and prevent deletion while used by a view or
template. The 3D renderer continues to use the solid `fill` colour for shading; it does not draw hatch patterns.

A 3D view may be seen in perspective, as through a camera, rather than parallel (axonometric): `View3D::perspective`, a `Perspective`
with the `lens` it is seen through (its focal length, mm on a 35 mm camera, whose 36 mm of film, `FILM`, is the picture's width; 10 to
600 mm, checked in `Model::check`) and `on` or off (off, it keeps its lens), set by the `perspective` command, which makes the default 3D
view as `section_box` does; `View3D::lens` is the lens while it's on. The camera is `view3d::Camera` with `lens` set: the eye stands back from
the target along the view as far as makes `scale` true at the target (`Camera::eye_distance`, the focal length in px over the scale), so
orbit turns it about the target, pan carries both, zoom walks it in (a point on the plane through the target staying under the cursor),
`Camera::sized` keeps the eye where it stood as the picture or the lens changes, and `with_eye_elevation` raises or lowers it looking at the
same target from as far across the plan. `fit_from` fits a perspective with the eye as far back as puts a ball round what is shown inside
the picture's narrower side. `Camera::project` keeps "larger is nearer": in perspective `d² / a - d` for a point `a` mm in front of an eye
`d` from the target, as near as a parallel view's at the target and straight across the picture of a plane (it goes as `1 / a`), so the
rasterizer's barycentric depths stay exact, and `point_at` turns a pixel and its depth back into the model point (textures, the section
box's `Boxed`). What is less than `NEAR` (10 mm) in front of the eye is cut away: a triangle into the one or two its near part makes
(`Projector::in_front`, as weights of its corners, so a curve's lighting carries over), a line to its near part (`Camera::in_front`: edges in
wireframe, levels' marks, the section box's frame); a point behind the eye projects to no number. A face is lit on the side the eye is on
(`Camera::toward`), as the card's `front_facing` is, and how far a line may lie behind a surface and show is a pixel's worth where the
line is (`Camera::slack`). `Camera::along` takes, in perspective, the point on the axis nearest the line from the eye through the pointer,
so a dragged section box face stays under it. `render` of the 3D view goes through its lens, and `eye_elevation` puts the eye that high. A
sheet's 3D viewport stays parallel (its scale is paper's). The card draws perspective too: `View::perspective` gives the shader the
focal length, how far the eye is and a depth of `a + b / ahead`, 1 at `NEAR` and 0 past the far side of the mesh (a reversed depth, as
precise far off as near), with the same `NEAR` cut by the card's clipping; `Mesh::pick` cuts and projects as the processor does.
`picking_the_mesh_in_perspective_finds_what_the_cpu_picture_shows_from_afar_and_from_within` and
`the_card_draws_in_perspective_what_the_processor_does_from_afar_and_from_among_the_walls` compare them, from outside and with the eye
inside the house (`view3d::inside`).

The 3D view can be drawn on the graphics card (File > Settings > Performance; off until asked for: `BimApp::card`, `settings.json`'s
`gpu_3d`). `view3d::gpu::Mesh` is the triangles a view shows as f32 vertices relative to the middle of what is drawn (millimetres far
from the origin would overflow f32), each with its colour, normal, face normal, plane, element and texture; the textures' mip levels as
words (`Skin`; `Mesh::lookups` gives the level a zoom looks up, as `Sampler::level_for` does); and a bounding volume hierarchy built on
the first pick. Surfaces seen through come after the opaque ones (`Mesh::clear_from`), each element's transparency in its `Owner` (an element whose layers differ in transparency has an `Owner` for each).
`Mesh::new` refuses, with a reason, what the card doesn't draw (hidden line, wireframe, more than `MAX_TRIS`), and the processor draws those as before, and sections and sheets always. `Mesh::pick` is `Image::pick` without a picture: the
same projection and barycentric test for triangles (the nearest seen through where it is nearer than the nearest opaque one), and for levels the same `LevelMarks` that `Scene::picture` draws (`level_marks`: one
definition of a level's lines and head). The desktop's `gpu.rs` and `gpu.wgsl` draw it in passes into a texture egui shows like the
processor's picture: the opaque surfaces (lit as `picture` lights them, textured as `Sampler::at` looks up), those seen through (the
nearest in front of the opaque depth, in targets of their own), then an edge wherever the opaque plane changes between a pixel and the one
to its right or below, else the surface tinted halfway where selected, with what is seen through blended over it, and the levels' lines
where nothing opaque nearer hides them. What changes between pictures is the camera and the lines; the owners' tints and the textures'
lookups are written again only when they change. A change to how
the processor shades, draws an edge or marks a level goes in the shader too: `the_card_draws_what_the_processor_does` and the core's
`picking_the_mesh_finds_what_the_cpu_picture_shows` compare them (and, seen through,
`the_card_draws_what_the_view_sees_through_as_the_processor_does` and `picking_through_what_the_view_sees_through_finds_what_the_cpu_picture_shows`), and skip on a machine with no graphics adapter. The card is for Vulkan, Metal and DirectX 12: on OpenGL wgpu panics in the shader compiler for what the shaders read (a depth texture; buffers in a vertex shader), so `Gpu::new` refuses it, and puts whatever else a compiler stops at under `catch_unwind`.
The card is made the first time it is wanted (`Card::ready`: compiling its shaders is not a start's cost), and one that can't be made says why on the Performance page.
A device error is caught
in the card's own error scopes, never left to wgpu's handler (which ends the program), and hands the view to the processor for good.

A level's own plan is the level (its `view_range`, `crop` and `graphics` are the level's); every other plan of it is an `Element::PlanView`
(Revit's Duplicate View: `duplicate_view`, which only plans take). A plan has a key: the level's id, or a plan view's. `Model::plan_level` says
which level a key shows, and everything that draws or measures the model takes that level (`draw_plan`, `plan_shows`, the canvas's tools), while
what is the view's own takes the key: its graphics (`graphics_of`), view range (`view_range_of`, which a template may control), crop (`crop_of`)
and annotation. Annotation says which view it is in: a note or a spot elevation by `view`, a dimension or a tag by an optional `view` beside its
`level` or its host (none: the level's own plan), and `Model::annotation_in` asks whether a plan draws it. A command that takes a `level` takes a
plan view too (`plan_param`): what is built stands on its level, what is dimensioned or tagged is in the view. A dependent view (`parent`) follows
its parent's view range and graphics (`plan_source`; `graphics`, `scale`, `detail` and `visual_style` refuse it, naming the parent), has a crop
of its own and draws its parent's annotation as well as its own. Deleting a view deletes what is annotated in it and its dependents; a level
takes its plan views with it. A project that has none writes the same file as before. A plan's own name, lettered at its bottom left, is in the drawings that make an image (`plan`, `ceiling_plan`: `titled`), not in the window's (`draw`, `draw_ceiling`) or a sheet's (`draw_at`), where the tab and the viewport's title say it. The desktop: View > Duplicate View, DP, and the Project
Browser's right-click, which also has Delete for a plan view; `Sheet::Plan` and `Sheet::Ceiling` hold the key.

Fill patterns, filled region types and filled regions (`fills.rs`) follow line patterns and styles. A `FillPattern` is
families of parallel lines as a PAT file has them (`HatchLine`: an angle, an origin, a spacing across, a shift along, and
dashes, a space negative and a dot 0), drafting (mm on paper, so the same at any scale) or model (mm in the building); no
lines, it fills solid. `fills::strokes` lays a pattern over any outlines with holes (inside an odd number of rings), each line
cut to the view's window (what lies past it is never counted or drawn), and gives up rather than draw past a cap: more than
50,000 strokes, or more work than `fills::MAX_WORK` (a line tried against every edge, a dash stepped over), which bounds a
dense pattern over an outline of thousands of corners and dots a twentieth of a millimetre apart. A pattern that gives up draws
none of its lines and says so in `Drawing::notes` (`the fill pattern of element 100012 has more lines than a view can hold at
this scale`), which the `render` command returns as `notes` for plans and sheets; a front end can show them. `FillPattern::check`
wants a dash (a dot, 0, is one), a space and a pattern at least `MIN_SPACING` long in a family's dashes, so a list of spaces
only draws nothing and never trips a cap. `render::filled_region` draws a region's
masking (paper), its background and its foreground pattern, then its boundary's sketch lines, each in its own line style
(`SketchLine::style`, a library reference as the region's is) else the region's, or not at all (`invisible`, or a region
of invisible lines with none of its own); in sketch mode the Options Bar's Line Style is what the lines drawn take, and the
lines picked take it at once. A plan draws its regions after the model and before the annotation, in a loop of their own (sections
and sheets likewise), so a masking region hides the walls under it but not the note on it. A `FilledRegion` is annotation of
its view (`Model::annotation_plan`, so a duplicated plan has its own) and its sketch is a `sketch::SketchLine` list, solved
when drawn, any number of outlines; Edit Boundary edits it as any other. Patterns and region types are library entries,
renamed with their references (a type follows its patterns, a region its type and line style) and refused deleted while
used. The desktop has Annotate > Region (Filled Region, FR, and Masking Region, sketch mode with the type and the line style
in the Options Bar), Properties (the Type Selector above the instance fields, Line Style with invisible lines), and Manage >
Annotation Settings > Fill Patterns (`fillpatterns.rs`: the patterns as one draft, a simple one edited as an angle, a spacing and
crosshatched or not and its families one by one with Edit Lines, a custom one made of lines redefined from lines, an imported
one kept as it came, and Import and Export of PAT files at the right, Replace imported by same name saying what an import does
with a name the list has; region types are edited from a region with Edit Type, on the same draft).

A custom pattern is made of lines drawn, as pyRevit's Make Pattern makes one (`fills::make`, the `make_fill_pattern` command):
the detail lines picked, all of one view, cut to the rectangle between two corners (the tile), each a family that runs on into the
next tiles' copies of itself. A line repeats only at a slope `(i W, j H)` of whole tiles (`i`, `j` at most `MAX_STEPS`, 48), so one
drawn at another is turned to the simplest such slope within `TURN` (0.25°), else the nearest; its family's dashes are its length
and a space to the repeat `L = |(i W, j H)|`, its lines `W H / L` apart, each shifted along by the lattice's other vector
`(c W, d H)`, `i d - j c = 1` (Euclid's). A line on the tile's edge and its copy on the opposite one are one family; curves go as
their drawn points' chords. A model pattern is as drawn; a drafting one is the drawing on paper at the view's scale, which the
pattern keeps (`FillPattern::drawn`, saved with it): turned from one to the other (the dialog's Type, or `fill_pattern`'s `target`
without `lines`), it is rescaled (`retargeted`), and the dialog offers Redefine for it where a simple one has Edit Lines, which
picks the lines and corners again and keeps its name and type. A pattern holds at most `MAX_FAMILIES`
(256) families. In the desktop, New Pattern > Custom hides the dialog while the lines are picked as anything is (Enter, or the
right-click menu's Finish), then the corners (`Tool::PatternCorner`), and brings it back with the pattern previewed: OK, Pick
Again, Cancel. Esc, another tool or another view brings the dialog back. `pat.rs` reads and writes PAT files (`;%UNITS`,
`;%TYPE`, a negative offset stepping the other way; a pattern that can't be one left out with why) for the dialog and for the
`import_fill_patterns` and `export_fill_patterns` commands. A PAT file is hostile input: `read` stops after `MAX_PATTERNS`
(2,000) patterns and counts the rest, `Read::left_out` names the first `MAX_LEFT_OUT` (100) with `left_out_count` for all, a line
is cut at 22 fields and a bad one is echoed to 40 letters, a family whose dashes are all spaces is left out, and a file over
`MAX_TEXT` (16 MiB) is refused unread. A byte that isn't UTF-8 is read as the Latin-1 letter with its number, which is right
for accents and not for Windows-1252's punctuation at 0x80 to 0x9F. The description after a pattern's name isn't kept. The
`import_fill_patterns` command takes the file's `text` up to that size too (a name is limited to 256 letters, a file isn't) and
replies with the first 100 `left_out` and `left_out_count`.

Edit Type opens a region's Type Properties from either a selected region or an active sketch: select a type, create a
new one, duplicate one with its settings, or rename it. Foreground and background each have their own pattern (including
None) and color; masking controls whether paper hides geometry underneath. The combined preview shows both layers and
a sample line behind them, so masking can be checked before applying. The Patterns tab edits and creates patterns in
the same draft. Apply and OK update definitions and the selected instances in one transaction; a type edit affects every
region of that type, a duplicate is assigned only to the selected instances or the active sketch, and Cancel discards
unapplied work. Duplicate or blank names are refused without replacing another definition. Renames preserve library
identity and update the active sketch's type reference. The palette's Type Selector and Edit Type use the same region
types as the Options Bar.

The instance parameter grid has no second Region Type field: the Type Selector is the type control. Non-masking regions
replace model backgrounds while retaining visible cut outlines, cut foreground patterns (including solid foregrounds),
and projected model lines. `render/regions.rs` finds the drawing's ink once (the model layer's lines and patterns, less what a
later fill covers, so a low wall hidden by a cut wall does not reappear; and a section's or elevation's hidden-line picture as
runs of its pixels, which is all the ink an elevation has) and gives each region the part of it in its area: the outline less
its holes, inside an odd number of rings, less the masking regions drawn before it. A line is kept where it lies in the area or
within half its stroke of it (its pen's width as it will draw, from the owner's object style while the pen is not set yet), so a
region edge sketched on a wall face keeps the whole face; Visibility/Graphics weights wider than the object style's are not
allowed for. Lines are cut by the rings and the masks as they are, never by boolean operations on polygons (which are cubic in
the corners an outline can have): only the model's filled patterns are, by the pieces of the area they reach. The pieces of a
polyline that meet stay one line, so its corners keep their joins, and a dashed line's dashes stay where they were (the dashes of
the cycle a piece begins in are solid lines of their own). A section's ink belongs to the region that keeps it (`Part::Pattern`),
so a sheet's viewport, which keeps what an element owns, has it. A caller that has the section's picture (the desktop's, a sheet's,
`section_png`'s) hands it in (`render::section_over`, `section_at_over`), at the view's size or a multiple of it (a screen with more
pixels to the point), and the ink is the picture's own pixels, as runs: a row of a pixel's height, and a run of up to two pixels
that sits on one of the row above (the edge of a tall wall) joined to it as one vertical line, so 200 walls 20 m tall at 4000 x 2600
px are some hundreds of lines and not a million runs. A picture of more than 250,000 runs (noise, not drawings) keeps the first
250,000 and the drawing's `notes` say the ink of the last rows is under the region. `render::section` draws a picture of its own
when a non-masking region wants one, which costs what the picture does, every call. Masking still hides the model; detail annotation remains above the
region. The same display list feeds the desktop, PNG, SVG and sheet viewports.

Limits, kept as Revit has them or not yet built: a non-masking region with a solid background hides a room's tag text while its outlines
stay (Revit keeps room tags above filled regions: not verified); grids, levels and room fills are drawn before the regions, so a masking
region hides them; there is no Bring to Front or Send to Back, a region is over the model and under the annotation, in the order of its
id; Move and Rotate carry a region's or a floor's model pattern with it (`pattern_origin`), where Revit may leave a model pattern fixed in
the building, and Align moves it; a filled region drawn on a sheet itself is plain annotation over the viewports, so a solid one hides
what lies under it (Revit has no regions on sheets).

Sketch snapping (`snap::in_sketch`) combines the visible model's wall faces and slab edges, detail lines and existing
region boundaries in the active view, and the draft's own lines. It supports endpoints, midpoints (of each segment of a
polyline), intersections (where two walls meet is one too), perpendiculars, nearest points and angle guides, honoring Manage > Snaps
and one-point overrides. An element being edited is omitted as a model snap target; its draft supplies the boundary instead.
Rectangle and arc previews use the same origin points and Shift constraint as a click. Annotation in another independent view,
or hidden by Visibility/Graphics, is excluded. A plan view cuts where its own view range says (`plan_range` of the view, not of its
level); a section or an elevation has no plan to snap to, only its own detail lines and regions. Of what the plan builds, the
nearest `MAX_OUTLINES` elements within reach have their outlines snapped to (one `walls::Index` for them all), and the nearest
`MAX_NEAR` chords are tried against each other. Ceilings snap in a floor plan as well as a ceiling plan, because a snap does not know
which of the two the cursor is in, and the members of a detail group are not snap targets.

The reference workflows are Autodesk's [fill layers](https://help.autodesk.com/cloudhelp/2024/ENU/Revit-GetStarted/files/GUID-8A38ADC8-98A6-4A30-BD21-1886F044F6ED.htm),
[region editing](https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-183D9B1E-6554-4C19-9EA9-B32C8D2A5C9D.htm),
and [sketch snaps](https://help.autodesk.com/cloudhelp/2022/ENU/Revit-Customize/files/GUID-A3947840-1F30-41D7-AF75-B61687952A07.htm).
Boundary weights follow each boundary line's style. See also Autodesk's [region masking behavior](https://www.autodesk.com/support/technical/article/caas/sfdcarticles/sfdcarticles/Filled-Region-with-solid-fill-does-not-cover-all-elements-in-Revit.html).

A material's cut and surface may be one of the project's fill patterns (`Material::cut_pattern`, `surface_pattern`, a
library reference like a region type's), over the patterns drawn in code (`model::Pattern`), which stay for the materials
that name them: `material` takes either name for `cut` and `surface`, and the desktop's material editor lists both. A fill
pattern a material shows can't be deleted, and is renamed with it. `render::fill_pattern` draws a fill pattern for a
region, a cut layer (its courses along the layer) and a floor's or a ceiling's surface. A model pattern is aligned and
rotated as Revit's: a filled region and a floor hold `pattern_origin` and `pattern_angle` (`fills::strokes_placed`), which
Move, Rotate and Mirror carry with the element; Align with a pattern line picked second (`render::pattern_line_at`) moves the
origin so that line lies on the reference, and Properties shows Pattern Angle. `Model::align_pattern(id, reference, line)` is
that arithmetic (the origin moved across by the gap between the two parallel lines, to the micron; the error says when the
element has no model pattern, or the lines aren't parallel or have no length), so every front end gets the same answer and
sets it with `modify`. `pattern_origin` and `pattern_angle` are in the `modify`, `filled_region` and `floor` command docs, and
`elements` shows them for a region and a floor even at their defaults.

## Linked models

A `Link` element (`link.rs`; commands `link` and `reload`) is another project shown in this one as a reference. The file holds its name, its
path, where its origin stands (`at`) and a turn (`angle`); what it read is `Model::loaded`, by path, outside the file (`serde(skip)`), filled by
`link`, `reload` and `open` (`relink`, which doesn't follow the links of a linked file). The views draw `link::seen(model)`: the project with
each loaded link's built elements (`built`: not its views, sheets, annotation, detail lines, filled regions, detail groups, rooms, grids or links) moved and turned by `commands::transformed`
and brought in with `insert::insert_as(.., linked: true)` under ids from `LINKED` up (`ids.rs`), so `link::is_linked(id)` tells them; a level of the
link at the height of one of the project's (1 mm) is that level, another comes in named `link: level`. Everything that takes a `&Model` draws and snaps to
them without knowing: `Look::with` makes a linked id halftone, and picking never answers one (`Drawing::pick_all`, `in_box`, `Image::pick`,
`in_box`, `shows` and `Mesh::pick` leave them out). The commands that make an image (`render`, `save` to svg, png or pdf) draw `seen`; the desktop's
`view_model` does, cached by the project's `Arc`. The schedules, `Model::check`, IFC and every command that edits work on the project itself.
To do: shared coordinates, a link's graphics overrides and visibility per view, hosting on a linked wall, unloading.

## Dimensions

A dimension (`Dimension`, the `dimension` command) measures the model as it is, so it follows what it measures. Its `refs` are lines
(`Reference {id, face}`: a wall's centre line, faces and ends, a door's or window's centre and jambs, a grid, a straight detail line) or, with
`point: start|end`, the point at that end of one (`Model::reference_point`: a wall's centre line ends where its location line does, a face where
the outline turns off it, so at the mitred corner; a detail line of any shape at its ends; `reference_points` lists them, `endpoint_at` finds the
nearest, `{endpoint: [x, y]}` in a command). `reference_geometry` gives either as a line, a point as a line of no length, and `crossings` lays
them along the way the dimension measures (`measured_along`): a linear one's `orient` (horizontal or vertical), else square to its first line,
else from its first point to the next elsewhere. `Model::check` refuses a line not parallel to the first, or not square to a linear one's line,
and points all in one place. Mirror keeps the end of a line along the element and flips the one across it (`Reference::mirrored`); Flip swaps
both. A linear one given no `orient` runs vertical where `at` is east or west of everything it measures, else horizontal; the desktop's Auto
(`dims::Placing::kind`) also lays an aligned one where the cursor is among the points (`Orient::follow`), as Archicad does. Radial and diameter
dimensions measure round columns, curved walls (centre line or a face) and circles and arcs (`reference_arc`); arc length, a curved wall's or an
arc's length along it (`reference_sweep`), all of it or, with two point references after the arc, between them (`Model::arc_span`), its line an arc through `at` round the same centre. `dims::round_at` picks the face nearest a click.

Each dimension has a dimension type (`dims.rs`, `Model::dimension_types`, a library entry `Kind::DimensionType`, the `dimension_type` command),
Revit's type parameters with lengths in mm on paper: Dimension String Type (continuous, baseline, ordinate), tick mark (diagonal, arrow, open
arrow, dot, none) and its size, line and tick pens, colour, dimension line extension, Witness Line Control (gap to element, or fixed to the
dimension line with its length), witness line gap and extension, baseline spacing, centre marks, and its text: font (as text types take one,
`DimensionType::lettering`), size, bold, italic, underline, width factor, offset, opaque or transparent background, Read Convention, units and
rounding (`DimensionType::length`), and the radius and diameter symbols and the equality text. A project starts with `Diagonal - 2.5mm Arial`,
which aligned, linear, angular and arc length dimensions start in, and `Arrow - 2.5mm Arial`, which radial and diameter ones do
(`dims::default_type`); a file of format 9 gives its dimensions their kind's (`Model::typed_dimensions`). A dimension's own text, Revit's
instance values, is a prefix, a suffix, text above and below, Replace With Text and Equality Display, and a segment may have its own over it
(`texts`, `SegmentText`, the `dimension_text` command; `Dimension::text` reads one through the other, `dimension_segment_at` finds a segment by a
point); `Model::dimension_labels` is what its
numbers read (and `elements` shows as `labels`): a baseline string's each from its first reference, an ordinate's each witness line's, the first 0.
An aligned or linear dimension may be kept equal (`equal`, Revit's EQ, which also turns its Equality Display on) and have segments
locked at a length (`locked`, `Lock`; the `dimension_constraint` command). `constraints::keep` runs in `Session::execute` after every undoable
command, before the check: each constrained dimension is solved on its own line (its `stations`, each with its reference), a locked segment tying
two stations rigidly and equality tying all its segments to one length (a locked one's, else its ends', which stay where they are, so an inner
reference dragged goes back); a run of tied stations stays where what the command moved puts it, else where its first is. What has to move is
moved by `commands::move_by`, as `move` moves it, and the dimensions are solved again (`PASSES`) until all hold; where they can't (locked
lengths that differ made equal, one element pulled two ways), the command is refused, saying which dimension, and nothing changes.
`render/dimensions.rs` draws them all as the type says (`Ink`: its lines, ticks and witness lines, and its numbers laid out by `crate::text`
over the paper where the background is opaque).
