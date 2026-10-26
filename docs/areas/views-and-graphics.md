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

Detail lines and their line styles (`lines.rs`): a `LinePattern` is a dash or a dot then a space, again and again, each length mm
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
draft, applied as one undo step). Next: patterns made from lines drawn in a view.

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
where it's cut. Commands: `split` (detail lines as walls), `join_polyline` (PJ), `explode` (PX), `fillet` (FL), `corner`
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
whichever way round it was drawn, to the left of an open line). Annotate > Detail > Revision Cloud draws one, a rectangle by
default; with it a rectangle or a polygon is one cloud round (`curves::join`), and Line is one cloud through the points
(`one_line`). `centre_mark` is Revit's Center Mark Visible: a 3 mm cross at each of `Curve::centres`, the Options Bar's Show
Centre for new arcs and circles.

Linework (`linework`, LW) is Revit's: an edge of a model element drawn in a line style in one plan, or hidden there. The
plan's `Graphics::linework` holds each as the element's GUID, the edge's two ends and a style (`None` hidden).
`render::edge_at` finds the edge picked: the plan drawn again around the point without its linework, the nearest
segment of the element's lines within `LINEWORK_REACH`. `render::linework` restyles, after `Look::dress`, the parts of the
element's lines that lie along that edge (collinear within a pixel, over its length), so one face of a wall changes and the
rest of its outline doesn't. Picked again, an edge's entry is replaced; `remove` takes it out (the desktop's `<By Category>`).
A line style a view's linework uses can't be deleted, and an element's entries go with it.

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
Component does with a detail component. On the desktop, Annotate > Detail has Detail Group, Repeating Detail and Create
Group, and the right-click menu has Create Group and Ungroup.

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
transparency shows in 3D only.

The 3D view can be drawn on the graphics card (File > Settings > Performance; off until asked for: `BimApp::card`, `settings.json`'s
`gpu_3d`). `view3d::gpu::Mesh` is the triangles a view shows as f32 vertices relative to the middle of what is drawn (millimetres far
from the origin would overflow f32), each with its colour, normal, face normal, plane, element and texture; the textures' mip levels as
words (`Skin`; `Mesh::lookups` gives the level a zoom looks up, as `Sampler::level_for` does); and a bounding volume hierarchy built on
the first pick. Surfaces seen through come after the opaque ones (`Mesh::clear_from`), each element's transparency in its `Owner`.
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
lines, it fills solid. `fills::strokes` lays a pattern over any outlines with holes (inside an odd number of rings), within
the view's window, and gives up rather than draw past a cap, too dense to read; `render::filled_region` draws a region's
masking (paper), its background and its foreground pattern, then its boundary's sketch lines, each in its own line style
(`SketchLine::style`, a library reference as the region's is) else the region's, or not at all (`invisible`, or a region
of invisible lines with none of its own); in sketch mode the Options Bar's Line Style is what the lines drawn take, and the
lines picked take it at once. A plan draws its regions after the model and before the annotation, in a loop of their own (sections
and sheets likewise), so a masking region hides the walls under it but not the note on it. A `FilledRegion` is annotation of
its view (`Model::annotation_plan`, so a duplicated plan has its own) and its sketch is a `sketch::SketchLine` list, solved
when drawn, any number of outlines; Edit Boundary edits it as any other. Patterns and region types are library entries,
renamed with their references (a type follows its patterns, a region its type and line style) and refused deleted while
used. The desktop has Annotate > Region (Filled Region, FR, and Masking Region, sketch mode with the type and the line style
in the Options Bar), Properties (the Type Selector above the instance fields, Line Style with invisible lines), and Manage > Fill Patterns (`fillpatterns.rs`:
region types and patterns as one draft, a simple pattern edited as an angle, a spacing and crosshatched or not, any pattern's
families one by one with Edit Lines, and Import and Export of PAT files). `pat.rs` reads and writes PAT files (`;%UNITS`,
`;%TYPE`, a negative offset stepping the other way; a pattern that can't be one left out with why) for the dialog and for the
`import_fill_patterns` and `export_fill_patterns` commands.

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
and projected model lines. `render/regions.rs` clips restored model ink to the region's filled area, honors holes and
masking regions, and preserves model occlusion so a low wall hidden by a cut wall does not reappear. Sections retain the
visible hidden-line projection as well as vector cut graphics. Masking still hides the model; detail annotation remains
above the region. The same display list feeds the desktop, PNG, SVG and sheet viewports.

Sketch snapping (`snap::in_sketch`) combines the visible model's wall faces and slab edges, detail lines and existing
region boundaries in the active view, and the draft's own lines. It supports endpoints, midpoints, intersections,
perpendiculars, nearest points and angle guides, honoring Manage > Snaps and one-point overrides. An element being edited
is omitted as a model snap target; its draft supplies the boundary instead. Rectangle and arc previews use the same
origin points and Shift constraint as a click. Annotation in another independent view, or hidden by Visibility/Graphics,
is excluded.

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
origin so that line lies on the reference, and Properties shows Pattern Angle.

## Linked models

A `Link` element (`link.rs`; commands `link` and `reload`) is another project shown in this one as a reference. The file holds its name, its
path, where its origin stands (`at`) and a turn (`angle`); what it read is `Model::loaded`, by path, outside the file (`serde(skip)`), filled by
`link`, `reload` and `open` (`relink`, which doesn't follow the links of a linked file). The views draw `link::seen(model)`: the project with
each loaded link's built elements (`built`: not its views, sheets, annotation, rooms, grids or links) moved and turned by `commands::transformed`
and brought in with `insert::insert_as(.., linked: true)` under ids from `LINKED` up (`ids.rs`), so `link::is_linked(id)` tells them; a level of the
link at the height of one of the project's (1 mm) is that level, another comes in named `link: level`. Everything that takes a `&Model` draws and snaps to
them without knowing: `Look::with` makes a linked id halftone, and picking never answers one (`Drawing::pick_all`, `in_box`, `Image::pick`,
`in_box`, `shows` and `Mesh::pick` leave them out). The commands that make an image (`render`, `save` to svg, png or pdf) draw `seen`; the desktop's
`view_model` does, cached by the project's `Arc`. The schedules, `Model::check`, IFC and every command that edits work on the project itself.
To do: shared coordinates, a link's graphics overrides and visibility per view, hosting on a linked wall, unloading.
