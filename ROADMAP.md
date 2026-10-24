# BIMCraft roadmap

BIMCraft is a clean-room building information modeler in pure Rust. It follows the workflow of the established BIM tools,
Revit among them, but it shares no content or text with any of them, and reads Revit's own files only under the fence in
principle 5. This roadmap is based on what the
code does today: what is missing, and the order that makes sense to build it in. What works now is in the
[README](README.md), and the rules every change follows are in [CLAUDE.md](CLAUDE.md). Ideas not scheduled yet, from
other BIM tools and users' wishes, are pooled in [IDEAS.md](IDEAS.md).

Sizes are rough effort for one developer working with agents: **S** a few days, **M** one to two weeks, **L** a month,
**XL** several months. There are no calendar dates. A release ships when its exit criteria are met.

---

## 1. Where it stands

The README lists what the engine, the desktop, the CLI and the MCP server do. What shapes the plan:

- **Done lately, so not repeated below:** families (revolves and voids, doors and windows as hosted families, the Family
  Editor), textures by material or from a picture, Paint, favorites kept for every project, and packages for Windows,
  Linux (AppImage, Flatpak, deb, rpm, Arch, tarball, riscv64) and macOS. Auto-update from GitHub releases is done for the Windows installer's copy, an AppImage and the macOS app, though none of it has run on a real system yet.
- **The file** is `.bimcraft` JSON with a `format` version (3 now): an older file is migrated on open, and a newer one is
  refused, saying so. A field this version doesn't know (a newer one's, in the same format) is left out and named, and the
  rest opens; commands still refuse a key they don't know.
- **Ids and GUIDs**, as Revit's ElementId and UniqueId: every element, every library entry (types, materials, families,
  parameters, filters, templates, phases, design options) and every part (layers, a family's forms and symbols) has a
  six-digit id and a GUID that never changes. What points at an element, an entry or a part holds its GUID in a file;
  an element's GUID is its IFC GlobalId. Insert from File brings elements of another project in, giving new ids where
  they're taken and new GUIDs to copies; Manage > Inquiry finds anything by id, GUID or GlobalId.
- **Validation and drawings** run over the whole model after every command. That is simple and safe, and nothing
  measures how it scales.
- **CI:** the release workflow runs the tests on Windows, Linux and macOS, but only for tags and manual runs. Pull
  requests aren't tested.
- **3D** is drawn in software and is orthographic: no perspective, shadows or sun.
- **Agents** reach the engine through the CLI and an MCP server over stdio. They can't attach to a running desktop.

---

## 2. How the order was chosen

1. **Foundations before breadth.** Stable ids, a versioned file format and performance headroom get more expensive with
   every feature built on top. Worksharing depends on all three, so they come first even though nobody sees them.
2. **A complete single-user architectural workflow before new disciplines.** Someone should be able to take a small
   building from concept to a printed set (curtain walls, detailing, area plans, revisions) before structure or MEP begins.
3. **Agents are first-class.** Every release adds to the API in step with the UI, because the command architecture makes
   that cheap and it is what sets BIMCraft apart.
4. **The rules stay.** Everything is a command, validation lives in `Model::check`, the core has no UI, tests come first
   and nothing panics. A feature lands as commands with tests, then as a tool.
5. **Clean room.** File formats come from public specifications (IFC, DXF, gbXML, BCF, glTF), and a format with no public
   specification (Revit's `.rvt` and `.rfa`) may be read only under the fence in [docs/rvt-provenance.md](docs/rvt-provenance.md).
   Content is original or openly licensed, a library's licence must suit MIT or Apache (the one exception is the
   Apache-2.0-only Revit reader, `crates/rvt`, which holds ported code), and Revit's name only describes the workflow.

---

## 3. Releases

### 0.5: Foundations

The goal is to make the file, the ids and the build trustworthy enough to build on for years.

| Item | Size | Notes |
|---|---|---|
| ~~File-format version and migrations~~ | M | Done: a `format` field, migrations on `open` (no format, 1 and 2 open as 3), a clear message for a newer file, and unknown fields left out of an open and listed (`left_out`, `lenient.rs`) while commands stay strict |
| ~~Globally unique ids (GUIDs) beside the display id~~ | M | Done: every element, library entry and part, with Insert from File, Select by ID and IDs of Selection (PRs #17, #19 and #21). Prerequisite for linking, merging and worksharing |
| ~~CI on every pull request: fmt, clippy and tests on Windows, Linux and macOS~~ | S | Done: the three checks on all three, an IfcOpenShell job (the sample and `ci/ifc-coverage.txt`, EXPRESS rules and every shape) and shellcheck and actionlint |
| ~~Autosave, crash recovery, recent files and a start page~~ | S to M | Done: background snapshots, adjacent project backups, temporary copies for untitled work, and Recover / Discard / Postpone on open ([design](docs/autosave.md)); File > Open Recent; a start page with New, Open, the recent files as tiles with pictures, and the update status |
| Benchmark project and performance baseline | M | Baseline in place: an original 20-storey, 9,521-element tower, release-mode core/UI probes and local private IFC measurements ([details](docs/performance.md)). Left: broader command coverage and stable-runner timing regression gates |
| Incremental validation and caching | L | Started: plan drawing and repeated-family geometry reuse, view-range filtering, vertical wall/host indexing, parallel scene construction/rasterization. Left: `check` only touched elements plus dependents, geometry by element revision, XY indexes for joins/shafts and picking/snapping |
| ~~Transactions: `script {atomic: true}` and grouped undo steps~~ | S | Done: `Session::transaction` runs commands all or nothing as one undo step (model, path, phase, option and history put back on failure); MCP's `script` takes `atomic` |

**Exit criteria**: green CI on three operating systems; a 0.4 file opens in 0.5 and is migrated; the benchmark model
edits at under 100 ms a command on a mid-range laptop.

### 0.6: Modeling productivity

The goal is the editing tools a working architect reaches for every hour.

| Item | Size |
|---|---|
| Arrays, linear and radial, optionally kept as a group | M |
| Groups (model groups) with group types, edit group, and exclude a member | L |
| Pin and unpin; a selection filter; match type properties | S |
| Locked and equal dimension constraints (a small constraint solver over references) | L |
| Copy to the clipboard, and paste aligned to the same place, to selected levels or to the current view | M |
| Join and cut geometry (wall to floor, column to wall cleanups in plan and section) | M |
| Wall openings (rectangular cuts) and wall profile editing (an elevation sketch) | M |
| Wall sweeps and reveals as profiles along a wall | M |
| Room separation lines; room bounding on and off per element | S |
| **Curtain walls**: grid lines, mullion types, panel types (glass, solid, door panel); curtain systems on faces later | XL |

Not scheduled yet: stacked walls as a type, in-place elements, and floor and roof shape editing by sub-element.

**Exit criteria**: a typical office facade (a curtain wall, groups of repeated bays, an array of columns) can be modeled
in the desktop and by an agent.

### 0.7: Documentation set

The goal is a printable, coordinated drawing set.

| Item | Size |
|---|---|
| Duplicate view (with or without detailing) and dependent views: done for plans (`duplicate_view`, DP); left: sections, elevations, 3D views, schedules and sheets | M |
| Detail lines, filled regions, masking regions, line styles: done (line patterns and styles, detail lines a line a click or as a polyline, the shared sketch solver, fill patterns, filled region types, filled and masking regions); left: the rows below | M |
| Line draw tools for detail lines, sketches and walls: done (rectangle, inscribed and circumscribed polygon, circle, start-end-radius, centre-ends, tangent end and fillet arcs, spline by control points and through points, ellipse and partial ellipse, polyline, Pick Lines, Sides, Radius and Offset; Join into Polyline and Explode Polyline); Fillet Arc and Pick Lines for walls too; left: Pick Faces | M |
| A line style for each of a region's boundary lines, or invisible: done | S |
| Split, Trim/Extend to Corner, Trim/Extend Single and Multiple Elements and Offset for detail lines and sketch lines: done (Split takes splines, a spline through points within 0.05 mm; grips on a detail line's handles; snaps to their ends, centres, quadrants, tangents and nearest points); left: Align for detail lines | M |
| Fill patterns: `.pat` import and export, an editor for patterns of many line families, and model patterns aligned and rotated on a face | M |
| Linework (a view's own line style on a model edge), repeating details and Detail Groups (insulation: done, along any curve) | M |
| Better than Revit: line patterns and fill patterns made from lines drawn on a plan (select them, then Create Line Pattern or Create Fill Pattern), as Archicad and pyRevit's pattern maker do | M |
| Drafting views, detail components (2D families), legends | L |
| Annotation types: text, dimension and tag styles; room tags with fields and tag families that look like Revit's (`tag` names any element with a field of its schedule, drawn as plain text, a door's in a box and a window's in a hexagon) | M |
| Area plans, area schemes (gross, rentable) and colour-fill legends by parameter | L |
| Plan regions (a local view range) and depth cueing in elevations | M |
| Revisions, revision clouds, and a revision schedule on title blocks | M |
| Editable title blocks (a 2D family with label fields), view titles under viewports, a sheet list | M |
| Viewports as windows onto their views (activate one to work through it, crop it, viewport types), and Object Styles' annotation categories, subcategories, colours and patterns. Paper space, Line Weights and Object Styles (each model category's pens, cut and seen) are in: every view has a scale (named in feet and inches as architects and engineers do), lines are pens 1 to 16 whose widths the project's Line Weights give by scale, and 0 a hairline at any scale | M |
| Print dialog: sheets to PDF or a printer, sizes, ranges, colour or monochrome | S |
| The rest of Revit's view control bar: Show/Hide Crop Region and Crop View buttons, Sun Path and Shadows, Temporary View Properties, a visual style for plans and sections and a detail level for the 3D view (scale, detail level, visual style, Temporary Hide/Isolate and Reveal Hidden Elements are done, and view templates control the first three) | M |
| Keynotes from a user-supplied keynote table (no bundled third-party data) | M |

**Exit criteria**: a small house's permit set (plans, reflected ceiling plans, sections, elevations, details, schedules,
an area plan and the revision history) prints from BIMCraft alone.

### 0.8: Families and content, the rest

Revolves, voids, doors and windows as families and a text-based Family Editor are done. What is left is the graphical half
and the content.

| Item | Size |
|---|---|
| Family editor sketch mode: reference planes, sketches drawn on screen, sweeps along a path and blends (revolve and void forms are done) | XL |
| 2D sketch constraint solver (dimensions bound to parameters, equal, aligned, locked) | L |
| Hosted families on floors, ceilings and faces, with cuts in the host (doors and windows host on walls today) | L |
| Nested and shared families; type catalogs (CSV) | M |
| Visibility per view direction and detail level for a family's forms and symbols | M |
| Sections that cut family forms (they show them beyond the cut today) | S |
| A starter content library: original, openly licensed (CC0 or CC-BY) doors, windows, furniture, casework, fixtures and profiles (32 families ship today: furniture, casework, doors, windows, and the Plumbing, Electrical, SPDA and Data tabs' fixtures) | L (ongoing) |
| A curated GitHub repository of openly licensed families, browsed, previewed and loaded from inside BIMCraft (search, categories, thumbnails, versions). Clean-room content only, checked before it's accepted; the download goes through the same rules as the updater (capped, digest-checked, never run) | M |
| Materials' surface and cut patterns as the project's fill patterns (the fill-pattern editor is done), and material roughness for the 3D view (textures are done) | M |

**Exit criteria**: a user draws a parametric window in the family editor, places it, flexes its types, and it schedules
and exports to IFC correctly.

### 0.9: Interoperability

The goal is that BIMCraft fits into a mixed-software office.

| Item | Size | Notes |
|---|---|---|
| DXF import (as a link or underlay in a view) and DXF export of views and sheets | L | Use the public DXF reference. Native DWG needs a licensed SDK or a GPL library, which conflicts with MIT or Apache, so DXF comes first and DWG is decided separately |
| Revit `.rvt` reader, in three parts, all done: the container, metadata and element inventory (`bimcraft-cli info`); the elements into the model (levels, walls, floors, ceilings, doors, windows, columns, rooms and roofs, through `run`, `convert` and the MCP `open`); File > Open in the desktop (a thread of its own, a report of what was left out) | L | Read only under the fence in [docs/rvt-provenance.md](docs/rvt-provenance.md); `crates/rvt`, Apache-2.0 only. IFC stays the way out. Left in the import: curtain walls, sloped floors and roofs by footprint, roofs by extrusion that are not a straight-line profile, a column's section shape, stairs and railings, groups, door swings, the join lists, and names for the categories `info` prints as bare ids (no table of Autodesk's `BuiltInCategory` values to check against a file was found). The desktop cannot cancel an import in flight |
| ~~Linked models: other `.bimcraft` and IFC files as references, positioned shared or origin to origin~~ | L | Done, origin to origin: `link` and `reload`, Manage > Link Model, a Linked Models group in the Project Browser. Shared coordinates, links in sections' cut poché by category, a link's own view overrides, hiding or unloading one in a view, and nested links are still to come (issue #36) |
| IFC export completeness: `Pset_*Common`, `Qto_*BaseQuantities`, space boundaries, classification references, an IFC4x3 option | L | Keep validating with IfcOpenShell |
| glTF 2.0 and OBJ export (for visualisation and the web) | M | |
| gbXML export (rooms and spaces, surfaces, openings) for energy tools | M | |
| BCF 3.0 import and export: issues with viewpoints, opened in views | M | |
| Schedules to XLSX; CSV import of parameters (schedules export to CSV today) | S | |
| Clash detection between categories (solid intersection), reported as BCF | M | High value once the geometry is there |
| Point clouds: PTS, XYZ, PLY and LAS import, then E57, shown in 3D, plans and sections, with snaps to their points | L | Public specifications only (ASPRS LAS 1.2 to 1.4, ASTM E2807). A cloud is project data placed by an element, capped and thinned on import, its origin moved so survey coordinates fit. LAZ needs a LASzip port and ReCap's RCP/RCS are closed, so neither comes in. IFC4 has no point cloud in common use, so export leaves them out and says so |

**Exit criteria**: a project round-trips through IFC with another tool with no loss of supported elements; a consultant's
DXF and IFC link in and coordinate.

### 1.0: Production-ready for one user

The goal is that someone could run a real small project on it.

| Item | Size |
|---|---|
| GPU renderer (wgpu) for 3D: shaded views draw on the card since 0.7 (opt-in), and surfaces seen through; still to come there are hidden line and wireframe, and then plans, perspective cameras, a section box, walkthroughs, shadows and sun, silhouettes | L |
| A navigation gizmo, steering and orbit around the selection; tiled windows (views already float in windows of their own, and palettes dock and float). Not Revit's ViewCube, which is Autodesk's trademark and design: an original one in the manner of Blender's open-source navigation gizmo (axis balls to click to look along an axis, drag to orbit), drawn in code | M |
| A dark theme with a white canvas: dark ribbon, palettes and browser around views drawn on white paper, as a choice beside the light and the dark theme (today the dark theme darkens the canvas too) | S |
| Editable keyboard shortcuts; project templates of BIMCraft's own on the start page | S |
| Accessibility: keyboard reach for every dialog and screen-reader labels (egui's AccessKit) | M |
| A file-format stability promise, with 1.x files opening in every later 1.x | S |
| Signed installers on all three operating systems (macOS signing is wired up and waits for a Developer ID) | M |
| A documentation site: user guide, command reference generated from `catalog()`, API guide | M |
| More interface and project languages, through community translation of `lang.rs` and `language.rs` | S each |

**Exit criteria**: performance budgets met on a 50k-element model; no data-loss bugs open; the release passes a scripted
end-to-end project (model, document, print, IFC) on all three operating systems.

---

## 4. API, MCP and automation track

This runs alongside the releases, roughly one item per release, because agent use is BIMCraft's distinguishing strength.

| Item | Size | Target |
|---|---|---|
| **JSON Schema for every command's params** (generated from typed param structs), exposed in `list_commands` and as MCP `inputSchema`s | L | 0.5 to 0.6 |
| Structured errors: a code, a field path and a suggestion, not only text | M | 0.6 |
| **MCP attached to the running desktop**: the desktop hosts the MCP server (a local socket or streamable HTTP), agents see the active view, the selection and the camera, and their commands appear in the user's undo history | L | 0.6 |
| MCP resources (the model summary, element JSON, rendered views by URI) and prompts (for example "model a house from this brief") | M | 0.6 |
| MCP streamable HTTP transport and multiple sessions | M | 0.7 |
| Richer queries: by parameter value, by bounding box or room, and relations (hosted in, bounded by, joined to); `elements` filters only by category and level today | M | 0.7 |
| Change notifications: which elements a command touched (also needed for incremental validation and worksharing) | M | 0.5 (engine) and 0.7 (MCP) |
| Python bindings (PyO3) of `Session`, for scripting and notebooks | M | 0.8 |
| A stable API version, a deprecation policy and semver on the command set | S | 1.0 |

---

## 5. After 1.0

### 1.x: Collaboration and worksharing

The command architecture is a strong basis: every change is already a JSON command that can be replayed. Build it in
stages, each useful on its own.

1. **Links** (done in 0.9): coordinate by reference, with no simultaneous editing.
2. **History and diff** (M): a project keeps its command log with authors and times; compare two versions by element
   GUID; restore a version. This is useful alone and is the foundation for the next step.
3. **Central model with element ownership** (XL): `bimcraft-server` (Rust, self-hostable) holds the central model.
   Clients keep a local copy, borrow the elements they edit (ownership per element, worksets as groupings for
   visibility and loading), and synchronise by sending command logs that the server replays and validates with
   `Model::check`. Conflicts are prevented by ownership rather than merged, which is the conventional and predictable
   BIM model.

Real-time co-editing and a browser viewer (a WASM build of the core) are possible after that, with ownership still
governing edits. The prerequisites are scheduled above: GUIDs, change notifications, the versioned format and
incremental validation (0.5), and links (0.9).

### 2.x: Disciplines

| Discipline | First slice | Size |
|---|---|---|
| **Structure** | First slice done: beams, structural columns, isolated and wall footings, structural floors, with types, schedules, IFC and the Structure tab ([design](docs/disciplines.md)). Left: braces (a sloped run, `IfcMember`), joins and framing types, slab foundations, beam systems, an analytical model exported to open formats | L |
| **Site** | Toposolids from points or contours (and imported DXF or LandXML), pads, property lines with bearings, cut and fill quantities | L |
| **Massing** | Extruded and lofted masses (a top outline, or a lean for a sloped one), mass floors you can run again, gross floor area, Show Mass, and walls, floors and roofs by face that follow the mass on Update to Face are in. Left: blends between outlines of different corner counts, revolved masses, curtain systems by face, and elements that follow the mass by themselves | M |
| **MEP** | First slice done: the Plumbing, Electrical, SPDA (lightning protection) and Data tabs, designed with Structure's in [docs/disciplines.md](docs/disciplines.md): pipes, conduits, cable trays, wires, conductors and cabling as runs in systems (`IfcSystem`), fixtures, fittings, boards, devices, terminals, outlets and racks as family categories with starter families of their own drawing, schedules, IFC out and back. Left: fittings and connectors (a pipe network that knows where it ends), auto-routing, sizing, circuits and panel schedules, a family kind for IFC predefined types (a basin from a WC), Visibility/Graphics by fixture category | XL+ |

Structure and site come before MEP: they are smaller, they reuse the existing solids and hosting, and architects need
them sooner.

---

## 6. Risks and decisions

- **Software 3D rendering** won't scale. A GPU renderer (wgpu) is the largest single technical bet. It started in 0.7 behind the
  existing `Scene` abstraction, as an opt-in path with the processor as its reference and its fallback; see [performance](docs/performance.md).
- **Whole-model rebuilds** after every command are simple and safe, but they set the ceiling on model size. The
  incremental work in 0.5 must keep the "validate in one place" rule: still one `check`, just scoped.
- **DWG.** No permissively licensed DWG library exists. Decide between DXF only, an optional commercial SDK plug-in, and
  a separately licensed GPL helper process.
- **The family editor's sketch mode** is as big as a small CAD program. Keep text, JSON and form-defined families working
  throughout, so agents and power users never wait on it.
- **egui** suits a thin UI, but dense dialogs (schedules, V/G, the family editor) push it. Keep the core UI-free so a
  second front end (web or WASM, say) stays possible.
