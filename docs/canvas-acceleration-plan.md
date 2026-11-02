# Accelerating the 2D canvas

A plan in pieces, as [the spatial core's](spatial-core-plan.md) and [the sections'](section-views-plan.md), for plans, section
overlays and sheets on the screen.

**The goal: the same functionality and the same look as now, truly accelerated.** Identical is a property of the design, not a
tolerance in a test: the triangles on the card are the ones egui's own tessellator makes from the shapes `canvas::paint` makes, drawn
by the same shader sums and blending. No line shader that resembles them. Export (SVG, PNG, PDF) stays on `Drawing`, untouched.

## Measured (2026-11-02)

The annotated tower, a storey's plan, 1600 × 1000, release, one thread of the interface. Storeys barely matter (10 storeys within
10% of 100), so the lookup does its job and the cost is the plan's own 88,000 primitives:

| Zoom from fitted | Make the drawing | Make shapes | Tessellate | Primitives (off screen) | Vertices |
|---|---:|---:|---:|---:|---:|
| 1× | 163 ms | 45 ms | 133 ms | 88,747 (0) | 5.4 M |
| 4× | 150 ms | 42 ms | 79 ms | 59,792 (38,835) | 2.5 M |
| 16× | 153 ms | 53 ms | 110 ms | 46,578 (44,879) | 2.1 M |
| 64× | 153 ms | 112 ms | 52 ms | 45,632 (45,456) | 0.3 M |

Shapes and tessellation are paid **every frame**, whatever changed: about 180 ms, and egui's upload of every vertex again is another 55,
so about 4 frames a second for moving the pointer. A pan or zoom adds the drawing on the interface's thread. Selecting adds only a few
milliseconds, so what costs is not knowing that nothing changed. (The 2 ms idle frame in [performance](performance.md) is of a small
project.)

## Pieces

Each keeps the workspace green. Each is held to today's path: the same pixels, compared on a real or software card.

1. **Retained triangles.** The drawing's shapes tessellated once by egui's `Tessellator`, kept in buffers on the card and drawn by a
   paint callback in their place between the grid and the tools' overlays. Rebuilt when the drawing, selection, hover, theme,
   pixel density or Thin Lines changes. What holds a picture, and a window with no card (the tests' harness), stays on today's path.
   Off-screen primitives are left out before shapes are made, which leaves the triangles as they were.
2. **Hover and selection in place.** Each element's range of vertices kept; a change of tint tessellates that element again and writes
   its range. Crossing an element costs its own triangles, not the plan's.
3. **Pan as a transform.** The buffers drawn moved until the new drawing comes. A whole-pixel move is what egui draws there but for
   rounding, in a few edge pixels, because a vertex's position is summed otherwise in floats: one level, three at the dark end of an
   sRGB framebuffer. The final drawing replaces
   it, exact. A zoom shows the last triangles scaled until then, as sections do with their picture.
4. **Plans on the view workers.** `render::draw` and tessellation off the interface's thread, on `view_worker.rs`, one job at a time, each
   for where the view is when it starts, the drawing and its picking published together. Moving never waits for a plan. What the view
   didn't move over (a plan opened, an edit, and a window with no card) is still made in its frame, so it shows at once.
5. **Geometry apart from the view.** `Drawing` holds pixels baked for one view. Model-space geometry kept separately from what depends
   on scale (lettering, symbols, line weights, dashes), so a pan needs no new drawing at all, with a spatial index for picking and
   snapping.
6. **Fragments by element.** An edit makes again only what it changed. Needs the neighbour and empty-query dependencies of
   [the review's priority 2](spatial-core-review.md): a wall's drawing depends on the walls it joins.
7. **Section overlays and sheets.** They share `canvas::paint`, so they take pieces 1 and 2 with little more.

## What must stay the same

- **Draw order.** A tinted element stays in its place in the order: an overlay on top would change what covers what.
- **Region compositing** (`render/regions.rs`) happens in the core before the shapes, so pieces 1 to 4 don't touch it. Piece 6 must
  rebuild everything a changed masking region overlaps, at its old and new place.
- **Colour.** Sums copied from egui's shader, for both kinds of framebuffer, with its dithering when the window's renderer has it on.
- **Fallback.** No card, a card that fails or a drawing with a picture: today's path, always there, which the tests also hold it to.

## Pieces 1 and 2 measured (2026-11-02)

`retained.rs`. The same plan at 1600 × 1000, release, an NVIDIA RTX 2060, what the interface's thread spends:

| | Before, every frame | A frame that changes nothing | Crossing an element | A new drawing's triangles |
|---|---:|---:|---:|---:|
| Fitted | about 250 ms | 0.2 ms | 3.5 ms, then 0.5 ms (0.12 ms median over 200 elements) | 178 ms |
| 4× | about 120 ms | 0.1 ms | 1.3 ms, then 0.2 ms | 22 ms |
| 64× | about 164 ms | 0.03 ms | 0.15 ms | 2 ms |

Selecting a tenth of the plan's 7,108 elements takes 10 ms. A rebuild tessellates on all threads, in order, and writes each part straight
to the card. It is still paid on every pan and zoom step, with the 150 ms of the drawing, which pieces 3 and 4 take off this thread.

**The look.** `kept_triangles_draw_the_pixels_egui_draws` compares bytes against `canvas::paint` through egui's own renderer: both
framebuffer kinds, light and dark, Thin Lines, selected and hovered, retinted from another tint, zoomed, clipped, at 1× and 1.5× density.
`the_whole_window_is_the_same_on_kept_triangles_in_each_look` drives the whole app in the three looks. Without dithering, 6,245 pixels
differ, so they can tell. The dense tower plan had no pixel different at any zoom, by a one-off run. They need a card, and pass without
one (as `gpu::headless`'s do), so a runner with no adapter proves nothing here: run them on a machine with one.

## Pieces 3 and 4 measured (2026-11-02)

The same plan, 1600 × 1000, release, the RTX 2060. Triangles are now made half a canvas past each edge, so a pan has something to show:
the scissor cuts them, so the pixels on the canvas don't change.

| | A frame moving | The worker: drawing, then triangles | Worst wait of the card's queue meanwhile |
|---|---:|---:|---:|
| Fitted | 0.08 ms | 168 + 207 ms | 3.2 ms |
| 4× | 0.05 ms | 159 + 58 ms | 0.3 ms |
| 64× | 0.04 ms | 164 + 2 ms | 0.1 ms |

Today's cost of the wider reach is on what is still made in its frame: an edit at 4× pays 58 ms for its triangles, not 22.

`a_view_moved_shows_what_is_kept_until_a_worker_replaces_it_with_the_exact_pixels` holds the window, once settled, to no pixel different
from a window with no card. What's under the cursor while moving is what the shown drawing shows there (`canvas::Shown`). The first frame
of a plan and an edit are as slow as they were: about 330 ms at the fit zoom. Piece 6 is what takes the edit's cost away. Plans with a
picture go through a worker too, drawn moved by `canvas::paint`, which no test yet drives with a card.

## Status

- [x] Measure
- [x] 1. Retained triangles
- [x] 2. Hover and selection in place
- [x] 3. Pan as a transform
- [x] 4. Plans on the view workers (for moving; opening a plan and edits stay in the frame)
- [ ] 5. Geometry apart from the view
- [ ] 6. Fragments by element
- [ ] 7. Section overlays and sheets
