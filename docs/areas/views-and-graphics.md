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
paper from 0.10 to 4 mm (our own steps, not Revit's), and 0, a hairline, which every output draws as thin as it can whatever the scale
and the zoom (a width of 0 in the display list: tiny-skia's hairline, PDF's `0 w`, an SVG pixel with `vector-effect="non-scaling-stroke"`,
a pixel on screen). What a pen draws is the project's line weights (`pen::LineWeights`, `Model::line_weights`, the `line_weights`
command, Manage > Line Weights in the desktop, `lineweights.rs`): the model's pens 1 to 16 at each of a list of scales, a view taking
the column of the scale nearest its own (`LineWeights::at`), and the annotation's at any scale. A project that hasn't set them has
our own start for its units (`default_for`: metric or imperial columns, the model's pens wider at finer scales, by the fourth root,
and at 1 : 100 the pens' own widths, `Pen::mm`). A drawing keeps each line's pen (`Drawing::pens`, `stroke`), an override sets the pen
(`Look::dress`), and the last step of drawing a plan, a section, a sketch or a sheet's own lines sizes each line (`Drawing::weigh`),
as annotation where its owner is a dimension, tag, text, spot elevation, grid, level, room or mark (`annotates`), else as the model;
a view on a sheet takes the line weights at its viewport's scale. Cut outlines are inked over the poché: `cut_edges` keeps of each
outline what isn't against another's on its outer side, so joins stay clean. The desktop draws a line its pixels at the zoom, at least one (`canvas::on_screen`); Thin Lines draws every line one. A
file from before pens (format 3) held weights in pixels of a sheet's drawing, and opens with each the nearest pen (`pens_from_pixels`).
Which pen each kind of line draws with is named once, in `pen.rs` (`Pen::CUT`, `Pen::PROJECTION`, `Pen::HEAVY` and the rest), never a number at the draw site: a cut is `Pen::CUT` in plan and in section alike. What's next on this foundation: object styles (each category's pens for cut and projection), viewports as windows onto their views,
and printing. `Drawing::detail` is the level the plan or the section is drawn at: coarse leaves out a compound element's layers and an opening's
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
the first pick. `Mesh::new` refuses, with a reason, what the card doesn't draw (hidden line, wireframe, anything seen through, more than
`MAX_TRIS`), and the processor draws those as before, and sections and sheets always. `Mesh::pick` is `Image::pick` without a picture: the
same projection and barycentric test for triangles, and for levels the same `LevelMarks` that `Scene::picture` draws (`level_marks`: one
definition of a level's lines and head). The desktop's `gpu.rs` and `gpu.wgsl` draw it in three passes into a texture egui shows like the
processor's picture: the surfaces (lit as `picture` lights them, textured as `Sampler::at` looks up, tinted halfway), an edge wherever the
plane changes between a pixel and the one to its right or below, and the levels' lines where nothing nearer hides them. A change to how
the processor shades, draws an edge or marks a level goes in the shader too: `the_card_draws_what_the_processor_does` and the core's
`picking_the_mesh_finds_what_the_cpu_picture_shows` compare them, and skip on a machine with no graphics adapter. The card is for Vulkan, Metal and DirectX 12: on OpenGL wgpu panics in the shader compiler for what the shaders read (a depth texture, so depth is kept in the id target; buffers in a vertex shader), so `Gpu::new` refuses it, and puts whatever else a compiler stops at under `catch_unwind`.
The card is made the first time it is wanted (`Card::ready`: compiling its shaders is not a start's cost), and one that can't be made says why on the Performance page.
A device error is caught
in the card's own error scopes, never left to wgpu's handler (which ends the program), and hands the view to the processor for good.
