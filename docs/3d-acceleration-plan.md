# The 3D view on the card, at any size

A plan in pieces, as [the canvas's](canvas-acceleration-plan.md) and [the sections'](section-views-plan.md), for the 3D view.

**The goal: a model's size never sends the 3D view to the processor, and no number of triangles is refused.** A 3D modelling tool has to
draw the big model, and the big model is where the processor is slowest. What a card can't hold at once it draws in parts, or over
several frames, or holds fewer of: it doesn't refuse. Orbiting, hovering and selecting stay responsive whatever the size. The picture
once the view is still is the one the card draws today.

## Measured (2026-11-02)

The annotated tower, 10 storeys (`tower {"storeys": 10, "annotation": true}`), 120,011 elements, 1920 × 1080, release, an NVIDIA RTX 2060
over Vulkan, with the card turned on in Settings:

| What | Time | When it's paid |
|---|---:|---|
| Build the scene (`Scene::new`) | 7.3 s | opening the view, **and after every edit**, on the interface's thread |
| Fit the view | 0.85 s | opening the view, on the interface's thread |
| A frame on the processor | 2.7 s | each step of an orbit, and each time the pointer crosses an element, which changes a tint |
| A frame on the card | never reached | `Mesh::new` refuses: "9302680 triangles, more than the card is given" |

Where the 9.3 million triangles are:

| Category | Triangles | Share | Elements | Each |
|---|---:|---:|---:|---:|
| Components (family instances) | 7,118,800 | 76.5% | 62,020 | 114 |
| Conduits | 816,000 | 8.8% | 8,500 | 96 |
| Pipes | 813,120 | 8.7% | 8,470 | 96 |
| Walls | 421,920 | 4.5% | 2,620 | 161 |
| Everything else | 132,840 | 1.4% | 10,830 | 12 for most |

For scale, the card draws the 20-storey highrise (427,280 triangles) in about 1.2 ms a frame ([performance](performance.md#3d-on-the-graphics-card)).
At 100 storeys the same tower is about 93 million triangles; the 3D view doesn't finish opening.

## What stands in the way

Each of these is a limit on size, and all of them have to go. Taking away only the first would change the error, not the result.

1. **`MAX_TRIS`** (`view3d/gpu.rs`): 1.6 million, refused in `Mesh::new`.
2. **The buffers the device was asked for.** egui's default setup requests `wgpu::Limits::default()`, a `max_buffer_size` of 256 MiB and a
   storage binding of 128 MiB, whatever the card has. `Gpu::upload` refuses past them, and one vertex buffer holds all of a mesh.
3. **44 bytes × 3 for each triangle, each copy stored in full.** About 1.2 GB of vertices at 10 storeys and 12 GB at 100, with a copy on
   the processor's side, in `Mesh::vertices`, and `Mesh::tris` and `Scene::tris` beside it.
4. **Every triangle drawn every frame.** Even held on the card, 93 million a frame is not an interactive orbit on this card.
5. **The work on the interface's thread.** Scene, mesh, upload and the first pick's hierarchy (124 ms at 427,000 triangles, so seconds
   here) all stop the window.
6. **A whole rebuild for each edit.** `scene_of` and `mesh_of` start again whenever the model is another `Arc`, so an edit costs what
   opening costs.
7. **What the card doesn't draw at all.** `Mesh::new` also refuses hidden line, wireframe, a section box and a point cloud. Each sends
   a big model to the processor exactly as its size does today, so turning on a section box on a tower would bring back the 2.7 s frame.
8. **Off until asked for.** `gpu_3d` starts false, so a new user with a card still gets the processor.

## The rule on the processor

After this plan the processor draws the 3D view in one case only: **there is no usable card** (no adapter, OpenGL only, or a card that
failed), and the window says so. Sheets, PDF, PNG and the CLI keep `Scene`'s software picture, which is what they are held to; they aren't
the interactive view. Nothing about a model, not its size and not how it is shown, is a reason.

A card that runs out of memory is not a card that failed. The out-of-memory error, which the card's error scopes catch today and answer
by giving up for good, becomes a signal to hold less (piece 9).

## Pieces

Each keeps the workspace green. Tests stay as they are for what they check today: the card against the processor's picture
(`the_card_draws_what_the_processor_does` and those beside it), and picking through the mesh against the picture. Each piece also compares its
settled picture, byte for byte, with the card's picture before the piece, on a model that fitted then, in both projections.

1. **No cap, and the card's real limits.** Ask the adapter for what it has (`Limits` from `adapter.limits()`, in `main.rs`, and the same in
   a test's headless card). Take out `MAX_TRIS`. A mesh goes up in as many vertex buffers as it needs, each within `max_buffer_size`, one
   draw for each. Turn `gpu_3d` on by default where a card exists. This alone puts the 10-storey tower on this card, at about 1.2 GB, with the
   stall of building and uploading it. Measure it: it is the base the later pieces beat, and the first number for how the frame scales.
2. **Scene and mesh off the interface's thread.** `Scene::new`, the fit and the mesh on `view_worker.rs`, cancellable, snapshot-bound, as
   sections do it, published together. Opening a view shows it empty with its progress, and the window stays live. The first pick's
   hierarchy is built there too.
3. **One shape, many places: components.** The card holds each family and type's triangles once, and an instance buffer holds each
   copy's transform and owner. Built from the geometry `component_geometry.rs` already reuses, without making each copy's triangles at all.
   To stay the same: a mirrored copy's winding (`front_facing`), a normal under its transform, a texture that lies by world position,
   and **each copy's plane numbers**, because an edge is drawn where the plane changes between pixels, so two touching copies must
   not read as one. A component with values of its own that change its shape is a shape of its own. Picking goes through a hierarchy of
   instances over each shape's, so a hover stays under a millisecond.
4. **The same for runs.** Conduits and pipes are 17.5% here, at 96 triangles each: one tube per section and segment count, scaled along its
   length, with its normals and edges held to today's. Then what's left is what really differs from element to element.
5. **Draw what the view can see.** Elements grouped by tile and storey, each group with bounds, using what `derived.rs` keeps. Groups
   outside the view are left out, on the processor's side first, then by an indirect draw if that's needed. This must not change one pixel:
   it only leaves out what can't reach the picture.
6. **A frame never waits for all of it.** Past what fits in a frame's time, draw nearest groups first and finish over the next frames into
   the same targets, as the plans' worker finishes the exact drawing after a pan. While orbiting, what isn't reached yet stands in as
   its group's box, or a coarser shape if measuring says that isn't enough. When the view stops it completes, and the settled picture is the
   exact one. This is what makes 100 storeys interactive, and the only piece that shows something different on the way there.
7. **An edit changes only what it changed.** The scene and the groups brought up to date from the elements an edit touched, with what
   `derived.rs` says depends on them (a wall's joins, a host's openings). The full build stays as the twin the tests hold it to, as in
   [the spatial core](spatial-core-plan.md). Overlaps the plans' piece 6 on the same dependencies, so whichever comes first builds them.
8. **What only the processor draws now.** On the card: a section box (clip planes; the processor draws no cut faces, so nor does the card), then
   hidden line and wireframe (from the edge pass), then point clouds (as points, with the same budget as piece 6). Each one held to
   the processor's picture, as the card's shaded view was. Until each lands, say in the view why it is on the processor.
9. **When the card runs out.** Nothing is refused here either. A budget that starts from what the card lets us allocate, and shrinks when an
   allocation fails: groups not in view let go first and uploaded again when wanted, then stand-ins for the farthest. The most a card can
   hold then decides how sharp a far storey is while moving, never whether the model draws.

## What must stay the same

- **The settled picture.** Pixel for pixel what the card draws today. Pieces 3, 4 and 5 are never allowed to change it; piece 6 only
  before the view stops. Where the card already differs from the processor (level lines, about 0.12% of a house's pixels) it stays as is.
- **Picking.** What's under a pixel is what that pixel shows, including through a stand-in being replaced and through what's seen through.
- **Hover and selection.** A tint is still a write to an owner, never a redraw of geometry.
- **Exports.** PNG, PDF, sheets and the CLI are untouched by this plan.
- **Never crash.** Everything the card can refuse is caught and answered, as `caught` does today.

## Piece 1 measured (2026-11-02)

The same tower and picture, on the same card, in release, with no refusal now and 5 buffers of 256 MiB at most:

| What | Before | Now |
|---|---:|---:|
| A frame | 2,700 ms, on the processor | **55 ms** median, 149 ms p95; 0.3 ms of it on the interface's thread |
| Build the scene | 7.7 s | the same, piece 2 |
| Build the mesh | never | 3.0 s, piece 2 |
| Put it on the card, the first frame | never | 2.2 s, piece 2 |
| The first pick, for the hierarchy | | 5.5 s, piece 2 |
| Each pick after that | | 2.0 ms |
| A frame with a new tint | 2,700 ms | 157 ms, not looked into |
| The process's memory | | 5.4 GB, and 0.4 GB more once picked |

`a_mesh_in_many_buffers_draws_the_pixels_of_one` holds a split mesh to one buffer's bytes, across the line to what is seen through and in both
projections. On this card a buffer may be 4 GiB and a binding 2 GiB. Still refused for size: textures or owners past a binding, and an
out-of-memory error still gives the card up for good (piece 9). At 100 storeys `Scene::new` alone is the wall: about 2.6 GB of it per 10
storeys, so about 26 GB and a minute, and the mesh double that again, until pieces 3 and 4. A window on `--tower` should stay out of the 3D
view until then.

The card is on by default for a new user. A `settings.json` already holding `gpu_3d: false` keeps it, as it can't tell a choice from the
old default.

## Piece 2: view work off the interface thread

The 3D view builds its scene, initial fit, card mesh and first-pick hierarchy on `view_worker.rs`. The first card upload has its own
worker; a new model shows a loading view until both the scene and upload are ready. Later Zoom to Fit and Home fits also run on a
worker. A superseded snapshot is cancelled during scene, mesh, hierarchy and upload construction, and its result cannot replace the
current model or camera. Switching away from 3D cancels pending work. The test harness builds inline for deterministic view tests.

On the annotated 10-storey tower at 1920 × 1080, release on the RTX 2060, the headless worker path measured **0.14 ms** to request
and start scene work, **16.58 s** until scene, fit, mesh and picking hierarchy completed, **0.01 ms** to request and start the card
upload, and **1.78 s** until upload completed. The work still takes time, but it no longer holds the requesting thread. This probe
does not measure complete egui frame latency or the first shader compilation.

The UI library's 636 tests passed, including the card's picture and picking comparisons.
`worker_upload_keeps_only_the_latest_mesh_and_draws_the_same_pixels` compares synchronous and worker uploads byte for byte in
parallel and perspective; the mesh cancellation test checks that a discarded hierarchy can be built again.

## Piece 3: one shape, many places (2026-11-03)

`component_geometry::SharedComponents` groups component instances by local family shape (mirroring and the parameters the family
evaluates are in the key; paint and phase metadata aren't). `Scene::new` keeps the shapes and notes each component whose run of triangles
is its shape placed, compared to the last bit (`Scene::placements`). `Mesh::new` draws a copy from its shape when each of its triangles has
the colour, texture, face, curved normals and transparency of the shape's first copy, so a painted copy, or one that the view colours or
makes see-through on its own, is drawn from its own triangles as before. On the card a copy is an instance: its place, turn, owner,
owner seen through, and where its plane numbers start. The plane numbers stay the scene's, one word per placed triangle
(`instance_planes`), because the scene numbers planes across the whole model and an edge is drawn where the number changes: two touching
copies, or a copy on a floor, keep the edges they had. A shape's opaque triangles go in the opaque pass and its see-through ones (a lamp's
diffuser) in the see-through pass, under the copy's second owner. A pick walks one hierarchy over the triangles and copies, then the copy's
shape's own with each box placed, and tests the very triangles the scene holds. Ties go to the first in the scene, so picking
is unchanged: the processor-picture tests pass as they did.

`copies_placed_draw_the_pixels_of_their_own_triangles_and_tubes_of_their_runs` holds the instanced picture to `Mesh::flat` (every copy its own triangles, the
picture before this piece) byte for byte, on the sample house with three lamps that are seen through in part, parallel, in perspective and
from inside, with a copy tinted and without. The core's `copies_hold_the_flat_triangles` checks each copy's placed vertices against the
flat ones.

The annotated 10-storey tower, 1920 × 1080, release, RTX 2060, through the worker path:

| What | Piece 2 | Now |
|---|---:|---:|
| Copies drawn from a shape | none | **all 62,020**, from 20 shapes of 2,845 triangles |
| Triangles the card holds one by one | 9,302,680 | 2,183,880 |
| Scene, fit, mesh and pick hierarchy, on the worker | 16.58 s | 11.80 s |
| Upload, on the worker | 1.78 s | 0.61 s |
| A frame (piece 1's: 55 ms median, 149 ms p95) | | **7.7 ms** median, 11.3 ms p95 |
| A pick | 2.0 ms | 1.4 ms |
| The process's peak memory | 5.4 GB | 4.9 GB |

`Scene::tris` still holds every copy's triangles for the processor, sections and planes, so memory is mostly the scene's now. Making the
scene keep copies as placements, its planes found per shape, is what stands between this and 100 storeys, along with piece 4.

## Piece 4: the same for runs (2026-11-03)

A round run's tube is now made as one tube placed: `runs::unit_tube` (length 1, radius 1, along x) and `Geom::tube_place`, its line along x
and its radius along y and z, so `Geom::tube` is the placed unit tube to the last bit and the scene notes every round run as a copy, of one
tube for each material. A copy's place became a point and three axes square to each other (`ComponentInstance::axes`) instead of a turn
about the vertical; a component's is still exactly the turn its pieces are placed by, since the terms it adds are zero. On the card a normal
goes along each axis by its share over the axis's length squared, which keeps it square to a stretched surface. The caps' and sides' face
normals and the scene's curved normals hold within the 1e-6 a copy is checked to. Rectangular runs (cable trays, ducts) stay their own triangles: a sloped one's ends are
upright, which is a shear, and they are few here.

`round_runs_are_one_tube_placed_stretched_and_turned` (core: level, sloped, upright and at an angle in plan; picking as the picture) and
`copies_placed_draw_the_pixels_of_their_own_triangles_and_tubes_of_their_runs` (the card, byte for byte against `Mesh::flat`) hold it.

The same tower, picture and card:

| What | Piece 3 | Now |
|---|---:|---:|
| Copies drawn from a shape | 62,020, from 20 shapes | **78,990** (every component and round run), from 21 shapes of 2,941 triangles |
| Triangles the card holds one by one | 2,183,880 | 554,760 |
| Scene, fit, mesh and pick hierarchy, on the worker | 11.80 s | 11.85 s |
| Upload, on the worker | 0.61 s | 0.19 s |
| A frame | 7.7 to 8.2 ms median | 8.8 to 9.1 ms median, 12.3 to 13.1 ms p95 |
| A pick | 1.3 to 1.4 ms | 1.1 to 2.5 ms, across three runs |
| The process's peak memory | 4.9 GB | 3.9 GB |

A frame is about a millisecond slower: 1.6 million more triangles go through the copies' vertex shader, which places each normal through
three axes. It's still drawing all 9.3 million each frame; piece 5 is what takes the frame down. What the card holds is now mostly the scene's
own: `Scene::tris` still keeps every copy's triangles, as piece 3 says.

## Piece 5: draw what the view can see (2026-11-03)

A frame draws only what can reach its picture (`Mesh::visible`). What's left out lies wholly past one side of the clip volume, so it leaves
no pixel; what's drawn keeps the order it had, because a tie between touching faces goes to the first drawn, and picking (which settles ties
by the scene's order) is untouched. The triangles drawn of their own are grouped in runs as they are drawn (`Mesh::groups`: at most 1,024
triangles and a sixteenth of the mesh's width, never across `clear_from`). Each copy is tested on its own: a shape's copies follow the
scene's order, which skips about a floor, so a run of them is as wide as the floor and groups of them left out almost nothing. Each group
and copy is a sphere, tested against the six sides as planes and padded for the card's f32; a coarse grid of cells (12 across the mesh's
box) settles first what lies wholly in view or wholly out, and the whole mesh in view is taken whole. The card draws the runs of triangles,
joined across gaps of up to 2,048 left out, and gathers the copies a view sees, in their order, into one buffer (`Uploaded::seen`), so a
shape is still one draw; it is written again only when what's seen changes. The shader is as it was.

The plan said grouping by tile and storey from `derived.rs`; that would have reordered what is drawn, and so the ties. Groups in the order
drawn and copies one by one leave out as much without it.

`groups_hold_what_is_drawn_in_order_and_a_view_leaves_out_only_what_it_cannot_see` (core: every triangle and copy left out lies past one
side, in clip space as the shader puts it; seen whole, nothing is left out) and `leaving_out_what_the_view_cannot_see_draws_the_same_pixels`
(the card, byte for byte against drawing all of it, near, from inside, in both projections, seen through and tinted) hold it. Made to leave
out too much, by shrinking the boxes or misplacing a gathered copy's planes, the second fails by hundreds of pixels.

The same tower and card, the camera turning 3° a frame, median of 35 frames, on a quiet machine:

| View | All drawn | Left out | Drawn, of 633,750 triangles and copies (with the gaps a run joins across) |
|---|---:|---:|---:|
| The whole tower, fitted | 10.0 ms | 9.6 ms | all |
| 4 × nearer, on a corner | 10.6 ms | 10.7 ms | 328,012 |
| 16 × nearer, on a corner | | 6.4 ms | 158,034 |
| From inside, in perspective | 7.4 ms | 3.8 ms | 189,328 |
| Nothing in view | | 0.3 ms | none |

What's left out is where the frame goes down: from inside it halves. Nearer in an axonometric view the corner still sees a slanted column
through every storey, so it leaves out half and saves nothing; what a frame spends there is drawing what is seen, which is piece 6's.
Working out what is seen takes 0.7 to 1.7 ms of the processor's a frame where the view's edge crosses the model, and 0.04 ms where nothing
is in view. The frame with everything drawn was 8.8 to 9.1 ms in piece 4's runs; frames on this machine vary by about a millisecond from
run to run, so the two columns here were measured in the same run.

## Piece 6: a frame never waits for all of it (2026-11-03)

A frame draws at most a budget of triangles (`FRAME_TRIANGLES` in the desktop's `gpu.rs`, 12 million, a copy counting its shape's
triangles; `Mesh::cost`). What a view sees within it is drawn as before, in one frame. Past it, the camera moving, the frame draws what
is nearest first (`Mesh::nearest_first`: the groups and copies seen, by how near the front of their sphere is, taken until the first that
doesn't fit) and a box for each of the rest: a unit cube (`Mesh::cube`, after the shapes) placed as a copy over the group's or copy's
box, grey, with the owner of what it stands for and planes of its own, so its edges are drawn. The nearest are drawn in the order they
had, so where they overlap they tie as before.

When the camera stops, the next frames draw the exact picture: everything seen, in its order, a budget's worth a frame, into the same
targets (the first part clears them, the rest load them). The post pass and the lines run only on the frame that completes it, so the
window keeps the moving picture until the exact one is whole, never a half-drawn one. The settled picture is the one drawn in one go,
byte for byte. A tint changed over a completed picture is the post pass alone. `Gpu::draw` says whether what it drew is the exact
picture, and the 3D view asks for another frame until it is.

`past_a_frame_s_budget_the_view_stands_in_while_moving_and_settles_on_the_exact_picture` (a budget of a seventh of the view: the
moving picture differs, the frames that follow keep it, and the one that completes, three frames or more on, is equal to the one-shot
picture; a tint after it is exact at once), `parts_taken_one_after_another_draw_each_triangle_and_copy_once` and, in the core,
`nearest_first_draws_the_nearest_within_a_budget_and_a_box_for_each_of_the_rest` (nothing drawn is farther than a box; each box holds
what it stands for) hold it.

The 10-storey tower is within the budget in every view on this card, so its frames are as piece 5 measured; the budget is for models
past it, as 100 storeys.

## Piece 7: an edit changes only what it changed (2026-11-03)

After an edit the 3D view's worker brings the scene it has up to date (`Scene::update`, `view3d/update.rs`) instead of making it whole.
What changed comes from `Model::changed_since`; an edit of the library or the project's settings is the whole scene again, as is one that
reaches a quarter of the elements. The elements the edit touched are made again, each painted and its faces named as the whole scene
does, with what points at them and what they point at (`Element::refs`, `Lookup::referrers`: a door's wall, a wall's doors, a level's
elements). Then, round after round, what touches an element that came out otherwise is made again: what its box touches, grown 10 mm, a
slab under a shaft's reach, and the walls whose location lines come within the thickest wall of it (`Lookup::walls_near_at`), since a
wall's end joins a wall it stops near and its own box, cut back at the join, may lie farther off. What comes out as it was ends the round.
Every other element's run of triangles, planes, faces and normals is kept (`Scene::spans`).

What made the scene depend on where an element's triangles are is gone. A curve is numbered by its element (`local_curves`), not by
where its first triangle lies; a plane by the planes the scene found before (`PlaneTable`, kept with the scene, so an edit's triangles
share a plane with those they lie in); a copy by its shape's key (`ShapeKey`), a new shape added after the rest. The plane numbers a
card draws stay below the cube's, which `Mesh::new` now numbers past `PlaneTable::next`. Two things the scene held were found again
for nothing, and went: each textured triangle's way across (`basis`, a function of the triangle, now worked out where a texture is
looked up: 0.5 GB of the tower's scene), and the model's extent for levels without one of their own, which made every solid again
(now the box round the elements' boxes, the same extent).

`Scene::new` is the whole computation the update is held to. `a_scene_brought_up_to_date_by_an_edit_is_the_scene_made_whole` (the
house: a wall moved, an end moved, a height, a level's elevation, paint, a shaft, a table moved and copied, a chair, a door deleted, a
wall added, a material's colour, a wall deleted, then undo, redo and undo) and `random_edits_of_a_tower_bring_its_scene_up_to_date_as_made_whole`
(50 of the oracle's random edits) compare each update, carried on from the last, with the whole scene: every element's triangles, its
faces and normals, planes shared by the same triangles, the same copies of the same shapes, and the same picture.
`an_edit_makes_again_what_it_touched_not_the_model` keeps a wall's move to under a twentieth of a tower's elements, and
`after_an_edit_the_worker_brings_the_last_scene_up_to_date_and_draws_the_whole_scene_s_picture` runs it on the worker.

The card's mesh is still made whole from the scene after an edit, but three of its passes over every triangle went to each element's
run instead. Whether the view shows a triangle, and how see-through the view makes it, are asked once an element. The box round what is
shown is the box round the elements' boxes. An owner's number is remembered from the triangle before. And the copies after each
shape's first are held to it on the render threads (the first of each still found in the scene's order, so the same copies are chosen).
The card's pictures are as they were: its 18 picture and picking tests pass byte for byte.

The annotated 10-storey tower, release, a quiet machine (`update_speed`):

| Edit | Made again | Found | The scene up to date | The whole scene | The mesh, before → now |
|---|---:|---:|---:|---:|---:|
| A wall moved | 199 elements | 0.04 s | **0.87 s** | 7.2 s | 1.40 → **0.52 s** |
| A component moved | 19 | 0.03 s | **0.89 s** | 6.9 s | 1.31 → **0.52 s** |
| A level's elevation | 18,370 | 0.38 s | **1.61 s** | 6.8 s | 1.38 → **0.56 s** |

So an edit's scene and mesh went from about 8.5 s to about 1.4 s. What's left of the scene's 0.9 s is copying what is kept: 9.3 million
triangles and what goes with each, about 1.4 GB. Sharing what is kept rather than copying it takes the scene's lists held in parts,
which every picture reads; the mesh likewise, with its upload. A level's elevation numbers the planes of 18,000 elements' triangles
again on one thread. The pick hierarchy and the upload are still made whole after an edit.

## Piece 8, first part: a section box on the card (2026-11-04)

The plan said clip planes and cut faces. The processor draws no cut faces: it keeps a pixel where the point it shows lies in the box, so a
cut wall is hollow and "cut faces aren't capped yet" is what the view does. The card is held to that picture, so it draws none either; capping
is a change to both painters, not to this piece.

The mesh holds all it would hold without a box (`Mesh::section_box` only says which), and the fragment shaders discard by the point
they already interpolate (`fs_scene_cut`, `fs_clear`), the box a uniform from the mesh's origin. So a face being dragged builds nothing:
`Gpu::draw_cut` takes the box, and a box that moved starts the picture again as a moved camera does. The opaque surfaces under a box have
pipelines of their own, as a shader that may discard gives up the depth test that runs before it, which a view with no box keeps.
`Mesh::visible_cut` adds the box's six sides to the clip volume's, which is what makes a box cheap on a tower. A pick asks `Boxed` as the
picture does, a corner at a time and then the pixel, and the levels go through one function (`levels_in`) for both painters.

Looking at the pictures found what the test's tolerance let through: a see-through wall drawn over the wardrobe flush against it, an
area of 0.4% of the picture, under the 0.5% allowed. Their faces lie in one plane. The processor's depths come out equal, and a tie is behind;
a card snaps each triangle's corners to 1/256 of a pixel, so on a slanted face, at about 25 mm of depth a pixel, the two come out about
0.1 mm apart. `fs_clear` now calls as near what is within a sixty-fourth of the depth a pixel crosses. A box only showed it, by cutting
away what stood in front. `what_is_seen_through_flush_against_what_is_opaque_lies_behind_it_as_on_the_processor` counts pixels with all four
neighbours differing, and fails at 783 without this.

**Left as it is:** the same tie between two opaque surfaces, in perspective, at a copy against a triangle of its own. Part of the wardrobe's back shows through
the wall's face (163 pixels in that view) where the processor shows the wall. Moving copies back by a tie cleared it but changed two pixels of
`copies_placed_draw_the_pixels_of_their_own_triangles_and_tubes_of_their_runs`, which this plan doesn't allow, so it wasn't kept. A card's own depth bias on the copies' pipelines is the
next thing to try, if it is to be fixed. It can be seen today without a box, from where the faces are in view.

`the_card_cuts_to_a_section_box_as_the_processor_does` (parallel, tinted, perspective, from inside, seen through),
`a_box_being_dragged_draws_the_pixels_of_the_box_it_will_be_and_leaves_out_none_of_them` (byte for byte: the mesh of one box cut to another
against that box's own mesh, and culled against all of it drawn), `past_a_frame_s_budget_a_section_box_settles_on_the_picture_drawn_in_one_go`,
`picking_the_mesh_through_a_section_box_finds_what_the_cpu_picture_shows` and `a_section_box_stays_on_the_card_and_what_it_cuts_away_is_not_picked`
hold it.

The annotated 10-storey tower, 1920 x 1080, release, RTX 2060, the camera turning 3 degrees a frame (9.3 million triangles; the processor's frame was
2.7 s). The whole tower with no box was 8.9 ms in the same run:

| Section box | A frame, median | p95 | Triangles drawn |
|---|---:|---:|---:|
| A storey's height | **3.3 ms** | 3.8 ms | 1.1 million |
| Half the tower | **7.0 ms** | 7.5 ms | 4.8 million |
| A face dragged, the box growing past half, the camera still | **11.3 ms** | 11.7 ms | 4.8 million and up |

When the drag ends, the command runs and the mesh is made again from the scene, as after any edit (piece 7), though nothing in it changed.

## Piece 8, second part: hidden line on the card (2026-11-04)

Hidden line is the same passes with less in them. `Mesh::lines` says so; the mesh is built with nothing seen through (glass is opaque, as in a
section's picture, and drawn in the scene's order), no textures, and one colour, so copies that paint had told apart are one shape again. The post pass paints surfaces
and ground paper where the flag is set (`Globals::ground.w`), and edges and tints come out as they do when shaded. Picking needed nothing: a triangle is
simply not `clear`. On a dark canvas the processor's picture goes through `canvas::color` a pixel at a time, which swaps paper and ink;
the card does the same sum in `inked` at the end of the post and line passes (`Gpu::swapped_ink`), since the edges' and tints' colours would otherwise need writing again.

`the_card_draws_hidden_line_as_the_processor_does` (parallel, tinted, perspective, from inside, cut to a box),
`in_a_dark_canvas_hidden_line_swaps_paper_and_ink_as_the_processor_s_picture_is_swapped` (every pixel, from the light picture through `canvas::color`'s sum),
`picking_the_mesh_in_hidden_line_finds_what_the_cpu_picture_shows_glass_and_all` and `hidden_line_stays_on_the_card_and_picks_what_it_shows` hold it. On the tower, in the third part's table.

Tests of the card crashed about one run in seven here (SIGSEGV in the Vulkan loader's `loader_get_icd_and_device`, under `SetDebugUtilsObjectNameEXT`, from tests that had nothing to do with this
work; two core dumps agree). wgpu names objects in a debug build, and tests make cards side by side. `headless` and `retained`'s test card now ask for no
debug names; 30 runs in a row are clean. A release build doesn't take this path: `InstanceFlags::DEBUG`, which asks for the extension, is set only under `debug_assertions`. A debug build of the window does, with its one card. The tests' cards also lose the Vulkan validation layers' flag; wgpu's own validation, which `caught` reads, is unaffected.

## Piece 8, third part: wireframe on the card (2026-11-04)

The processor steps along each edge, no step longer than a pixel, and inks the pixel each point is floored into, so its lines aren't the ones a card rasterizes: the same quads with every pixel inked differed in
half the ink (10,987 of 21,121 pixels). So each edge is two triangles a little wider than the line, an instance of one list on the card (`Mesh::wires`), and `fs_wire` asks of each pixel whether one of the line's steps lands in it: only
the few steps along its longer way that can (`Scene::wire_pixels` is the processor's side). `vs_wire` cuts each to the section box, to the eye and to the picture, as `wire_line` does, so a box being dragged and a moved camera ask nothing of the processor per line. Later edges
are over earlier ones, as a card keeps the order of a draw; `vs_wire_tinted` then draws only what is tinted over the rest, and only when something is. The mesh has no surfaces and no level's marks, and a view with a scan in it stays on the card, as the processor's wireframe draws no points. The post pass paints the paper.

A pixel picks the last drawn of the lines within two pixels of it, tinted ones last (`Mesh::pick_wire`), through the same hierarchy, over the lines' boxes, each candidate stepped as it is drawn.

`the_card_draws_wireframe_as_the_processor_does_a_pixel_at_a_time` compares to the byte: in parallel, tinted, in perspective, cut to a box and with a category hidden, no pixel differs on the sample house. Only where a line's end is cut do any: 15 of 40,896 inked with lines
past the picture and 45 of 31,382 with lines past the eye, what an f32 finds of a cut against an f64. `in_a_dark_canvas_wireframe_swaps_paper_and_ink_and_a_box_being_dragged_cuts_its_lines`,
`picking_the_mesh_in_wireframe_finds_what_the_cpu_picture_shows_within_two_pixels_of_a_line` (every third pixel) and `wireframe_stays_on_the_card_and_picks_what_it_shows` hold the rest. Each was seen to catch its fault: the stepping, the tinted pass, the box and the tint in a pick each taken out.

The annotated 10-storey tower, 1920 x 1080, release, RTX 2060, the camera turning 3 degrees a frame (`tower_in_the_styles_of_lines`):

| | Hidden line | Wireframe |
|---|---:|---:|
| What it draws | 9.3 million triangles | 5.8 million lines |
| A frame, whole, median | **10.0 ms** (p95 28.3 ms) | **11.7 ms** (p95 12.2 ms) |
| A wall selected | 8.8 ms | 12.9 ms |
| From inside | 2.8 ms | 4.9 ms |
| The processor's picture | 2,259 ms | 2,017 ms |
| A pick | 0.9 ms | 1.05 ms |
| Mesh, from the scene | 0.5 s | **15.9 s** |
| Picking hierarchy | 0.26 s | 2.3 s |

Two things are left of wireframe, neither of which changes its picture:

- **Finding the edges takes 16 s on this tower**, all of it `Scene::edges`' map over every triangle's three sides. The processor pays it too, on the interface's thread; the card's is on the worker and is abandoned with its view. They are kept with the scene, so it is the first wireframe after an edit that waits. Finding them from the shapes (a copy's edges are its shape's, but for where it touches another) or by element is the next thing to try.
- **Lines have no budget of their own.** Every line goes to the card every frame, none left out by what the view can see (hence "inside" at 4.9 ms is the vertex shader's cutting, not culling). At this tower it fits a frame; at 100 storeys, by proportion (not measured), a frame would be about 120 ms, and piece 6 doesn't yet reach it. Grouping lines as `Mesh::groups` does triangles would give both culling and a budget.

## What isn't decided yet

- **How far a card gets.** Piece 1's measurement decides how much of piece 5 is on the processor's side, and whether piece 6 needs coarser
  shapes or only boxes. Piece 6 has boxes, and a fixed budget, not one measured from the card's frames. Occlusion culling is left out until a measurement after piece 6 asks for it.
- **The rest of the scene's memory.** `Scene::tris` holds each copy in full for the processor and for sections. Piece 3 stops the card
  needing it; sections and exports may still build it, which [the sections' plan](section-views-plan.md) follows.
- **`--tower`.** Done in piece 1, as a window can't be told to build the tower and its file is past the size a project may open at.

## Status

- [x] Measure
- [x] 1. No cap, and the card's real limits
- [x] 2. Scene and mesh off the interface's thread
- [x] 3. One shape, many places: components
- [x] 4. The same for runs
- [x] 5. Draw what the view can see
- [x] 6. A frame never waits for all of it
- [x] 7. An edit changes only what it changed (the scene; the mesh, pick hierarchy and upload are still made whole, faster)
- [ ] 8. What only the processor draws now
  - [x] A section box
  - [x] Hidden line
  - [x] Wireframe
  - [ ] Point clouds
- [ ] 9. When the card runs out
