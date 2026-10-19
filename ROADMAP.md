# BIMCraft roadmap

BIMCraft is a clean-room building information modeler in pure Rust. It takes inspiration from the workflow of the
established BIM tools, Revit among them, but it shares no code, content, file format or text with any of them. This
roadmap is based on what the code does today (version 0.4.0, October 2026), not on the README's feature list. It sets
out what is missing and the order that makes sense to build it in.

Sizes are rough effort for one developer working with agents: **S** a few days, **M** one to two weeks, **L** a month,
**XL** several months. There are no calendar dates. Releases ship when their exit criteria are met.

---

## 1. Where it stands

### The engine (`bimcraft-core`, about 28k lines)

- **56 commands**, each a JSON call through `Session::execute`, with undo (200 steps, copy-on-write `Arc<Model>`),
  rollback on any failure or panic, and whole-model validation (`Model::check`) after every command.
- **25 element kinds**: level, grid, wall, floor, ceiling, shaft, door, window, room, column, stair, ramp, railing,
  roof, component (family instance), dimension, tag, text, spot elevation, section, elevation, 3D view, sheet,
  viewport, schedule.
- **Strong modeling core**: compound walls on six location lines, curved walls, mitred joins at any angle,
  attach top/base to roofs and floors; roofs by footprint (a straight skeleton with gables and courtyards), by
  extrusion, and dormers; sloped floors; ceilings; shafts; stairs and ramps with landings; railings hosted on
  stairs and ramps.
- **Data**: types per category, a material library (colour, cut and surface patterns), project parameters,
  parametric families with formulas (text-defined forms), schedules with fields, filters, sorting, grouping and
  calculated fields.
- **Views and graphics**: plans with view range and crop, reflected ceiling plans, sections, elevations, a software
  3D view (orthographic orbit), sheets A0–A4, Visibility/Graphics by category, filter and element, view filters,
  view templates, phases with phase filters, and design options.
- **I/O**: `.bimcraft` (JSON). IFC4 export, validated with IfcOpenShell. IFC2x3/IFC4 import (STEP parser written
  in-house) for most architectural elements. PNG/SVG plans, PDF sheets, CSV schedules.
- **Units and languages**: mm/cm/m/in/ft/ft-in; drawings and names in English, Portuguese and Spanish.

### The desktop (`bimcraft-ui-egui`, about 15k lines)

- Ribbon (Architecture, Annotate, View, Manage, Modify plus a contextual tab), Options Bar, Properties with the
  Type Selector, Project Browser, view tabs, light and dark themes.
- **About 40 tools**: wall, door, window, column, component, roof (footprint and extrusion), gable edge, dormer,
  floor, slope arrow, ceiling, shaft, stair, ramp, railing, room, level, grid, aligned, angular and radial
  dimensions, spot elevation, text, tag and tag all, measure, section, elevation, modify, move, copy, rotate,
  mirror (pick or draw), align, trim/extend to corner, split, offset, attach, flip, demolish, crop grips and wall
  grips.
- **Editors and dialogs**: Type Properties (with a layer editor), family Type Properties, Materials, Project
  Parameters, Schedule properties, Visibility/Graphics, Filters, View Templates, Phasing, Design Options, Snaps,
  Keyboard Shortcuts (a list).
- Snaps with overrides, typed lengths, temporary dimensions, window and crossing selection, Tab cycling, and
  careful click timing for fast input.

### Agents and automation (`bimcraft-cli`)

- CLI: `run` (with `--cmd`, `--script`, `--save`), `info`, `convert`, `commands`.
- MCP server over **stdio only**, with 10 tools: `execute`, `script`, `list_commands`, `inspect_model`,
  `query_elements`, `schedule`, `render` (a PNG back), `new_model`, `open` and `save`.

### Engineering

- About 460 test functions (core, UI through egui_kittest, CLI, MCP), a hostile-parameter sweep, and strict lints
  (no panics, no `unsafe`).
- CI is a **release workflow only**: a Windows MSI on tags. Pull requests aren't tested, and there are no Linux or
  macOS builds (issue #2). The installer isn't signed and there's no auto-update.

---

## 2. Gap analysis

| Area | Missing, roughly by importance |
|---|---|
| **Foundations** | No file-format version or migrations (types use `deny_unknown_fields`, so a newer file breaks an older build and vice versa); element ids are per-file counters (`next_id`), not globally unique, which blocks linking, merging and worksharing; no autosave or crash recovery; no recent files; `Model::check` and the drawings rebuild everything after every command, and no large-model benchmark exists; a script isn't one undo step |
| **Modeling** | Curtain walls (grids, mullions, panels); groups; arrays; pin; locked and equal constraints; copy to levels and paste aligned; join and cut geometry; wall openings, profile edits, sweeps and reveals; stacked walls as a type; in-place elements; room separation lines; floor and roof sub-element shape editing; railings beyond stairs and paths |
| **Doors and windows** | Fixed rectangles (`OpeningType` is width, height and sill). They aren't families: no frames, panels, swings by style, or nested parts |
| **Families** | Defined in JSON or a form; there's no graphical family editor, no reference planes or dimensional constraints (no sketch solver), no hosted (wall- or face-based) families, no nested families, no symbolic plan or detail-level representations, no type catalogs, and no content library |
| **Documentation** | No detail lines, filled regions, masking or detail components; no drafting views, legends, duplicate or dependent views; no per-view scale or detail level (coarse/medium/fine); no plan regions; no area plans, area schemes or colour-fill legends; no revisions or clouds; no keynotes; no editable title blocks or view titles; annotation has no types (text and dimension styles); tags cover doors and windows only |
| **Views and visualization** | No perspective cameras, section box or walkthroughs; 3D rendering is in software, so large models won't orbit smoothly; no textures, shadows or sun; no depth cueing; no tiled windows |
| **Interop** | No DWG/DXF; no glTF/OBJ; no gbXML; no BCF; IFC export lacks standard `Pset_*` and `Qto_*` quantities, space boundaries and IFC4x3; no linked models (IFC or `.bimcraft`) as references; no XLSX schedules |
| **API and MCP** | Command parameters are prose, not JSON Schema; MCP has tools only (no resources, prompts or notifications) over stdio only; one headless session; **agents can't attach to the running desktop** or see its selection or view; `elements` filters only by category and level; no atomic multi-command transaction; no plugin or scripting host |
| **Desktop UX** | No view cube or steering wheel; no temporary hide/isolate; no selection filter; no match properties; no clipboard (copy/paste between views or projects); shortcuts can't be edited; no print dialog; no recent files or start page; three languages only |
| **Collaboration** | Single user, single file. No linking, central model, element ownership, sync or history |
| **Disciplines** | No structure (beams, braces, foundations, analytical model, rebar), no MEP (systems, ducts, pipes, fittings, equipment), no site (toposolids, pads, property lines), no massing or conceptual design |
| **Distribution** | Windows MSI only, unsigned, no auto-update, no PR CI, no docs site |

---

## 3. How the order was chosen

1. **Foundations before breadth.** Stable ids, a versioned file format and performance headroom get more expensive
   with every feature built on top. Worksharing depends on all three, so they come first even though nobody sees them.
2. **A complete single-user architectural workflow before new disciplines.** Someone should be able to take a small
   building from concept to a printed set (curtain walls, real doors and windows, detailing, area plans, revisions)
   before structure or MEP begins.
3. **Agents are first-class.** Every release adds to the API in step with the UI, because the command architecture
   makes that cheap and it's what sets BIMCraft apart.
4. **The rules stay as they are**: everything is a command, validation lives in one place, the core has no UI, tests
   come first, and nothing panics. Every new feature lands as commands with tests, then a tool.
5. **Clean room.** File formats come only from public specifications (IFC, DXF, gbXML, BCF, glTF), content is
   original or openly licensed, and the library licences must suit MIT/Apache.

---

## 4. Releases

### 0.5: Foundations

The goal is to make the file, the ids and the build trustworthy enough to build on for years.

| Item | Size | Notes |
|---|---|---|
| File-format version and migrations | M | `format` field, a migration chain on `open`, and newer-file detection with a clear message. Forward compatibility: tolerate unknown fields where it's safe, and keep `deny_unknown_fields` only on params input |
| Globally unique element ids (GUID or UUIDv7) alongside the display id | M | Prerequisite for linking, merging, worksharing and stable IFC GlobalIds per element |
| CI on every PR: fmt, clippy and tests on Windows, Linux and macOS | S | Plus the IfcOpenShell validation as a job |
| Linux and macOS builds (AppImage/.deb, .dmg), issue #2 | M | Signing and notarisation come in 1.0 |
| Autosave, crash recovery, recent files and a start page | S–M | Recovery from an autosaved file after a panic is caught or the process is killed |
| Benchmark project and performance baseline | M | A generated 20-storey, about 10k-element model; time each command, `check`, plan render and 3D frame. Set budgets and fail CI on regressions |
| Incremental validation and caching | L | `check` only what a command touched plus its dependents (joins, hosts, rooms). Cache derived geometry (wall outlines, roof faces) by element revision. Add a spatial index for picking and snapping |
| Transactions: `script {atomic: true}` and grouped undo steps | S | One undo step, all or nothing |
| Own vocabulary | S | Describe features in BIMCraft's own terms in docs and MCP text (for example "a building modeler with a Revit-like workflow" rather than "Revit-class"), and keep familiar shortcuts as aliases. Low cost, and it keeps the clean-room position clear |

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
| Copy to clipboard, paste aligned to the same place, selected levels or the current view | M |
| Temporary hide and isolate in a view; reveal hidden elements | S |
| Join and cut geometry (wall–floor, column–wall cleanups in plan and section) | M |
| Wall openings (rectangular cuts), wall profile editing (elevation sketch) | M |
| Wall sweeps and reveals as profiles along a wall | M |
| Room separation lines; room bounding on and off per element | S |
| **Curtain walls**: grid lines, mullion types, panel types (glass, solid, door panel), curtain systems on faces later | XL |

**Exit criteria**: a typical office façade (curtain wall, groups of repeated bays, arrays of columns) can be modeled
in the desktop and by an agent.

### 0.7: Documentation set

The goal is a printable, coordinated drawing set.

| Item | Size |
|---|---|
| Per-view scale and detail level (coarse, medium, fine), with type layers shown at fine | M |
| Duplicate view (with or without detailing) and dependent views | M |
| Detail lines, filled regions, masking regions, line styles | M |
| Drafting views, detail components (2D families), legends | L |
| Annotation types: text styles, dimension styles, tag styles; tags for any category (rooms, walls, components) and room tags with fields | M |
| Area plans, area schemes (gross, rentable) and colour-fill legends by parameter | L |
| Plan regions (a local view range) and depth cueing in elevations | M |
| Revisions, revision clouds, and a revision schedule on title blocks | M |
| Editable title blocks (a 2D family with label fields), view titles under viewports, a sheet list | M |
| Print dialog: sheets to PDF or printer, sizes, ranges, colour or monochrome | S |
| Keynotes from a user-supplied keynote table (no bundled third-party data) | M |

**Exit criteria**: a small house's permit set (plans, RCPs, sections, elevations, details, schedules, area plan,
revision history) prints from BIMCraft alone.

### 0.8: Families and content

The goal is that users can make the parts a project needs without editing JSON.

| Item | Size |
|---|---|
| Family editor: its own window with reference planes, sketches, extrusion, sweep, revolve, blend and void forms | XL |
| 2D sketch constraint solver (dimensions bound to parameters, equal, aligned, locked) | L |
| Hosted families (wall-, floor-, ceiling- or face-based) with cuts in the host | L |
| **Doors and windows as families** (frames, leaves, glazing, swing symbols), migrating today's `OpeningType` | L |
| Nested and shared families; type catalogs (CSV) | M |
| Visibility per view direction and detail level; symbolic lines for plan | M |
| A starter content library: original, openly licensed (CC0 or CC-BY) doors, windows, furniture, casework, fixtures and profiles | L (ongoing) |
| Own fill patterns (an editor) and material appearances (textures, roughness) for the 3D view | M |

**Exit criteria**: a user builds a parametric window family in the editor, places it, flexes its types, and it
schedules and exports to IFC correctly.

### 0.9: Interoperability

The goal is that BIMCraft fits into a mixed-software office.

| Item | Size | Notes |
|---|---|---|
| DXF import (as a link or underlay in a view) and DXF export of views and sheets | L | Use the public DXF reference. Native DWG needs a licensed SDK or a GPL library, which conflicts with MIT/Apache, so DXF comes first and DWG is decided separately |
| Linked models: other `.bimcraft` and IFC files as references, positioned shared or origin-to-origin | L | Built on the 0.5 GUIDs; groundwork for worksharing |
| IFC export completeness: `Pset_*Common`, `Qto_*BaseQuantities`, space boundaries, classification references, IFC4x3 option | L | Keep validating with IfcOpenShell |
| glTF 2.0 and OBJ export (for visualisation and the web) | M | |
| gbXML export (rooms and spaces, surfaces, openings) for energy tools | M | |
| BCF 3.0 import and export: issues with viewpoints, opened in views | M | |
| Schedules to XLSX; CSV import of parameters | S | |
| Clash detection between categories (solid intersection), reported as BCF | M | Cheap once the geometry is there; high value |

**Exit criteria**: a project round-trips through IFC with another tool with no loss of supported elements; a
consultant's DXF and IFC link in and coordinate.

### 1.0: Production-ready for one user

The goal is that someone could run a real small project on it.

| Item | Size |
|---|---|
| GPU renderer (wgpu) for 3D, and later plans: perspective cameras, a section box, walkthroughs, shadows and sun, textures, silhouettes | XL |
| View cube, steering and orbit around the selection; tiled windows | M |
| Editable keyboard shortcuts; a start page with templates of BIMCraft's own | S |
| Accessibility: keyboard reach for every dialog and screen-reader labels (egui's AccessKit) | M |
| A file-format stability promise, with 1.x files opening in every later 1.x | S |
| Signed installers on all three operating systems; an auto-update channel | M |
| A documentation site (user guide, command reference generated from `catalog()`, API guide) | M |
| More interface and project languages, through community translation of `lang.rs` and `language.rs` | S each |

**Exit criteria**: performance budgets met on a 50k-element model; no data-loss bugs open; the release passes a
scripted end-to-end project (model, document, print, IFC) on all three operating systems.

---

## 5. API, MCP and automation track

This runs alongside the releases above, roughly one item per release, because agent use is BIMCraft's
distinguishing strength.

| Item | Size | Target |
|---|---|---|
| **JSON Schema for every command's params** (generated from typed param structs), exposed in `list_commands` and as MCP `inputSchema`s | L | 0.5–0.6 |
| Structured errors: code, field path and a suggestion, not only text | M | 0.6 |
| **MCP attached to the running desktop**: the desktop hosts the MCP server (a local socket or streamable HTTP); agents see the active view, the selection and the camera, and their commands appear in the user's undo history | L | 0.6 |
| MCP resources (the model summary, element JSON, rendered views by URI) and prompts (for example "model a house from this brief") | M | 0.6 |
| MCP streamable HTTP transport and multiple sessions | M | 0.7 |
| Richer queries: by parameter value, by bounding box or room, relations (hosted in, bounded by, joined to) | M | 0.7 |
| Change notifications: which elements a command touched (also needed for incremental validation and worksharing) | M | 0.5 (engine) and 0.7 (MCP) |
| Python bindings (PyO3) of `Session` for scripting and notebooks | M | 0.8 |
| Plugin host: WASM components calling commands, sandboxed and hostile-input-safe like everything else | L | 1.0 |
| A stable API version, deprecation policy and semver on the command set | S | 1.0 |

---

## 6. After 1.0

### 1.x: Collaboration and worksharing

The command architecture is a strong basis here: every change is already a JSON command that can be replayed. Build
it in stages, each useful on its own.

1. **Links** (done in 0.9): coordinate by reference, with no simultaneous editing.
2. **History and diff** (M): a project keeps its command log with authors and times; compare two versions by element
   GUID; restore a version. This is useful alone and is also the foundation for the steps below.
3. **Central model with element ownership** (XL): `bimcraft-server` (Rust, self-hostable) holds the central model.
   Clients keep a local copy, borrow the elements they edit (ownership per element, worksets as groupings for
   visibility and loading), and synchronise by sending command logs that the server replays and validates with
   `Model::check`. Conflicts are prevented by ownership rather than merged. This is the conventional and predictable
   BIM model.
4. **Real-time co-editing** (XL, later): live presence, cursors, and changes streamed as they're made. Ownership
   still governs edits; consider operational transforms over commands only if ownership proves too coarse.
5. **Cloud-hosted option, review and markup in a browser** (WASM build of the core plus a web viewer): later still,
   and optional.

The prerequisites, already scheduled above, are GUIDs (0.5), change notifications (0.5), the versioned format
(0.5), incremental validation (0.5) and links (0.9).

### 2.x: Disciplines

| Discipline | First slice | Size |
|---|---|---|
| **Structure** | Beams (with framing types and joins), braces, structural columns, isolated, strip and slab foundations, structural floors, beam systems; an analytical model exported to open formats | XL |
| **Site** | Toposolids from points or contours (and imported DXF or LandXML), pads, property lines with bearings, building pads, cut and fill quantities | L |
| **Massing** | Conceptual masses (extrusions, blends, lofts), floors from masses, gross areas, walls, curtain systems and roofs by face | L |
| **MEP** | Systems (supply, return, domestic water, sanitary), ducts, pipes, fittings, equipment and fixture connectors, auto-routing, sizing; electrical circuits and panel schedules later | XL+ |
| **Analysis** | Energy model export (gbXML, done in 0.9), daylight, quantity takeoff and cost (schedules and formulas already cover much of this) | M–L |

Structure and site come before MEP: they're smaller, they reuse the existing solids and hosting, and architects need
them sooner.

---

## 7. Risks and decisions to make

- **Software 3D rendering** won't scale. A GPU renderer (wgpu) is the largest single technical bet. Prototype it
  early (during 0.6) behind the existing `Scene` and `Drawing` abstractions, even though it ships in 1.0.
- **Whole-model rebuilds** after every command are simple and safe, but they set the ceiling on model size. The
  incremental work in 0.5 must keep the "validate in one place" rule: still one `check`, just scoped.
- **DWG.** No permissively licensed DWG library exists. Decide between DXF only, an optional commercial SDK plug-in,
  and a separately licensed GPL helper process.
- **Family editor scope.** It is as big as a small CAD program. Keep JSON and form-defined families working
  throughout, so agents and power users aren't blocked on the editor.
- **egui** suits a thin UI, but dense dialogs (schedules, V/G, the family editor) push it. Keep the core UI-free so
  a second front end (for example web or WASM) stays possible.
- **Trademark and clean-room hygiene.** Keep using public specifications and original content, keep Revit's name to
  nominative descriptions, and keep the shortcut aliases for familiarity.
