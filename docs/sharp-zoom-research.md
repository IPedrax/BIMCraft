# Sharp lines at every zoom: how fast viewers do it, and what BIMCraft should do

Asked 2026-11-14: "how do those superfast PDF readers draw sharp lines all the time when zooming in and out, without you
seeing the blocks redraw?" What we found, what BIMCraft does today, and the plan.

## What BIMCraft does today, measured in the window

`bimcraft --bench` on the annotated 100-storey tower (`docs/areas/desktop.md`, *Benchmarking the window*), 1920 × 1080, RTX 2060:

- A plan's drawing (`render::draw`) is made **in screen pixels for one view**: one zoom, one pan. egui tessellates it into
  triangles with a 1-pixel anti-aliasing feather baked into each edge, and `retained.rs` keeps them on the card.
- While the view moves, the frame shows **the last triangles scaled** (`canvas::Shown`) and a worker makes the exact drawing
  for where the view is now: 150 to 400 ms on the tower, more on a bigger plan.
- Scaling triangles scales their feathers and their widths: at 4× a hairline is 4 px wide with a 4 px blur on each side.
  Until the worker's drawing comes, **that is the blur the user sees**. A zoom with the wheel asks for one drawing after
  another, so on a big plan the blur lasts as long as the wheel turns.

## How the fast viewers do it

There are two families, and the ones that never blur are the second.

### 1. Raster tiles, redrawn fast enough not to be seen (Acrobat, Foxit, SumatraPDF/MuPDF, PDF.js, Preview)

The page is rasterised on the CPU into tiles at the current zoom. While the zoom changes, the tiles of the old zoom are shown
scaled (blurry), and new tiles replace them as they are made. What hides the blur:

- **Speed.** A page rasterises in a few milliseconds, the visible tiles at once on all cores; stale renders are cancelled as
  soon as the zoom moves on, so the newest zoom is always the one being rendered
  ([PDFium tiling and caches](https://blog.loslab.com/en-us/pdfium-component/pdfium-component-render-cache-zoom-performance.html),
  [Syncfusion's PDFium viewer](https://www.syncfusion.com/blogs/post/smooth-scrolling-performance-image-heavy-pdfs)).
- **Levels of detail kept ahead.** Apple's `CATiledLayer` keeps several resolutions of each tile (`levelsOfDetail`,
  `levelsOfDetailBias`): zooming in shows a level already rendered sharper than needed
  ([Apple's ZoomingPDFViewer sample](https://developer.apple.com/library/archive/samplecode/ZoomingPDFViewer/Listings/ZoomingPDFViewerCocoa_ZoomingPDFViewer_TiledPDFView_m.html),
  [cocoa-dev on the bias](https://lists.apple.com/archives/cocoa-dev/2013/Dec/msg00198.html)).
- **Swap whole.** The viewport is replaced as one picture, so tiles never visibly pop in one by one.

This works for a PDF page because a page is small, at most tens of thousands of paths. A BIM plan of a tower storey is
88,000 primitives and a drawing of it takes 150 ms, so this family can't hide the blur here without a much faster drawing.

### 2. Vector geometry on the GPU, only a transform changing (Figma, Mapbox GL / MapLibre, Google and Apple Maps, CAD viewers)

The geometry is put on the card once in **world coordinates**. A zoom or pan changes only a matrix in a uniform: nothing is
rasterised or tessellated again, so every frame is sharp, at the frame rate. What makes the lines sharp:

- **Each segment is a quad expanded in the vertex shader**, its width worked out in screen pixels from the zoom, plus a
  feather of about one pixel; the fragment shader fades the coverage by the distance to the segment's centre line. Mapbox's
  line shader sets the feather to `1 / devicePixelRatio / 2` and extrudes by half the width plus that
  ([MapLibre's line shader](https://cdn.jsdelivr.net/npm/maplibre-gl@5.24.0/src/shaders/glsl/line_sdf.vertex.glsl)); the same is done by
  instanced-segment renderers ([Rye Terrell's instanced lines, via regl-gpu-lines](https://github.com/rreusser/regl-gpu-lines/),
  [WebGPU instanced lines](https://rreusser.github.io/notebooks/webgpu-lines)) and in CAD (SolveSpace's
  [edge shader](https://gitea.suzanne.soy/suzanne.soy/solvespace/src/branch/master/res/shaders/edge.vert)).
- **A width can be in world units or in pixels.** Map labels and hairlines keep their pixel width; a wall's cut line, which
  is a width on paper at the view's scale, grows with the zoom, as in Revit. Either way it is one multiply in the shader, and
  the feather stays one pixel.
- **Dashes in the shader**, from the distance along the line (Mapbox's `line_sdf`), so a dash pattern needs no new geometry.
- **Text as vector glyphs or signed-distance glyphs**, sharp at any size.
- **Fills** are triangles in world space; their edges are drawn as hairlines, or anti-aliased by the screen-space derivative
  of an edge distance.
- Figma's renderer is "a highly-optimized tile-based engine … all rendering is done on the GPU and is fully anti-aliased"
  ([Figma's engineering blog](https://www.figma.com/blog/building-a-professional-design-tool-on-the-web/)), now on WebGPU with
  compute shaders ([Figma rendering, powered by WebGPU](https://www.figma.com/blog/figma-rendering-powered-by-webgpu/)); Vello
  takes the same idea furthest, the whole 2D pipeline in compute shaders on wgpu, the transform applied early on the card so
  "the vector paths will be re-rendered at full resolution with vector crispness"
  ([Vello's vision](https://skia.googlesource.com/external/github.com/linebender/vello/+/231a434e70025f4020383ca86bcd2db61eca57dd/doc/vision.md)).

## What BIMCraft should do

The second family. It is [the canvas plan's](canvas-acceleration-plan.md) piece 5 ("geometry apart from the view"), and the
answer to "sharp while zooming" is the same piece:

1. **A plan's geometry in model space.** `render::draw` already works in millimetres before it maps to pixels; keep the
   primitives in millimetres (segments with their width on paper and their dash pattern, fill triangles, glyph outlines),
   made once per level and edit, by the workers, on every core.
2. **A segment pipeline on the card** (wgpu, beside `retained::Pipeline`): one instance per segment with its two ends, its
   width (paper mm × scale, or a pixel width for Thin Lines and hairlines), colour and dash; the vertex shader expands it in
   screen pixels with a one-pixel feather, the fragment shader fades by distance and cuts dashes. Fills and glyphs as
   model-space triangles in the same pass, in draw order.
3. **Zoom and pan are a uniform.** No worker, no tessellation, no scaled picture: every frame of a wheel zoom is sharp, at
   60 Hz on a tower storey (88,000 primitives is a few hundred thousand instances, well inside one draw call).
4. **What depends on the zoom is little, and late.** Annotation in BIMCraft is sized on paper by the view's scale, not by the
   zoom, so it is model-space geometry too. Only what is laid out in screen pixels (the tools' overlays, grips) is made per
   frame, as it is now.
5. **The exact drawing stays the oracle.** The rule of the canvas plan is that the window shows the pixels egui's tessellator
   makes. Two ways to keep that, for the user to choose:
   - *Sharp while moving, exact once still*: the segment pipeline draws while the view moves, and the worker's egui
     triangles replace it once it stops. Never blurry; a settled view is byte-identical to today's. A pixel or two of
     anti-aliasing may change as the exact drawing lands.
   - *The segment pipeline always*: simpler, faster, one look, but no longer egui's pixels byte for byte; the tests would
     compare against `render::png` within a tolerance instead.

   Our recommendation is the first. It keeps every existing guarantee and removes the blur.

Export (SVG, PNG, PDF) stays on `Drawing`, untouched, as the canvas plan says.

## What was done first (2026-11-14)

The first option, without waiting for model-space primitives: the kept drawing's own shapes, the ones egui tessellates, are also
kept as segments for the motion shader (`ui-egui/src/motion.rs`, `fs_motion_*` in `retained.wgsl`). A drawing's pixel
coordinates are an affine map from its model's, so scaling its centre lines is exact; only the widths and the fade were wrong,
and the shader works those out on screen. A frame that zooms draws the segments; a pan and a still view draw egui's triangles.

## Status

- [x] Research (this note)
- [x] Measured in the window (`bimcraft --bench`)
- [x] The segment pipeline (`retained.rs`, `motion.rs`) for frames that zoom
- [ ] Its pixels compared with egui's on a real card, at zoom 1 (where it should all but match)
- [ ] Model-space primitives from `render::draw`, so a zoom needs no new drawing at all (canvas plan, piece 5)
