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
  of its `OST_SketchLines`; a roof by footprint only when flat, one by extrusion only where its box confirms it. A room tries the
  middle of its box and then a grid of points over it, and making the arrangement of a level's walls is what costs (`geom::faces`
  splits and matches every segment), so the faces of a level are made once and every point of every room of that level is asked of
  them (`geom::region_in`): the Snowdon sample's import went from 133 s to 2.4 s in a release build when the rooms stopped remaking
  their level's plan for each point. The notes are part of the product: a change that leaves something out says so.

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
