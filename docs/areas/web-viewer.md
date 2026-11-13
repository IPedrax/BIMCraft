# The web viewer

`web/viewer` is a page that shows a viewable (`bimcraft-cli` `save` to `.glb` with `viewer: true`, `crates/core/src/viewable.rs`)
in a browser, with three.js 0.170.0 and three-mesh-bvh 0.8.3 (MIT), kept in `web/viewer/vendor` and loaded through an import map
of relative paths, so the page asks nothing of any server but the one it is served from (their sources and checked hashes:
`vendor/README.md`; their licences: `LICENSES/`). It only looks: nothing it does changes the model. Open it with
`index.html?file=<url of a .glb>`, or choose or drop a `.glb` on its start card, which also says in plain words why a file could
not be opened (not found, no connection, not a model). While a model comes, the page shows only a ring in the middle of the view
with how far it is (`ring.js`), no words, and nothing for the first 0.4 s: the download of a `?file=` by its length, then the
elements as they are placed. It has no build step; the modules in `web/viewer/js` load as they are, and their tests run with
`node --test web/tests/*.test.mjs` (CI runs them).

A `save` to `.glb` without `viewer` is the scene for Blender (`gltf.rs`), unchanged. The viewable differs in what a viewer needs:
each shape stored once and quantized, each element's properties (what `elements` gives) and the levels in `extras`, and how the
default 3D view opens (parallel or perspective).

## What each file owns

- `main.js`: the `app` the others share (the renderer, the model scene and an overlay scene drawn over it and never cut, the
  camera, the elements by id and GUID, the levels, the selection), its events (`on`, `emit`), picking (`pick`, `cast`: hidden
  and cut-away faces are passed over), the pointer and key routing, hide, isolate and show all, loading and the picking trees.
  The picture is drawn when something changed (`app.redraw()`), not every frame. Without a section only front faces are
  drawn, since a back face lying on a front one (a wall's finish on its core, a lining in a door's opening) fights it for
  each pixel and flickers as the camera moves; a face wound against its own normal is turned round at load (`facesOut`).
  Front faces that lie on each other fight too (a model's lining 0.035 mm off a wall's reveal, a door's head 0.01 mm under
  a lintel, a ceiling on a slab's underside), so beams and columns, then walls and ceilings, then floors and roofs are drawn
  a little further off (`BEHIND`, `PUSH`): the vertex shader moves each corner along the line from the eye by 0.02 % of its
  distance a step, so the face covers the same pixels. A polygon offset does not do: in depth steps it grows with the square
  of the distance, too little up close in Safari and enough at 20 m to bring a cabinet through a wall; with a slope term, the
  same on any wall seen aslant. The near plane follows the view (`fitNear`): 0.1 m while walking, else a fortieth of the way
  to what is looked at. Measured on a large house model, 912 views at eye height moved by under a millimetre: the pixels that
  change fall from 0.76 million (the polygon offset) to 0.23 million, the rest edges moving.
  Pan, Walk and Measure are tools, one at a time: Pan's left button slides the view as the middle button does. Under Pan and Walk a
  click still selects and a right click still opens the menu; only a drag is the tool's. What the
  pointer is over, in the model or a browser row, is lit blue (`app.hover`); a right click without a drag opens a menu
  (`menu.js` says which rows: select, Properties, Focus, Hide, Isolate, Exit isolate or Show hidden, Home), on the element under the pointer or else the
  selection, and plans and sheets have it too, without the rows that need the 3D view. A click selects and never opens Properties;
  Properties opens from that menu and stays open, following the selection, until it is closed. The tool bar keeps what the menu lacks:
  Hide and Isolate are in its Visibility fly-out (`fly`, over its button by `overButton`), and Properties is not on it. There is no
  Show all (it did not go back to the view before Isolate): while something is isolated or hidden a quiet ✕ sits
  just over the tool bar, higher while one of its bars is open (`#quiet`, `quietLabel` in `menu.js`), faint until the pointer is on it, when it widens to say Exit isolate
  or Show 3 hidden. Exit isolate (the ✕, Esc with no tool on, or the menu's row) brings back just what was shown before Isolate (kept
  by GUID) and glides the camera back to where it was; isolating again while isolated keeps the first of both. `cursors.js` draws the cursor of what a drag does: orbit,
  pan, zoom just after the wheel; Walk's is a gun sight whatever is held, a plus with a gap at its middle, as in Autodesk's First Person
  (read from its demo's cursor, drawn here).
- `nav.js`: orbit about the point under the cursor, pan, zoom toward the cursor, pinch, double-click to focus, Fit, Home (the
  desktop's default view, from the south-east, above) and the fly between views. A view is framed in the part of the picture the panels and the tool bar
  leave free (`app.free()`). Fit and Focus keep the way the camera faces but look from 30° above at least, so an element
  picked after walking is not framed side on behind a wall; they, Home and the ViewCube end Walk (`app.leaveWalk`), else the
  next drag put the camera back at the walker's eyes.
- `cube.js`: the ViewCube, drawn on a 2D canvas: what its parts are and what a click does. Its numbers stand in one line
  (`CUBE`). It gives `nav.js` an azimuth and elevation (`nav.turn`, `nav.angles`), so a turn runs the same way round on both. The arrow
  keys and Home work with no input focused.
- Parallel follows the project: a viewable's `scenes[0].extras.view_3d` holds the default 3D view's projection (`parallel`), so
  the page opens as the desktop does; a `home` there (`azimuth`, `elevation`, and `target` and `scale` if placed) is where Home goes,
  else the default Home. In a parallel view the eye stays `nav.cam`, moved as ever, and
  `app.camera` is an orthographic camera that sees as wide as the eye sees `nav.dist` ahead, so `dist` says how much it shows: a
  zoom changes it, a turn or a pan leaves it alone. Its rays start at the back of its clip box, which reaches behind the eye.
  The depth push and the edge pass each have a parallel branch. Walk turns the view to perspective first, as the desktop does.
- `through.js`: See Through, the right-click menu's Transparency. For now, only in the 3D view, walls, floors, ceilings, roofs and the selection go clear, ticked in a further menu that stays open (`openMore`): from off a row makes only that clear, and Off ends it. A thin bar at the side of the view says how clear, 5 to 95 % (70 to begin with): it shows only while something is clear, is faint until the pointer is on it, and goes while the view moves. A click, a hover, Measure and the zoom pass what is clear (`app.solid`); Tab steps back through all that stands under the pointer (`app.under`), so a wall can still be picked, and X turns it on and off. What is clear is drawn with a copy of its material that writes no depth (`clearMat`), throws no shadow and keeps its edges, fainter, inked in a pass of their own (`Look.ink`). It changes no file and no model, a walker is still stopped by the walls, and a page's change or reload keeps it (`adopt`).
- `walk.js`: Walk: eye 1.6 m over the floor, a step up of 0.4 m,
  gravity, every solid element but doors (rooms, masses, rebar, lines and the like are not solid) stopping a body 0.25 m wide, a
  floor being any face flatter than 60°. The gap is kept square to the wall, from three heights and the body's middle and sides, and
  a body nearer than its width to any face round it is pushed out (`clear`): counted along the walk, as it once was, a wall met aslant
  let the eye come within a centimetre and the near plane cut through it. The walker goes straight:
  W and S keep the heading, which turns only with the arrow keys or a drag, and a heading within 4° of a wall nearby is put square to
  it as a walk begins (`square` in `body.js`). Only the solids within 6 m are tried (`nearSolids`, found again every 2 m); on a large house
  model walking costs about 3 ms a frame more than standing. `node --test web/tests/walk.test.mjs` walks a corridor with a pier.
  Starting moves nothing, as Autodesk's viewer: the camera stays where it is and looks where it looked, only turning to
  perspective. Gravity does the rest. With a floor anywhere under the eye it comes down onto the nearest, straight down
  with the view unchanged (9.8 m/s², at most 10 m/s, so 150 m takes 15 s and a room a little over half a second; from over the
  house that floor is the roof). With nothing under it, from outside the model or walked off an edge, it holds its height and
  flies on until something is under it. An eye lower than eye height over a floor stands up on that floor at 1.5 m/s
  (`lift`), which is where it differs: Autodesk's takes a floor less than about 1.25 m under the eye for one overhead and drops through it
  to the storey below. The wheel glides level along the way the walker faces, 2.1 m a notch in half a second, and stops at a
  wall (the desktop's goes through), and a fall goes on under it, so rolling in from outside is a plane coming down. Those numbers
  are what Autodesk's viewer was measured doing, from the camera frame by frame; `body.js` holds them in plain numbers: `node --test web/tests/body.test.mjs` (CI runs it).
  A double-click goes where it lands and leaves nothing selected.
  The near plane stays as the view had it until the walker is on a floor, or a fall from afar would flicker.
  The mini plan is a top view cut 1.2 m over the feet; its × or M hides it, the map button or M brings it back. A press on
  it puts the walker on the floor there (within a storey under the cut) and a drag carries it, as on a web map; the plan
  holds still under the pointer and centres on the walker again when let go. Which way
  a face looks is `hit.normal`, set by `app.cast` in the world (a hit's `face.normal` is in the shape's own frame, which
  its node turns and mirrors; read as it was, porch steps were no floor, so Walk stayed on the ground a metre under the
  front door, eyes at its handles, and slid along the wall under it).
- `follow.js`: the status dot, and a `?file=` followed as the file changes (see below).
- `ring.js`: the loader, with no browser in it but the element it draws on: each step's share of the ring, and the ring that shows after 0.4 s, spins until something is counted, never goes back and says 100 % only as the model shows.
- `body.js`: how the walker's body moves, in plain numbers with no browser in it: a frame of gravity, standing up, the wheel's glide.
- `cap.js`: what a section cuts closed with a flat face on the plane, as a CAD program fills a section. For each plane the
  meshes it crosses are counted through the stencil, back faces against front; a sheet on the plane is drawn where the count
  stands, into the picture and into the normals the ink reads, so a cut is outlined. A mesh that does not close (`closes`
  in `cut.js`: the area vectors of its faces do not sum to nought) stays out and shows its back faces in the cut's colour
  as before: its count would stand over everything behind it. On a large house model (about 12,000 meshes) a frame takes about 50 ms
  with one plane or a box's six, since a mesh no plane crosses is not drawn.
- `section.js`: the section bar: X, Y and Z planes, a plan cut (1.2 m over the main level), a box, a box round the selection,
  Flip and Clear. A plane first cuts through the middle element each way (the median of the elements' middles), not the
  middle of the framing box: on a large house model the plinth pulls the box's middle so far west that an X cut there left a handful of
  its thousands of elements on one side. A plane has a white arrow that slides it along its normal and two rings, red and blue, that tilt it about
  its two ways across. As desktop viewers draw them, a ring shows only its tip, a short arc where the rings cross at the
  arrow's point, until the pointer is on it; the handle under the pointer or in a drag turns yellow (a ring shows whole)
  and the others fade (`Section.over`); a box has a cone on each face. The handles keep their size on screen at any zoom
  (`handleSize`): in perspective a share of how far ahead of the eye they stand, in a parallel view of how much the view shows,
  since a parallel eye's place means nothing to the picture and a zoom can carry it past them. Their presses start at the back
  of the clip box as a pick does (`app.aim`), or a handle behind that eye can't be reached. They follow the view (`Section.place`):
  a plane's handles stand in the middle, by area on screen, of what shows of the part of the plane inside the model's box, within
  what the panels and the bar leave free, so with all of the cut on screen they ride with the model and close up they hold their
  place while it slides under them. Only the point `at` moves, along the plane, so no cut changes. They hide when none of the cut
  is on screen, and hold still in a drag, when the plane is seen edge on, and when it has been slid out of the box (so it can be
  brought back). A box's cones do the same, each on its own face, and each hides on its own. The geometry is `cut.js`, plain numbers
  with no browser in it: `node --test web/tests/cut.test.mjs` (CI runs it). A cut face is filled in the cut colour by drawing the back faces it opens (`cuttable` in `main.js`), and the
  shadows follow the cut: three.js leaves the renderer's own planes out of the shadow map, so each material carries them too (`clipShadows`).
- `measure.js`: distance (two points) and angle (three), snapping to a corner within 12 px and an edge within 8 px. Each
  measurement stays on the model until it is deleted (Delete, or the key) or all are cleared; a click on its label picks it
  and shows its east, north and up parts as red, green and blue dashed legs, each named in its colour beside its own leg (none for a
  leg under a twentieth of the length, or seen shorter than 24 px). The face under the pointer is lit before a click (only the
  triangles on its plane facing its way, and left out of the edges' ink: drawn into it, the lit face was inked in streaks that moved
  with the view). What is under the pointer is found once a frame at its last place (`hover.js`); a press that moves the view hides
  the dot and the lit face, and they are found again where the pointer is when it lets go or the view moves under it. Units (mm, cm,
  m, ft-in, in, ft) and precision are on the bar's Settings and remembered in the browser. Lines and points are drawn each frame in
  an SVG over the picture (`#measvg`, cut where they pass behind the eye), so they are 2 px and 4.5 px at any distance; labels are a
  white pill with black words, the picked one black, and are laid again each frame (`labels.js`: the picked first, each at the first
  of its places clear of the others) so none sits on another as the view moves. A placed point is dragged to move it (it snaps
  again; a press let go picks its measurement); a distance's label is dragged to pull its line off the model, square to it in the
  plane that faces the eye, with lines out to the points. A double-click on a face shows its sizes, as SolidWorks does: the face lit,
  each straight edge's length beside it (the longest 16, none seen under 30 px), its area and, where the shape has a parallel face
  behind it within 3 m, its thickness (`faces.js`: the triangles on its plane joined edge to edge from the one clicked, the edges
  only one has, joined into straight runs). The two clicks of the double-click are undone first. Esc or a click clears it. A shape
  past 600 000 corners gives only the triangle clicked. `node --test web/tests/faces.test.mjs`.
- `panels.js`: the floating panels (Model, Properties, Views), as Autodesk's viewer has them: no header, a name line in its
  place that drags the panel (the selected element, the open view, "Model"), any edge or corner to size it, short by default
  with the rows scrolling inside, no lines between rows, and each row lit under the pointer. A panel stays whole inside the
  model and comes to the front when pressed; `main.js`'s `crowd` moves the ViewCube and a view's title aside only while a
  panel stands by them, and a view is framed clear of the side each panel's middle is on (`clearOf`). Groups fold in
  Properties and Views with a 0.16 s slide and fade. Its numbers run with no browser: `node --test web/tests/panels.test.mjs` (CI runs it).
  Nothing is remembered across a reload. On a phone the panels stay bottom sheets, which neither move nor size.
- `props.js`: Properties as a person reads them: grouped (identity, constraints, dimensions, location, Revit parameters, family
  values, ids), a GUID shown as the level or element it names (an element is a link that selects it), lengths in mm, areas in
  m², volumes in m³, weights in kg, angles in degrees. Location, Revit parameters and Ids start folded, and a group stays as
  it was left from one element to the next.
- `browser.js`: the model browser (the model, then level, category, type, element; rows made when opened), an eye and Only
  on each row (the model's row: show all and go home), and
  search by name, type, mark, category, level, id, or 8 or more characters of a GUID, best matches first.
- `look.js`: the Look menu, edges and shadows, each on or off and remembered in the browser. Edges are found on the picture:
  the model is drawn again with its normals, and a pass inks a crease (normals more than about 35° apart), a break in depth
  and the outline, so they cost the same for any model. A speck is no edge: a pixel whose two sides lie on one face, or a
  side that is such a pixel, is left uninked, so a hairline gap between two faces doesn't flicker as the camera moves. The sun's shadows fall on the building and on a ground under its
  lowest point; the shadow map is drawn again only when what is shown changes, not as the camera moves.
- `views.js`: Views, the plans, ceiling plans, elevations, sections, drafting views, legends and sheets, grouped as the
  desktop's Project Browser groups them. A view is drawn over the 3D picture as SVG, panned and zoomed by its view box; a click
  picks the element under the cursor (within a few pixels) and selects it as the 3D view does, hide and selection carry over,
  a double click on a sheet's viewport opens its view, and Measure snaps to the drawing's corners within 10 px. A file that
  carries no drawings, as `viewable.rs` makes today, lists the 3D view alone.

## The file

`viewable.rs` stores each shape once: an element's corners are told in a frame of its own (from its first corner, toward the
farthest, then the corner farthest from that line), so every copy of a family's type, wherever it stands and however it is
turned or mirrored, has the same corners in its frame. Copies whose corners agree within a few 16-bit steps or 0.05 mm share
one mesh, and each element's node turns it into place with a matrix. Positions are 16-bit across the shape's box, normals
8-bit and colours 8-bit (`KHR_mesh_quantization`, required), so a corner is within one step of where it was: 0.76 mm on a 50 m
floor. On a large house model this took the file to about a seventh of its size, with half as many shapes as elements. Because the frame is
turned, the box three.js gives a node from its mesh is larger than the element; the viewer measures each element's box from
its corners (`setFromObject(n, true)`).

glTF defines `COLOR_0` as linear light, and three.js encodes what it draws for the screen, so `viewable.rs` turns each colour's
sRGB bytes to linear ones (`linear`; alpha stays as it is). Stored as shown, they would be lifted a second time: a roof tile of
(214, 83, 75) showed as (241, 159, 153). In the viewer the sky and the sun sum to full light on a sunlit face that looks up
(`HemisphereLight` 1.95 in `main.js`), so that face shows its model colour, as on the desktop.

## Following a file

On a page opened with `?file=`, `followFile` (`follow.js`) asks the same address for the file's headers every 2 s (`HEAD`, not
cached), and fetches it again when its ETag, date or length changed, keeping the camera, the section and what is hidden; a hidden
tab asks nothing. The status dot in the bottom right corner (`#live`) says how that goes: green as shown, amber fetching, red
when the file could not be fetched. It is as clear as the ViewCube until hovered, only a hover shows its words, and a click on it
fetches the file again by hand.

## The framing box

`coreBox` in `main.js` is the box Home, Fit, Walk and Section start from. It is the elements whose middles lie in the middle
90 % on each axis, then any element near that (within half its span, 3 m at least) and no more than half as big again. So the
sample house's roof is in it, and a long road along a site is not (the road stays in the model and in Fit's far plane).

## Looking at it

Save a viewable (`bimcraft-cli` with `save {"path": "house.glb", "viewer": true}`), serve `web/viewer` and the `.glb` from one
local server (the page fetches the file), and open `index.html?file=../house.glb`; the network log shows no request but to it.
Check with the sample house and a large model at 1280x800 and at 390x844 (a phone: the panels become bottom sheets and the tool
bar keeps its icons only): Home, the ViewCube, Walk from outside (it must land on the ground floor slab and walls must stop it),
each section, Measure on a known wall, a search, a door's Properties, and the Look menu with each of edges and shadows on and off.
