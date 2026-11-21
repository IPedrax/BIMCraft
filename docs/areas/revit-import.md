# Revit files and the Revit import

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

- Revit files: Autodesk's files are never committed. Download sample projects into folders of their own outside the repo (the
  URLs, sizes and hashes are in [docs/rvt-provenance.md](../rvt-provenance.md)), then `BIMCRAFT_RVT_SAMPLES=<folder> cargo test -p
  bimcraft-rvt` runs the real-file tests (they skip when it is unset; the variable takes several folders, parted like a PATH) and
  `bimcraft-cli info x.rvt` shows what a file holds. `bimcraft-cli run x.rvt --save /tmp/plan.png` imports one (`--cmd 'render
  {"view": "{3D}", "path": "/tmp/3d.png"}'` for the 3D view; look at both) and `--save out.ifc` writes it as IFC, which IfcOpenShell
  validates (`express_rules=True`) and builds shapes of, to compare with rvt-rs's own export. After touching the reader, compare it
  with rvt-rs built from its own clone and target directory on the same files (a measuring stick, never a dependency), pin what
  matched in `crates/rvt/src/tests/rvt_rs.rs` as numbers (never Autodesk's strings), extend the mutation sweeps (`tests::sweep` for a
  new kind of element), and add a row to the provenance document for every module and every format finding. Mutation checks of the
  reader go on a scratch copy of the workspace with its own target, as the other crates' do.

  The import (`crates/rvt/src/import.rs`) reads a Revit file into a `bimcraft_core` model and says what it left out. `build` runs one
  survey of the file (`inventory::Survey`: the records of the categories it maps, the type records, the levels), then one reader per
  kind in this order, because each reads what the one before made: levels, walls, floors and ceilings, roofs, doors and windows
  (hosted in the walls made), columns, rooms (their point found among the walls made). A reader takes the newest copy of a record
  across the file's partitions, converts feet to millimetres (`mm`, `micron`), and builds elements the way a command would; it makes
  no rule of its own. `finish` runs `Model::check` and drops, with the rule's message, whatever fails it (a wall takes its doors along)
  until the model passes, so what comes back always checks. A reader never invents: what a record does not say, or the reader cannot
  decode, becomes a line of `Imported::skipped` through `Import::left_out` or `note`, grouped by reason with the ids. Every length it
  takes is within `records::MAX_FEET` of zero, since the importer asks the model for geometry (a room's region) before the check.
  Wall sides follow the file's flip (set: the exterior is on the right, so start and end swap and an arc's sign changes); an opening's
  host is the nearest wall below its own id in its reference list, else above, and its hole is its box along an axis-parallel wall;
  a column is its box's rectangle (a turned one: its symbol's size and the placement's angle); a floor, ceiling or roof is the loops
  of its `OST_SketchLines` (the owner of a curve is the one id among its second reference list's that is a floor, a ceiling or a
  roof, first, middle or last; a curve's record reads as both a line and an arc, and the box it carries says which it is);
  a floor edited with Revit's shape tools comes in shaped by its top's vertices, which its data holds, its top at the height most
  of them have (the creases between them are BIMCraft's own, and a variable layer's level underside is not read); a sketch that
  does not close is tried again without the lines that cannot bound it (another sketch's, past the element's box, and a line
  drawn twice); a roof by footprint comes in level (one that rises is a stand-in, its slopes unread), one by extrusion only where its box
  confirms it, and a wall that names a flat roof or one by extrusion has its top cut by it. A room tries the
  middle of its box and then a grid of points over it, and making the arrangement of a level's walls is what costs (`geom::faces`
  splits and matches every segment), so the faces of a level are made once and every point of every room of that level is asked of
  them (`geom::region_in`): the Snowdon sample's import went from 133 s to 2.4 s in a release build when the rooms stopped remaking
  their level's plan for each point. The notes are part of the product: a change that leaves something out says so.

  A workshared (central) file up to 2023 differs in two ways the survey allows for. A record's header tag is followed by a counted
  list of 14-byte items, empty in a file that is not workshared, so `records::id_before` finds a record's id back from its category
  (14 bytes, and 14 more for each item) instead of at a fixed distance from the marker; and the element table's two id columns differ in
  many records, an element's record carrying either, so `elem_table::parse` lists both. A real 128 MB central model inflates its one
  partition to 664 MiB, which is why `Limits::max_inflated` is 2 GiB. Such a file imports what the importer has a reader for (its
  levels, walls, floors, pipes, ducts and cable trays as runs, and its fittings, fixtures, equipment and terminals as boxes of a family of their category, `generic.rs`). A category the name table has no name for (`inventory::category_name`) is not
  guessed at: its placed elements come in as boxes too, of a family named by the category's id (`Category -2008123`), and the note
  says its category has no name here; one whose box is the box of an element of a named category is that element's companion (every
  pipe, fitting and duct has one in such a file), so it is left out and listed. The categories that have a name and no reader (model
  lines, rebar, areas, assemblies, wall reveals) are listed in the notes with how many there are.

  The categories the name table knows that had no reader come in from the survey's own records, their category and box, with no
  new finding in an element's data (`framing.rs`, `generic.rs`'s `KINDS`). Structural framing is a beam along its box's longer side,
  through the middle of the box across it, its top the box's top on the level at or below it, its section the box across it; a member
  whose box runs across the axes in plan (wider than a third of its length) or stands (as deep as it is long) is left out, since its
  line is not read and its box would make a beam of another width and direction. Structural foundations are isolated footings of their
  box (a wall foundation or a mat too). Stairs' runs, landings and supports, ramps, railings, curtain panels and mullions, wall sweeps, slab
  edges, conduits, furniture systems (of the furniture category), site pads, masses, trusses, beam systems, stiffeners and
  connections come in as boxes of a family of their category, as fittings do, each saying so (a model group, class
  `ElementGroup`, and terrain, class `Toposolid` or `SiteSurface`, are left out instead: a group's members come in on their
  own, and terrain's box is a solid block over the site). Each component is of the Revit type its reference list names, as a door's
  is (`openings::symbol_of`; `Imported` where none is named), in a family of the Revit family's name (`openings::family_of`, the
  second list; else the family the type's other instances name; else the category's family). From 2025 a type has no record of its
  own, only a name entry, which is why both lists are read, from 2025 only (the provenance row says what was measured). Where the type's box, in the
  family's own axes, placed by the instance's placement (`placement.rs`, read from at most 4 KiB of its data, never copied:
  `Wants::placements`) gives the record's box to a millimetre, it comes in as that box, turned as placed, its middle set off from
  the family's origin by `Offset X` and `Offset Y`. The type's box is its symbol's (before 2025), else the instance's own record box
  taken back through a placement whose axes lie along the project's (exact), else the box its type's such instances agree on; a
  placement upside down (a light hung from a ceiling) or on its side (one on a wall) is re-expressed upright where one of its axes
  stands upright. Otherwise (no placement, one on a slope, no type, or a box that disagrees) it comes in as the record's box,
  unturned, and the note says which. A component comes in with **its shape** where the file holds it: its own graphics
  (`graphics.rs`, a tree read breadth first by the fields the file's own schema gives, found by its id) name the symbol it
  draws and the transform it is drawn by, and that symbol's solids (`shapes.rs`: faces on planes, cylinders and surfaces of
  revolution, and meshes) read whole, meet along every edge and, with the points and lines drawn beside them, fill the box the
  graphics give round them; the symbol's whole box, placed, must be the record's box to a millimetre. Its family then shows that shape for its type, a mesh form only that type shows (the
  type parameter `Shape`, 0 for the box), and each symbol a Revit type's instances draw is a type of its own (`Desk`, `Desk (2)`
  for one Revit made for an instance's own parameters; those that come in as boxes beside them a type of their own too), placed at
  the transform's origin and turned by it: in plan where its z is up, else tilted (`Component::tilt`, its family's x and z), and
  mirrored (`Component::mirrored`) where the transform's axes are a mirror's. An element whose graphics draw solids of their own
  where it stands (a stair's run, landing or support, a wall sweep, a slab edge) or several symbols (a railing's rails, a mullion)
  comes in with **a shape of its own**: those solids and each symbol placed by its transform, checked the same way, as a type of
  its own (elements drawn alike share one), and the note says "where it stands". So does one that places a single symbol whose
  box, placed, is not its record's, where all it draws is (a stair's run: its steps, and its walk line above them); a symbol placed
  that draws no solid adds nothing. A shape comes in **in its materials**: each face
  names the material it is drawn in (a mesh its own), and the family holds a mesh for each material its type's shape uses, one that
  names none in the family's own; and each material comes in with **its look**, its shading colour and transparency, read from its
  object by the schema's fields (`data::Look`, given to the model's material of its name once the import has made them), so walls'
  and floors' layers show their colours too. A symbol whose family **nests** others draws them in, each nested symbol's shape
  placed by its transform (`shapes::nest`, up to `MAX_NESTING` deep; one nesting itself, or one not read, leaves it a box), and a
  profile turned may be an ellipse. A shape that would take its family past the triangles one family may build
  (`family::MAX_BUILT`) or past `MAX_FORMS` goes in a family of the same name numbered on (`Railings (2)`, a name no other family
  has; `generic::room`, up to `MAX_OVERFLOW` of them), and a type keeps a box only where none has room. The graphics are found in the second pass and the
  symbols drawn read after it, each partition that holds one inflated again, and the symbols those nest in a round for each depth
  (`Gathered::drawn`, a `shapes::Drawing` for each element, and `Gathered::shapes`); the provenance rows say what was measured, and
  what is not read (solids standing outside both boxes Revit gives round a symbol, which its view conditions would explain, which
  come in as boxes saying why; solids outside the box round its solids alone are read where they fill the box round all it
  draws, `shapes::fills_either`; a component whose family would pass the types a family may hold is left out). Faces lie on
  planes, cylinders, cones, surfaces of revolution, ruled surfaces and bicubic Hermite surfaces, and profiles are lines, arcs,
  ellipses and cubic Hermite splines (`shapes.rs`). An edge's points are only a sampling of it: a face that is not cut into
  triangles as they give it (a thin rim between two arcs, whose chords cross) is cut again with its edges following the curves its
  curved neighbours put them on (`shapes::along`), and edges that meet only in space (at a pole) are joined there
  (`shapes::chain`). Snowdon 2025's architectural model brings in
  5,654 components, 4,614 with their type's shape and 795 with a shape of their own (every stair run among them), in 137 families
  (one of them the railings' numbered on) of 1,424 types.
  A box is a fair stand-in only for some (`generic::Shape`): a path's (a railing, a mullion, a sweep, a slab edge, a conduit),
  where its graphics draw no shape that reads, only where it runs along an axis, so one that turns a corner or runs across the
  axes, whose box is a block where there is none, is left out with that, as is one whose shape reads where no family of its name has
  room left for it (the note says which); and a whole (a stair,
  a truss, a beam system) is left out where its box holds a box of its parts' categories, which come in on their own. Conduits come
  in as boxes and not as runs: the finding that a duct's or a cable tray's data holds its line (below) was made on those two
  categories, and no conduit was looked at. A curtain wall (a wall of a type with no layers that a panel, a mullion or a grid line
  names as its host) comes in as a BIMCraft curtain wall of a pattern of neither panels nor mullions, 0 mm thick and set out by its
  exterior face, which draws nothing of its own: its panels and mullions come in as components with their shapes, so its facade is
  Revit's. A curved one, which a BIMCraft curtain wall cannot be, is left out, and a wall of a type with no layers that nothing
  names comes in as the plane of its box. A door or a window set in a wall comes in with **its shape** where its
  graphics draw one that reads (`generic::drawn_where_it_stands`, the components' reading): a family of the door or window
  category of its Revit family's name, its types Revit's (each shape and size a type, numbered on), each with `Width` and
  `Height` (the hole in the wall, from its box) and the shape in the opening's own axes as a mesh (`openings::OpeningFamilies`);
  one whose graphics draw nothing (a schematic opening cut) or do not read keeps a type of the project's own, the size of its
  box, and the note says why. The hole is the rectangle of its box: an arched one's arch is not read. A door or a window whose nearest wall is a curtain wall (a door panel of the grid) is set
  in no other wall: a BIMCraft curtain wall holds one only in a wall in its cell, so it comes in as a component of a generic family
  of its Revit family's name, with its shape (`openings::in_curtain`, `Import::in_curtain`, `generic::Scope::Only`). A curtain grid
  line (class `CurtainGridLine`) is left out, its box being no body.

  Every note of an element left out reads `N kinds left out, why (ids …)` (`Import::left_out`; an element the model's rules drop
  after it was made goes through `Import::dropped`), because the desktop adds those numbers up for its status line
  (`revit::count_left_out`); a note about something that came in reads `N kinds: why (ids …)`. A roof by extrusion (class `Roof`) is its
  sketch lines' chain as the profile of its top, with its type's thickness square to each slope, cut plumb, and it comes in only where
  the box that makes is the record's box to a millimetre (`roofs::extruded`): rvt-rs does not read it, so the provenance row says it is
  read from the sample's bytes. The desktop (`crates/ui-egui/src/revit.rs`) never depends on this crate: the window has a hook,
  `BimApp::revit: Option<Importer>`, which `apps/bimcraft`'s `main` fills when built with the `rvt` feature, so a test never reads a real
  file and a build without it has no `.rvt` option (`open_extensions`) and says why when given one. `open_revit` reads the file on a thread
  of its own behind `catch_unwind`; `poll_revit` takes the answer each frame (a refusal, a panic or a lost thread is a status message),
  `importing_box` holds the window still (keys stand down, `os_requests` queues the files that come meanwhile, the last wins, and Quit is
  still heard) and `install_revit` makes it a new unsaved project (`Session::reset(model, None)`, `reopened`; never in File > Open Recent)
  with a `Report` that File > Revit import report shows until another project replaces it (`reopened` drops it); its Copy Report button
  puts `Report::text`, the whole report in plain text with every note and its ids, on the clipboard, since the window's labels select
  one at a time (issue 45). A file named on the
  command line is `BimApp::open_later`: the window comes up first. Tests give the window a closure for a reader and block it at a gate
  (`tests/revit.rs`, with `step`, never `run`).
