# Sections and elevations that scale

A plan, in pieces like [the spatial core's](spatial-core-plan.md) (it is the part of its piece 7 that concerns sections,
elevations and the 3D scene they are drawn from), to make drawing a section or an elevation cost what it shows, not the
size of the project. Every piece keeps the result: the same pixels in the picture, the same primitives over it, the same
sheets and PDF. The old computation stays as the twin tests hold the new one to.

## What it costs now (2026-11-01)

The annotated tower with its drawing set (`tower {"storeys": 10, "annotation": true, "sheets": true}`, 120,513
elements), release build, a 1600 × 1000 px view:

| Step | Time | What it does |
|---|---:|---|
| Frame the view (`section_fit`, `sheet::size`) | 0.85–1.0 s | `span`: every solid of the model, and `model_extent` (every solid again) for the levels' ends |
| Build the 3D scene (`Scene::new`) | **7.0 s** | 186,310 solids (135 ms) as **9.3 million triangles** (1.4 s, one thread), faces and textures (0.9 s), coplanar planes and smooth normals (4.1 s, one thread) |
| The hidden-line picture (`section_image`) | 1.3 s | every one of the 9.3 million triangles projected and binned |
| The drawing over it (`section_over`) | 1.5–1.8 s | `span` again (0.7 s), every solid cut by the line, every wall's layer lines, `heights` (every solid twice more) for the levels |
| An elevations' sheet (two elevations) | 14.8 s | all of the above, once a view |

At 100 storeys (1.2 million elements) the scene would be about 93 million triangles; a run passed 32 GB and was stopped.

What the picture shows: **the north elevation sees 4,884 of the 9.3 million triangles (0.05%), from 1,034 of 92,440
elements**; the section through the middle, 3,289 elements. Nearly all the work is on what is hidden behind the facade
or far from the line.

In the desktop (`crates/ui-egui/src/section.rs`) the picture is kept until the view, its size or the model changes, but
the drawing over it (`render::section_over`) is made again **every frame**, 1.5 s each at 10 storeys; and any edit builds
the whole scene again (`orbit::scene_of`) before the section can be redrawn.

## Why

1. **The same whole-model work, again and again.** One elevation builds every solid of the model about seven times
   (`span`, `model_extent` twice through `level_spans`, `heights` twice, `section_in`), each with a new `walls::Index`.
2. **A 3D scene for a 2D drawing.** The picture is rasterized from the scene the 3D view uses: every element
   triangulated, with textures, smooth normals and coplanar plane ids for all 9.3 million triangles, though the picture
   needs only the few thousand in sight, in hidden line.
3. **No culling.** Nothing asks what is in the view's volume (its line, depth and crop) or what is hidden behind the
   facade before triangulating and rasterizing it.
4. **Nothing kept between frames or edits.** The drawing over the picture each frame; the scene after each edit.

## Pieces

Each leaves the workspace green, and is measured with the probe at 1, 10 and 100 storeys.

0. **Measure.** The probe (`examples/performance.rs`) times a section and an elevation step by step (frame, scene,
   picture, drawing over it, a sheet of them) and counts the triangles and elements seen. Numbers in
   [performance.md](performance.md).

1. **Each thing once a drawing.** A `SectionWork` made once for a view and a model: the solids the view's look shows,
   the model's extent, its heights, the levels' spans. `span`, `heights`, `level_spans`, `section_bare`, `section_in`
   and `sheet::framing` take it instead of making their own; `model_extent` and `heights` find what they need from it.
   Twin: the functions as they are, asserted equal in tests. Expected at 10 storeys: the drawing over the picture 1.5 s
   to about 0.2 s, framing 0.9 s to about 0.1 s.

2. **The desktop keeps the drawing.** The drawing over the picture is kept with the picture, under the same key (the
   model, the view, its pixels, dark or light) and made again only when that changes; tints change the paint, not the
   drawing. A UI test counts drawings made over frames that change nothing (as `Cut::pictures` counts pictures).
   Expected: an idle or hovered section frame from 1.5 s to the paint alone.

3. **Only what the view's volume holds.** Each element's box, found with its solids and kept in the `SectionWork`; the
   cut asks only the solids whose boxes cross the line, the picture only those in the volume (in front of the line,
   within the depth, within the crop's heights and ends). The layer lines of the walls near the line only, from the
   lookup's tiles. A large gain for sections with a far clip and for the cut; little for an elevation, whose volume is
   the building. Twin: every solid, as now.

4. **Only what can be seen: occlusion culling.** The heart of it. Before anything is triangulated:
   - the boxes of the elements in the volume, projected, nearest first;
   - a depth pre-pass of the opaque elements that hide the most (walls, floors, roofs, facade panels: prisms and slabs,
     cheap to triangulate), into a coarse depth pyramid (each tile of 8 × 8 px keeping its farthest depth);
   - each other element whose box lies **strictly behind** every tile it covers is left out, never triangulated;
   - the rest are rasterized in the order they are now, so every pixel, every tie between equal depths and every edge
     comes out as it does now (what is left out is strictly behind, so it could never win a pixel).
   Only opaque elements hide others: one seen through hides nothing, so what is behind it stays.
   Twin: the picture without culling, compared pixel for pixel in tests (the tower, and every section test). Expected:
   the elevation's triangles from 9.3 million to tens of thousands; at 100 storeys, a picture that finishes in seconds.

5. **A section's own scene.** The hidden-line picture rasterized from a lean scene of the survivors: triangles, owners
   and the plane ids the edges need, worked out element by element (in parallel, not one global map), without the
   textures and smooth normals only the shaded 3D view uses. `Scene::new` stays for the 3D view. Twin: `Scene::cut_in`
   on the full scene.

6. **Fragments kept across edits.** Each element's solids and triangles cached by its revision, a component's by
   (family, type, values) and placed as a transform, as the spatial core's piece 6 plans; the section's scene and the 3D
   scene (and the card's mesh, in chunks) assembled from them, and an edit makes again only the fragments of what it
   changed (the change sets of piece 3 say which). The geometry depends on the library, so a fragment's key holds what
   it read of it (a family, a type), as the plan's height bands do (`derived::Group`).

7. **Off the interface's thread.** The picture made on a worker from the model's snapshot (models are shared, not
   copied); the previous picture shown, scaled, until the new one comes; a request for a view no longer shown dropped.
   The drawing over the picture (pieces 1 to 3 make it quick) stays on the frame.

8. **Sheets and PDF.** A sheet of elevations and sections uses the same work: one `SectionWork` and one culled scene a
   sheet, not a scene a viewport; the PDF the same.

## Targets

| | 10 storeys now | 10 storeys | 100 storeys |
|---|---:|---:|---:|
| An elevation's picture, from a model just opened | 9.2 s (frame, scene and picture) | < 1 s | < 5 s |
| The same after an edit (fragments kept) | 8.3 s | < 300 ms | < 1 s |
| The drawing over the picture | 1.5 s | < 150 ms | < 300 ms |
| A section frame that changes nothing (desktop) | 1.5 s | < 16 ms | < 16 ms |
| An elevations' sheet | 14.8 s | < 2 s | < 10 s |
| Memory for an elevation | 9.3 M triangles | survivors only | survivors only |

## Order and risk

1 and 2 first: small, safe, and they take the per-frame cost out of the desktop. 4 is the large gain and the most care:
the culling must be conservative (strictly behind, opaque only) so the picture stays identical, which the pixel twin
proves. 3 and 5 fall out of 4's boxes and survivors. 6 is the structural change, shared with the 3D view and the plan
fragments of the spatial core's piece 7, and goes last; 7 and 8 use what the others make.

## Status

- [x] Plan, with the measurements above
- [ ] 0. Measure in the probe
- [ ] 1. Each thing once a drawing
- [ ] 2. The desktop keeps the drawing
- [ ] 3. Only what the view's volume holds
- [ ] 4. Occlusion culling
- [ ] 5. A section's own scene
- [ ] 6. Fragments kept across edits
- [ ] 7. Off the interface's thread
- [ ] 8. Sheets and PDF
