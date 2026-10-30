# BIMCraft ideas

A living list of everything that could make BIMCraft better than Revit: features other BIM and CAD tools do well, gaps
Revit users have complained about for years, and things only BIMCraft can do because every action is a command and the
engine is open. It grows when someone finds a good idea and shrinks when one is built or turned down.

The [ROADMAP](ROADMAP.md) is the plan: what will be built and in what order. This file is the pool the roadmap draws
from. An idea here is not a promise, and an idea in the roadmap may also be here with more detail.

## How to use this list

- **Pick one up.** Choose an idea, open an issue (or a draft PR) that names its id, say how you'll build it as commands,
  and start with the failing test, as [CLAUDE.md](CLAUDE.md) asks. The rules still hold: everything is a command, it's
  validated in `Model::check`, the core stays UI-free, nothing panics.
- **When it's done**, delete its row in the same PR and add what it does to the [README](README.md). When only part of it
  is done, rewrite the row to say what's left.
- **When it's turned down**, move it to [Turned down](#turned-down) with one line on why, so nobody proposes it again
  without an answer.
- **Add ideas freely.** One row each, in the section where it fits, with the next free id. Say where it comes from (a
  product, a forum thread, your own practice) and why it matters.
- **Clean room.** "Seen in" names where an idea is known from, nothing more. Build it from the concept and public
  specifications only: no content, icons, help text, file formats without public specifications or trademarked names
  (a feature like Archicad's Pet Palette is described here by what it does; it's never called that in BIMCraft).

### Difficulty

| Level | Meaning | Rough effort with agents | Roadmap size |
|---|---|---|---|
| **1** | Trivial: one command or a UI touch, no new concepts | hours to a day | S |
| **2** | Easy: a small feature in existing modules | a few days | S |
| **3** | Medium: a new element kind, tool or drawing feature | one to two weeks | M |
| **4** | Hard: touches geometry, rendering or the file across many modules | about a month | L |
| **5** | Epic: a subsystem of its own | several months | XL |

Difficulty is the effort to build it well in BIMCraft as it stands today, with its tests, IFC and both themes, not the
effort in general. The **Value** column is a guess at how much users would notice it: ● low, ●● medium, ●●● high.

### Quick wins

High value for little work, a good place to start: VIEW-01, VIEW-03, VIEW-07, VIEW-12, UX-01, UX-03, UX-05, UX-09, DOC-05,
DOC-12, DATA-02, MOD-21, QA-01, AI-07.

---

## Contents

1. [Views and navigation](#1-views-and-navigation) (VIEW)
2. [Modeling](#2-modeling) (MOD)
3. [Curtain walls and facades](#3-curtain-walls-and-facades) (CW)
4. [Documentation and annotation](#4-documentation-and-annotation) (DOC)
5. [Data, properties and schedules](#5-data-properties-and-schedules) (DATA)
6. [Model quality and checking](#6-model-quality-and-checking) (QA)
7. [Analysis and sustainability](#7-analysis-and-sustainability) (AN)
8. [Visualisation and presentation](#8-visualisation-and-presentation) (VIS)
9. [Interface and productivity](#9-interface-and-productivity) (UX)
10. [Agents and AI](#10-agents-and-ai) (AI)
11. [Collaboration](#11-collaboration) (COL)
12. [Interoperability](#12-interoperability) (IO)
13. [Site and urban context](#13-site-and-urban-context) (SITE)
14. [Interiors and furnishing](#14-interiors-and-furnishing) (INT)
15. [Construction, fabrication and cost](#15-construction-fabrication-and-cost) (CON)
16. [Structure and MEP](#16-structure-and-mep) (DIS)
17. [Platform and engine](#17-platform-and-engine) (ENG)
18. [Local standards](#18-local-standards) (LOC)
19. [Turned down](#turned-down)
20. [Sources](#sources)

---

## 1. Views and navigation

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| VIEW-01 | **Background grid in plans and sections**: a construction grid (major and minor lines, spacing set per view or project), drawn behind the model, optionally snapped to | Archicad, Vectorworks, most CAD | 2 | ●●● | A layer under `render::draw`; spacing follows the view's scale so it never turns into a grey wash. Snap goes in `snap::snap` as one more kind with the lowest priority. Off on sheets and in print |
| VIEW-02 | **Rotated grid / working plane origin**: the grid and orthogonal snaps turn to a picked wall's angle, for buildings off the cardinal axes | Archicad (user origin), AutoCAD UCS | 2 | ●● | Adds an angle to `Snaps` and the grid; Revit's only answer is rotating the whole view to True North / Project North |
| VIEW-03 | **View history**: back and forward between the views and zooms you were in (Alt+Left/Right), like a browser | Browsers, Rhino named views | 1 | ●● | Window state only, a stack in `BimApp` |
| VIEW-05 | **Ghost of another level**: show the level below (or any level) faint under the current plan, picked from the view control bar, without changing the view's range | Archicad (Trace & Reference), Revit underlay (per view, clumsier) | 2 | ●●● | Draw the other level's `render::draw` output desaturated first. Make it a window setting, not a view property, so it can be toggled in a keystroke |
| VIEW-06 | **Reference any view on any view**: trace a section over a plan, or an older version of the plan over itself, moved and rotated freely, with differences highlighted | Archicad (Trace & Reference, Compare) | 3 | ●● | Builds on VIEW-05 and COL-03 (diff) |
| VIEW-07 | **Section box from selection**: select elements, one key, the 3D view crops to a box round them with handles on its six faces | Revit (Selection Box, BX), Navisworks | 2 | ●●● | Needs a crop box on `View3D` (geometry clipping in `view3d`) if there isn't one yet; grips as level extents do |
| VIEW-08 | **Cutaway 3D by any plane**: cut the 3D view by a section's plane, a level's cut plane, or several planes at once, with cut faces filled in poché | Archicad (3D Cutaway), SketchUp section planes | 3 | ●●● | Capping the cut needs closed solids, which walls, slabs and family forms already are |
| VIEW-09 | **3D documents**: a 3D view placed on a sheet with annotation (dimensions, labels) in the 3D view itself, so axonometric details print annotated | Archicad (3D Document) | 4 | ●● | Needs annotation in view coordinates of a projected view; a big win for construction details and assembly sequences |
| VIEW-10 | **Perspective camera, walk-through and fly mode** | Every BIM tool | 3 | ●●● | The perspective camera is in (`perspective`, a lens and an eye height, on the processor and the card); walk mode needs gravity onto floors and stair runs (`Model::floor_slabs`) |
| VIEW-11 | **Depth cueing** in elevations and sections: lines and fills fade with distance, in hidden line and shaded | Vectorworks 2026, Revit (elevations only) | 3 | ●● | Roadmap 0.7. `Drawing` items need their depth; in sections they already know their distance from the line |
| VIEW-12 | **Zoom to selection and zoom to element by id** from the status bar or the command palette | Revit (partially) | 1 | ●● | Pairs with Manage > Inquiry |
| VIEW-13 | **Linked views**: two views side by side whose selection, cursor crosshair and zoom centre follow each other (a plan and its 3D, a plan and a section) | Rhino viewports, Archicad (partially) | 3 | ●● | The selection is already the model's; the crosshair needs a point in model space from each view's `screen` |
| VIEW-14 | **Interactive section and elevation markers in 3D**: section lines and elevation markers drawn in the 3D view, draggable there | — | 3 | ● | |
| VIEW-15 | **Element info on hover**: a tooltip with category, type, id and a few chosen parameters after a short pause | Archicad (Info Tags), Vectorworks | 2 | ●● | Fields chosen in Settings; reads `schedule::field_text` |
| VIEW-16 | **Colour by parameter** in any view: a quick "colour by Fire Rating / Phase / Workset / any field" with an automatic legend, without making a filter for each value | Vectorworks (Data Visualization), Revit colour schemes (rooms only) | 3 | ●●● | A transient `Look` override per distinct value of a schedule field; the legend can later be placed on a sheet (DOC-09) |
| VIEW-17 | **Explode view**: a 3D view that pulls levels apart vertically (or layers of a compound wall outward) for presentation | Archicad (add-ons), presentation tools | 3 | ●● | Per-level z offset in the 3D scene only |
| VIEW-18 | **Model-space minimap / navigator**: a small overview of the whole level with the current viewport as a rectangle you can drag | Archicad (Navigator Preview), image editors | 2 | ● | |
| VIEW-19 | **Section perspective**: a section view drawn with a perspective camera behind the cut | Archicad, SketchUp | 4 | ●● | After VIEW-10 |
| VIEW-20 | **Plan regions and a sloped cut plane** for split-level buildings | Revit (plan regions), Archicad (partially) | 3 | ●● | Roadmap 0.7 has plan regions; a sloped cut plane is new |

## 2. Modeling

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| MOD-01 | **Priority-based junctions of compound layers**: each material has a strength (a priority), and where walls, floors, roofs and columns meet, stronger layers run through weaker ones automatically, in plan, section and 3D | Archicad (Building Materials intersection priority) | 5 | ●●● | The single feature Revit users most envy. Today walls mitre (`walls::cap`) as wholes; this needs layer-by-layer boolean cleanup across categories. Start with wall–wall in plan, then wall–floor in section |
| MOD-02 | **Complex profiles for walls, beams and columns**: a 2D profile drawn by the user (with layers and materials) extruded as the element, with stretch zones so it resizes | Archicad (Complex Profiles), Revit (sweeps, partially) | 4 | ●●● | A profile is a library entry; walls get a `profile` instead of a layer list. Covers parapets, cornices, retaining walls, plinths |
| MOD-03 | **Freeform solid element with direct editing**: push and pull faces, move edges and vertices, subdivide, with materials per face; for what no tool covers | Archicad (Morph), SketchUp, BricsCAD | 5 | ●●● | A `Mesh` element with a half-edge structure; IFC as a closed face set (already written for family forms). Pairs with MOD-04 |
| MOD-04 | **Push/pull on any element's face** in 3D: drag a wall's top to change its height, a slab's edge to move it, a roof's eave to change the overhang | SketchUp, BricsCAD BIM | 4 | ●●● | Map a picked face (`Image::face`) to the parameter that drives it; every drag runs an ordinary command |
| MOD-05 | **Shells**: curved surfaces by extrusion, revolution or ruled between two curves, with thickness, for vaults and organic roofs | Archicad (Shell tool) | 4 | ●● | |
| MOD-06 | **Solid operations between elements**: subtract, intersect, union kept live (a beam cutting a wall, a void cutting a slab), across categories | Archicad (Solid Element Operations), Revit (Join/Cut, roadmap 0.6) | 4 | ●●● | `csg::subtract` exists; this is a relation between elements, held by GUID, in `visit_refs!` |
| MOD-07 | **Opening element**: a box or extruded profile that cuts through any number of walls, slabs and roofs at once, with its own schedule and IFC `IfcOpeningElement` | Archicad (Opening tool), Revit (shafts, in part) | 3 | ●●● | Shafts already cut slabs; extend to walls and beams, any direction (MEP penetrations) |
| MOD-08 | **Stairs by rules**: draw a stair and it validates and fixes itself to a chosen code's riser/tread relation (2R+T), headroom and landing rules, with the failing step highlighted | Archicad (Stair rules and standards) | 3 | ●●● | `stairs::stair` plus a rule set per standard; see LOC for NBR 9050 / 9077 |
| MOD-09 | **Stairs by sketch**: boundary and riser lines drawn freely, winders and spiral stairs, with the walk line computed | Revit, Archicad | 4 | ●● | |
| MOD-10 | **Railing patterns**: posts, panels, balusters as a repeating pattern with rules at ends and corners, on stairs, slab edges and free paths | Archicad (Railing tool), Revit | 4 | ●● | |
| MOD-11 | **Door and window assemblies**: several doors, windows and fixed panels combined into one storefront-like unit with shared frame | Vectorworks 2026 | 3 | ●● | A group of openings in one wall hole; also a small alternative to a full curtain wall |
| MOD-12 | **Gravity**: place columns, furniture and fixtures onto the slab, roof or terrain under the cursor rather than at the level | Archicad (Gravity) | 2 | ●● | Asks `Model::floor_top` / roof planes at the point |
| MOD-13 | **Magic wand**: click inside any closed shape (lines, walls, a DXF underlay, a room) to make a floor, ceiling, roof, room or mass from it | Archicad (Magic Wand), Revit (Pick Walls, partially) | 3 | ●●● | Region finding already exists for rooms (`Model::bounding`); generalise to any drawn lines |
| MOD-14 | **Walls along any path**: a polyline or a chain of picked lines becomes joined walls in one go, with arcs | Archicad, Vectorworks | 2 | ●● | One `wall` per segment in a script, joined |
| MOD-15 | **Wall with a sloped or stepped top** without a roof to attach to: top profile by points along its length | Archicad (slanted/trapezoid walls), Revit (edit profile) | 3 | ●● | Roadmap 0.6 has wall profile editing; slanted walls (leaning in section) are a separate, harder idea |
| MOD-16 | **Slab drainage**: shape-editing points and lines on a floor or flat roof to make falls to drains, with layers that vary in thickness | Revit (Shape Editing), Archicad | 4 | ●●● | Roadmap lists it as not scheduled. A structural layer that stays flat and a screed that tapers is the common case |
| MOD-17 | **Propagate a junction fix**: fix how two elements meet in one place, and the tool finds every similar junction in the model and offers to apply the same fix | BricsCAD BIM (Propagate) | 4 | ●● | Needs a description of a junction (types, angle, layers) to match on |
| MOD-18 | **Auto-match data**: give one wall a type or parameter, and the tool suggests the same for every wall that looks the same (same thickness, same layers, same context) | BricsCAD BIM (Automatch) | 3 | ●● | Pure query over the model plus a preview of suggestions; agents can drive it too |
| MOD-19 | **Arrays that stay parametric**: linear, radial and along a path, the count and spacing editable afterwards | Revit, Archicad (Multiply) | 3 | ●●● | Roadmap 0.6 |
| MOD-20 | **Hotlinked modules / repeating units across files**: a unit (an apartment, a hotel room, a bathroom pod) kept in its own file and placed many times; editing the file updates every placement | Archicad (Hotlinks), Revit (groups and links, both limited) | 5 | ●●● | Built on links (roadmap 0.9) and GUIDs; each placement remaps GUIDs deterministically |
| MOD-21 | **Rotate 90° in one key** while placing or with a selection, plus flip and mirror shortcuts | Archicad 29, Revit (spacebar) | 1 | ●● | |
| MOD-22 | **Trim, extend, offset, fillet and split for every linear element** (walls, runs, lines, beams), with the same keys | AutoCAD, Revit (partially) | 3 | ●●● | Generic over `run_arm!` and walls |
| MOD-23 | **Roof wizard / roof by walls**: select walls, choose hip/gable/shed per edge, and the roof is made with the walls attached | Chief Architect (automatic roofs), Revit (by footprint) | 2 | ●● | `skeleton::faces` already handles gables; this is a tool over it plus wall attachment |
| MOD-24 | **Dormers**: a dormer object that cuts the roof and joins itself | Archicad (library), Chief Architect, Revit (manual) | 4 | ●● | |
| MOD-25 | **Parts**: split a compound wall or slab into its layers as separate elements (for construction sequencing), divided further by lines | Revit (Parts) | 4 | ●● | Each part is an element referencing its source by GUID |
| MOD-26 | **Stacked walls**: a wall type made of other wall types stacked by height | Revit | 3 | ●● | Roadmap: not scheduled yet |
| MOD-27 | **Sloped and curved ceilings, soffits, ceiling grids** with tile layout set from a start point and angle | Revit, Allplan 2025 (ceiling system) | 3 | ●● | |
| MOD-28 | **Moldings and sweeps along any edge**: a profile run along a slab edge, a roof edge, a wall's base or top | Revit (sweeps, slab edges, gutters, fascias) | 3 | ●● | Roadmap 0.6 has wall sweeps; generalise to any edge |
| MOD-29 | **Elements by face from imported geometry**: walls, floors and roofs from faces of a mesh or an IFC proxy, not only from masses | Revit (by face), BricsCAD (BIMify) | 3 | ●● | `massing` already does it for masses |
| MOD-30 | **Constraints the user can see**: locked dimensions, equalities and alignments shown as icons on the elements, a list of every constraint in the project, and a clear message when one blocks an edit | Revit (locks, EQ), Vectorworks | 4 | ●●● | Roadmap 0.6 has the solver; the visibility and the explanation are what Revit lacks |

## 3. Curtain walls and facades

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| CW-01 | **Curtain wall with grids, mullions and panels**: grid lines by spacing or count, mullion profiles, panel types (glass, solid, door), editable cell by cell | Revit, Archicad | 5 | ●●● | Roadmap 0.6 item. Each panel, mullion and grid line a part (`HasPart`) with id and GUID |
| CW-02 | **Pattern-based curtain wall**: a repeating 2D pattern (not just a rectangular grid: running bond, diagonals, staggered, Voronoi-like) of frames and panels, drawn once in a pattern editor and laid over the wall's surface | Archicad (Curtain Wall scheme), Revit (curtain panel by pattern, in families only) | 5 | ●●● | Builds on CW-01. The pattern is a library entry (cells with boundaries and panel types); laying it on a curved or sloped surface uses `view3d::basis`. Panels that fall off the edge are trimmed or replaced by an edge panel type |
| CW-03 | **Curtain systems on any face**: a facade pattern on a mass face, a roof, or a freeform surface, panels following the curvature (flat panels approximated) | Revit (curtain system by face), Archicad | 5 | ●●● | After CW-02 and MOD-03 |
| CW-04 | **Adaptive panels**: a panel type that changes with a parameter driven by position (open/closed by orientation, sun, a formula over x and z) | Revit (adaptive components with Dynamo), Grasshopper | 4 | ●● | Formulas already exist for families; feed them the panel's position and normal |
| CW-05 | **Facade panelisation report**: count distinct panel shapes, warn when two differ by under a tolerance, suggest rationalising to fewer unique panels | Rhino/Grasshopper workflows | 3 | ●● | Pure function over the panels; agents love it |
| CW-06 | **Brise-soleil / louver generator**: fins or louvers along a facade by spacing, angle and depth, optionally varying with orientation | Vectorworks, Grasshopper | 3 | ●● | Could be a CW pattern, or a run array |
| CW-07 | **Storefront from a sketch**: draw the grid lines on an elevation and the curtain wall makes itself | Archicad (curtain wall in elevation) | 3 | ●● | |
| CW-08 | **Spandrel and floor-edge detail**: curtain walls that know slab edges and add the spandrel panel and fire stop automatically | — | 3 | ● | |

## 4. Documentation and annotation

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| DOC-01 | **Automatic exterior dimensioning**: one click dimensions the whole outside of a building in chains (openings, walls, grid, overall) | Archicad (Automatic Dimensioning), Revit add-ins | 3 | ●●● | Needs associative dimensions first (DOC-02); the rules for which chains to make are a pure function |
| DOC-02 | **Associative dimension chains** that follow the elements they measure, with witness lines to faces, centres, layer boundaries and grids | Revit, Archicad | 4 | ●●● | Dimensions hold references (GUIDs + which face), like `Tag` holds its host |
| DOC-03 | **Auto-dimension of openings and rooms** inside a plan, one command per view | Revit add-ins | 2 | ●● | After DOC-02 |
| DOC-04 | **Rich text that stays formatted**: bold, italic, lists, sub- and superscript, tables inside text notes, never lost on edit | Revit (a long-standing complaint), Vectorworks | 3 | ●● | `font.rs` draws the lettering; spans with styles |
| DOC-05 | **Auto-text fields** in notes and title blocks: project name, sheet number, date, scale, element parameters, updated live | Archicad (Autotext), Revit (labels in title blocks) | 2 | ●●● | A note's text with `{field}` placeholders resolved at draw time |
| DOC-06 | **Data tags with any field, expressions and conditions**: a tag style that shows "W × H" or "Fire rating if any", from parameters, IFC properties or formulas, built in a tag editor | Vectorworks (Data Tags) | 3 | ●●● | Extends `Tag` beyond one `label`: a template of fields and literals, the formula engine of schedules |
| DOC-07 | **Tag everything visible in a view of a kind** with collision avoidance between tags and leaders | Revit (Tag All, no avoidance) | 4 | ●● | Label placement is a known optimisation problem; start greedy |
| DOC-08 | **Change markers / revision clouds from the diff**: compare the project with the issued version and draw clouds round what changed, automatically, listed in the revision schedule | Archicad (Change markers), none automatic | 4 | ●●● | Unique to BIMCraft's command log and GUIDs (COL-03). Roadmap 0.7 has manual revisions |
| DOC-09 | **Graphic legends generated from the model**: a legend of every door type, wall type, material or colour scheme used in the project, kept in sync | Vectorworks (Graphic Legends), Revit (legends are manual) | 3 | ●●● | A legend is a view whose content is computed, like a schedule |
| DOC-10 | **Schedules with pictures**: a door or window schedule showing each type's elevation drawing in a column | Archicad (Interactive Schedule previews), Revit (manual legends) | 3 | ●●● | The family geometry already draws; render a small elevation per type |
| DOC-11 | **Publisher sets**: named sets of sheets and views with an output format each (PDF, DXF, IFC, images), published in one go, again on change | Archicad (Publisher), Revit (print sets) | 2 | ●●● | A command `publish {set}`; useful from CI and agents |
| DOC-12 | **Sheet index generated** on the cover sheet, with revision columns | Revit (sheet list schedule) | 2 | ●● | A schedule over sheets |
| DOC-13 | **Master layouts**: title blocks, logos and stamps on a master that many sheets follow, with per-sheet overrides | Archicad (Master Layouts), Vectorworks | 3 | ●● | |
| DOC-14 | **View placement aids on sheets**: align views to each other by their grids, keep a plan in the same place on every sheet of a series, snap views to a sheet grid | Revit (Guide grids, ideas with ~900 votes) | 2 | ●●● | |
| DOC-15 | **Detail propagation**: a detail's annotation (notes, dimensions) copied to the other places that show the same junction | Revit (Copy to clipboard, manual) | 4 | ●● | Pairs with MOD-17 |
| DOC-16 | **Live 3D details**: a callout that cuts the 3D model at large scale and fills it with drafting (insulation hatch, sealant), so details come from the model, not from drafting over it | Archicad (Detail and Worksheet with model), Revit (callouts) | 4 | ●●● | Detail level fine already draws layers; this adds detail components on top |
| DOC-17 | **Pen sets / line weight tables per output**: one table for colour presentation, one for monochrome construction drawings, switched per view or per publish | Archicad (Pen Sets), Revit (object styles, single set) | 3 | ●● | A level of indirection between category styles and `Look` |
| DOC-18 | **Shadows in elevations and plans**: sun shadows cast in 2D drawings for presentation | Vectorworks, Archicad | 4 | ●● | Needs the sun (AN-02) and a shadow pass in `render` |
| DOC-19 | **Hatched cut fills by material with pattern alignment**: patterns aligned to the element (brick courses follow the wall), not to the sheet | Archicad (fill orientation) | 2 | ●● | `texture.rs` already lays model patterns by face axes |
| DOC-20 | **PDF with layers**: categories as optional content groups, so a consultant can switch them on and off in a PDF reader | Archicad, Vectorworks | 2 | ●● | `pdf.rs` writes the PDF; OCGs are in the PDF 1.5 specification |
| DOC-21 | **Keynotes from the model**: keynote by material, type or element, with a keynote legend per sheet listing only what's on it | Revit, Archicad 27 | 3 | ●● | Roadmap 0.7 |
| DOC-22 | **Spot slopes and spot elevations on 3D faces**: a spot on a roof or ramp shows its slope as %, ratio or degrees | Revit | 2 | ●● | |
| DOC-23 | **Area plans for local zoning**: gross, net, usable, saleable areas by local rules, with the polygons and calculation table printed as building departments ask | Revit (area schemes), Archicad (zones) | 3 | ●●● | See LOC-04 for Brazilian municipal tables |
| DOC-24 | **Drawing standards as data**: lettering heights, line weights, symbol shapes per standard (ABNT, ISO 128, AIA/NCS), switched for the whole project | — | 3 | ●● | |
| DOC-25 | **Markup import**: read comments and clouds from a reviewed PDF back into the views they were drawn on | Bluebeam integrations (Archicad 29) | 3 | ●● | PDF annotation dictionaries are public (ISO 32000) |

## 5. Data, properties and schedules

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| DATA-01 | **Expression-based properties**: a property computed from others by a formula (Fire rating from wall type and thickness, a room's occupancy from its area), visible everywhere a parameter is, not just in a schedule | Archicad (Expression-defined properties) | 3 | ●●● | Schedules' calculated fields already have the formula engine; promote them to project parameters with a formula |
| DATA-02 | **Edit in schedules both ways, with paste from a spreadsheet**: paste a column from Excel/LibreOffice into a schedule to set many values | Revit (one cell at a time), Archicad (Interactive Schedule) | 2 | ●●● | A paste is one `modify` per row in one undo step |
| DATA-03 | **Non-element data tables**: a table of the user's own (a door hardware catalogue, a finish code list) that parameters can look up and schedules can join | Revit Ideas (>1000 votes) | 3 | ●●● | A library entry of rows; a lookup function in formulas |
| DATA-04 | **Classification systems**: classify any element by Uniclass, OmniClass, Uniformat, NBR 15965 or a custom tree, written to IFC `IfcClassificationReference` | Archicad (Classification Manager), Revit (assembly code) | 3 | ●●● | Classification trees loaded from files the user supplies (or openly licensed ones) |
| DATA-05 | **Auto-classification**: suggest the IFC class and classification code of unclassified elements from their geometry and names | BricsCAD BIM (BIMIFY / classify) | 3 | ●● | Rules first, ML later; agents can do it today through MCP |
| DATA-06 | **Property manager**: one dialog that shows every parameter, where it applies, which elements hold values and which don't, with bulk rename, merge and delete | Archicad (Property Manager) | 2 | ●● | |
| DATA-07 | **Schedule split across sheets** with repeated headers, and placed as several pieces | Vectorworks 2026, Revit | 2 | ●● | |
| DATA-08 | **Schedule charts**: a bar or pie chart from a schedule's totals placed on a sheet (area by department, cost by category) | Vectorworks (Marionette charts) | 2 | ●● | |
| DATA-09 | **Worksheets**: a spreadsheet inside the project that can reference schedules, parameters and formulas, for calculations that don't belong to elements | Vectorworks (worksheets) | 4 | ●● | Might be DATA-03 plus formulas |
| DATA-10 | **bSDD lookup**: pick a property or classification from buildingSMART's Data Dictionary, with its definition and units | buildingSMART bSDD | 2 | ●● | Public API; an opt-in network call, so it goes through a crate like `crates/update`'s and never from the core |
| DATA-11 | **Room data sheets**: a printable page per room with its finishes, equipment, area and requirements, generated from the model | dRofus, healthcare workflows | 3 | ●● | |
| DATA-12 | **Program vs. model**: import a space program (rooms and target areas), and show live how the model meets it, room by room, with colours | Snaptrude (Program mode), Arcol, dRofus | 3 | ●●● | A table of targets plus VIEW-16's colouring |
| DATA-13 | **Parameter units and validation**: a parameter can say its unit, range and allowed values, and `Model::check` refuses what's out of range | Revit (partially) | 2 | ●● | |
| DATA-14 | **Global parameters driving geometry**: a project-wide value (floor-to-floor height, module size) that dimensions and levels follow | Revit (global parameters) | 3 | ●● | After the constraint solver |

## 6. Model quality and checking

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| QA-01 | **Warnings that say how to fix them**: every warning with a "show me" (zoom and select) and, where possible, a "fix it" button running a command | Revit (warnings are a famous pain) | 2 | ●●● | BIMCraft refuses invalid models outright; this is for soft issues (overlapping walls, rooms not enclosed, unplaced rooms, duplicate marks) as a list |
| QA-02 | **Model health dashboard**: element counts, unused types, warnings, file size, slowest views, unplaced views, in one page | Revit add-ins (Ideate, Model Checker) | 2 | ●● | A `health` command returning JSON; the desktop draws it |
| QA-03 | **Purge unused** with a preview, by kind, and "purge until nothing left" | Revit, Archicad | 1 | ●● | |
| QA-04 | **IDS checking**: validate the model against a buildingSMART IDS file (Information Delivery Specification) before export, and show which elements fail which requirement | buildingSMART IDS 1.0, Solibri, Bonsai | 3 | ●●● | IDS is an open XML standard; a pure function over the IFC-to-be |
| QA-05 | **Rule-based checks**: a rule set (minimum corridor width, door clearances, ceiling heights, stair headroom) run on demand or on save, with results as issues | Solibri | 4 | ●●● | Rules as data (JSON), so agents can write them. See LOC for national codes |
| QA-06 | **Clash detection** between categories, links and IFC references, grouped into issues, exported as BCF | Navisworks, Solibri, BricsCAD | 4 | ●●● | Roadmap 0.9. BVH picking from the GPU work gives the broad phase |
| QA-07 | **Accessibility check**: wheelchair turning circles in rooms, door clear widths, ramp slopes, grab bar presence in accessible WCs | — | 3 | ●●● | Turning circles drawn in plan as a check layer; see LOC-01 |
| QA-08 | **Egress analysis**: travel distances from every point of a floor to the nearest exit, drawn as a heat map, with paths | Revit (Path of Travel), Archicad add-ons | 4 | ●●● | Visibility graph over rooms and doors; a great showcase for a fast Rust core |
| QA-09 | **Duplicate and overlap finder**: two walls on top of each other, a door in a door, repeated components at one point | Revit warnings, BricsCAD | 2 | ●● | |
| QA-10 | **Model audit trail**: who changed which element when, from the command log, shown on the element's properties | Unique to BIMCraft | 3 | ●● | After COL-03 |

## 7. Analysis and sustainability

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| AN-01 | **Embodied carbon dashboard**: kg CO₂e by element, material and life-cycle stage from material quantities and EPD data the user imports, updating as you model | Vectorworks 2026 (Sustainability dashboard), One Click LCA, Revit (Tally, Insight) | 3 | ●●● | Material volumes are already computable per layer; carbon factors are a material property. No bundled database unless openly licensed (EC3, ÖKOBAUDAT have terms to check) |
| AN-02 | **Sun position and shadow study**: sun from location, date and time; shadows in 3D, plans and elevations; animated over a day | Every major tool | 3 | ●●● | Location in project settings; shadows in the GPU renderer or a shadow map in software |
| AN-03 | **Solar exposure map**: hours of direct sun or kWh/m² on facades and roofs, coloured in 3D | Ladybug, Archicad (Energy evaluation), Forma | 4 | ●● | Ray casting over the BVH |
| AN-04 | **Daylight factor / sDA on plans**: grid of points per room, colour map, percentage meeting a target | Climate Studio, Revit (Insight) | 4 | ●● | Simplified sky models are public; a full Radiance pipeline could be an external process |
| AN-05 | **Energy model export and run**: gbXML or OpenStudio/EnergyPlus model from rooms, walls and openings, run EnergyPlus (BSD-licensed) as an external tool, results back on rooms | Archicad (Energy Evaluation), Revit (Insight) | 4 | ●●● | Roadmap 0.9 has gbXML |
| AN-06 | **U-values from layers**: thermal transmittance of each compound type from material conductivities, shown in Edit Type and schedules | Archicad, Vectorworks | 2 | ●●● | Pure calculation; material gets a conductivity |
| AN-07 | **Condensation check (Glaser method)** for a compound wall or roof | German/Brazilian practice tools | 3 | ● | After AN-06 |
| AN-08 | **Live metrics bar**: gross area, FAR (floor area ratio), site coverage, unit count, façade-to-floor ratio, updating as you draw | Snaptrude, Arcol, TestFit | 2 | ●●● | Masses already report gross floor area; a small always-on panel |
| AN-09 | **Wind and view analysis**: what each window sees (sky, green, buildings), a view score per room | Forma, Ladybug | 4 | ● | |
| AN-10 | **Acoustic insulation estimate** of partitions from layers (mass law), and room reverberation time from finishes | — | 3 | ● | |
| AN-11 | **Water and rainwater sizing**: roof areas to gutter and downpipe sizes, cistern volume (NBR 10844, 15527) | — | 2 | ●● | Roof areas exist; ties into the Plumbing tab |

## 8. Visualisation and presentation

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| VIS-01 | **Real-time PBR rendering** with sun, sky, shadows, reflections and ambient occlusion, good enough for client meetings without leaving BIMCraft | Enscape, Twinmotion, Archicad/Vectorworks (Redshift), D5 | 5 | ●●● | The GPU renderer (wgpu) is the base; materials get roughness and metalness |
| VIS-02 | **Sketch / hand-drawn style**: wobbly lines, overshoots, pencil textures, watercolour fills for concept presentations | Archicad (Sketch rendering), Vectorworks, SketchUp | 3 | ●●● | A post-process on `Scene::edges` and on plan `Drawing`s; cheap and charming |
| VIS-03 | **Path-traced still renders** in the background, saved to a file | Archicad (Cineware), Vectorworks (Redshift) | 5 | ●● | Could use an existing permissively licensed Rust path tracer |
| VIS-04 | **Ambient occlusion and soft outline in shaded 3D** as a cheap visual upgrade | SketchUp, Archicad | 2 | ●● | Screen-space AO on the depth buffer the software renderer already has |
| VIS-05 | **Entourage**: people, trees and cars as flat or simple 3D placeholders, original or CC0 | Every presentation tool | 2 | ●● | Content work more than code |
| VIS-06 | **Animation**: a camera path through key views, exported as video or GIF | Revit (walkthrough), Lumion | 3 | ●● | |
| VIS-07 | **Construction sequence animation**: show the building grow phase by phase (or by 4D date), exported as video | Navisworks (TimeLiner), Synchro | 3 | ●● | Phases exist; this is a loop over them |
| VIS-08 | **VR/XR**: open the 3D view in a headset through OpenXR | Enscape, Twinmotion, Archicad (BIMx) | 4 | ●● | |
| VIS-09 | **Interactive web presentation**: export a self-contained HTML viewer of the 3D model with sheets linked to it (click a section marker on a plan, go to the section) | Archicad (BIMx hyper-model) | 4 | ●●● | A WASM build of the core's renderer (ENG-02) |
| VIS-10 | **AI render from a view**: send a viewport (and depth/normal passes) to a user-chosen image model to make a quick atmospheric render | Allplan 2025 (AI Visualizer), Archicad (AI Visualizer), Veras | 2 | ●● | Opt-in, through a network crate and the user's key; never from the core. Clean-room: no service is bundled |
| VIS-11 | **Material editor with preview sphere**, roughness, bump and transparency | Every tool | 3 | ●● | Roadmap 0.8 mentions roughness |
| VIS-12 | **Presentation plans**: soft shadows under walls, textured floors, furniture in colour, people in plan, at one click | Archicad, Vectorworks | 3 | ●●● | Mostly `Look` presets plus DOC-18 |

## 9. Interface and productivity

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| UX-01 | **Command palette**: Ctrl+K, fuzzy search over every command, tool, view, sheet, family and setting, with its shortcut shown | VS Code, Figma, Blender (F3) | 2 | ●●● | Everything is already a command in `ALL`; this is a search box over `catalog()` plus the browser tree |
| UX-02 | **Context palette at the cursor**: when you pick a point or an edge, a tiny palette of what you can do there appears next to the cursor (add a point, curve the edge, offset) | Archicad (Pet Palette), Vectorworks | 3 | ●●● | Built from `context_items`, filtered to what the picked thing allows |
| UX-03 | **Numeric input at the cursor**: start drawing and type length, angle or coordinates; the values show beside the cursor and Tab moves between them | Archicad (Tracker), SketchUp (VCB), AutoCAD (dynamic input) | 2 | ●●● | Revit's temporary dimension typing is clumsy; the typed value goes through `Units::parse` |
| UX-04 | **Parameter transfer**: pick up all settings of one element (eyedropper) and inject them into others (syringe), choosing which settings go | Archicad (Eyedropper/Syringe), Revit (Match Type, type only) | 2 | ●●● | One `modify` per target with the chosen fields |
| UX-05 | **Undo history panel**: the list of commands done, click one to go back to it, with each command's text | Photoshop, Blender | 2 | ●●● | Every step is already a command with its JSON |
| UX-06 | **Macro recorder**: record what you do as a script, edit it, run it again or bind it to a key | Office macros, AutoCAD action recorder | 2 | ●●● | Unique cheapness here: the session already logs commands; the script format exists |
| UX-07 | **Saved selection sets** and **selection by criteria** (all walls of this type on these levels with Fire Rating empty) | Archicad (Find & Select), Revit (filters) | 2 | ●●● | Queries on the roadmap's API track serve both |
| UX-08 | **Quick filter of the selection**: a breakdown by category in the status bar, click a category to keep only it | Revit (Filter), Vectorworks | 1 | ●● | |
| UX-09 | **Editable keyboard shortcuts and two-letter sequences, importable** from a file | Revit (KS), Archicad | 2 | ●● | Roadmap 1.0 |
| UX-10 | **Tabs and workspaces**: save the arrangement of palettes, views and the ribbon as a named workspace (modelling, documentation, review) | Archicad (Work Environment), Blender | 2 | ●● | `Docking` is already data in `Settings` |
| UX-11 | **Ribbon customisation and a quick-access toolbar** of the user's own | Revit, Office | 2 | ●● | |
| UX-12 | **Pen and touch input**: draw walls with a stylus, pinch to zoom on a tablet | Morpholio, Shapr3D | 3 | ●● | egui gets touch; tools need a "sketch" mode tolerant of shaky input |
| UX-13 | **Sketch recognition**: a freehand line becomes a wall, a circle a column, a rectangle a room | Shapr3D, Snaptrude (sketch to BIM) | 4 | ●● | After UX-12 |
| UX-14 | **Smart cursor with inference hints**: show why it snapped ("parallel to wall 100231", "on extension of grid B") | SketchUp, Vectorworks (Smart Cursor) | 2 | ●●● | `snap::snap` already knows; it should say so |
| UX-15 | **Instant start and fast open**: a small project ready in under a second, measured in CI | — | 3 | ●●● | A distinct advantage over Revit's start-up |
| UX-16 | **In-app learning**: guided tours of a tool, run against a sample project, that check each step was done | Archicad 29 (Learning Center) | 3 | ●● | Steps check the model through commands, so they can't be faked |
| UX-17 | **Element properties quick-edit**: double-click a parameter's value on screen (a wall height label) to change it | Archicad, Vectorworks | 2 | ●● | |
| UX-18 | **Multiple monitors**: views and palettes on another screen as real OS windows | Revit, Archicad | 3 | ●● | egui multi-viewport; views already float |
| UX-19 | **Dark mode everywhere**, including drawings (a dark drawing background) | Archicad, Rhino | 1 | ● | Mostly done for the window; the drawing background is a choice in Settings |
| UX-20 | **Recent files and a start page with thumbnails** | Revit, Archicad | 2 | ●● | Roadmap 0.5 |

## 10. Agents and AI

BIMCraft's distinguishing strength: every action is already a command an agent can run, with a params doc written for
agents. These ideas build on that.

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| AI-01 | **Assistant in the desktop**: a chat panel that drives the open project through the same commands, with every change in the undo history and a preview before applying | Archicad 29 (AI Assistant, beta), Revit add-ins | 3 | ●●● | Roadmap's "MCP attached to the running desktop" is the base; the user brings the model (local or an API key) |
| AI-02 | **Brief to model**: a written brief (rooms, areas, adjacencies, site) becomes design options as masses, then rooms and walls | Snaptrude, TestFit, Hypar | 4 | ●●● | Design options and masses exist; an agent prompt template (MCP prompt) and a scoring command |
| AI-03 | **Floor plan image or PDF to model**: a scanned plan becomes walls, doors, windows and rooms, with the scale calibrated from a known dimension | Various research tools, BricsCAD (BIMify for 2D) | 4 | ●●● | Vision model outside the core; the import is a script of commands, so it's reviewable |
| AI-04 | **Natural-language selection and queries**: "select all doors without a fire rating on levels 2 to 4" | — | 2 | ●●● | The agent turns it into a query command (UX-07) |
| AI-05 | **Auto-documentation**: an agent makes the sheets (plans, sections through the main spaces, elevations), places views, tags and dimensions them | BricsCAD BIM (automatic sections and sheets) | 3 | ●●● | All of it is commands; needs DOC-01 and DOC-07 to be good |
| AI-06 | **Model reviewer**: an agent walks the model against a checklist or a code (QA-05) and files issues with viewpoints (BCF) | Solibri, Archicad 29 Assistant | 3 | ●●● | |
| AI-07 | **Explain this element**: right-click, "why is this wall like this": its type, layers, joins, constraints, what references it, which command made it and when | — | 2 | ●● | Pure data from the model and the log; no AI needed for the facts, an agent can summarise |
| AI-08 | **Generative layouts**: lay out parking, apartments in a slab, desks in an office, toilets for an occupancy, by rules, and pick from options | TestFit, Revit (Generative Design) | 4 | ●●● | Deterministic solvers in Rust, with an agent to set the rules |
| AI-09 | **Learned shortcuts**: notice repeated sequences and offer to make them a macro (UX-06) | — | 3 | ● | |
| AI-10 | **Agent-written families**: describe a piece of furniture or a window, and the agent writes the family JSON (forms, parameters, types), previewed in the Family Editor | — | 2 | ●●● | Families are already text; works today through MCP, this makes it a button |
| AI-11 | **Local models only, if the user wants**: every AI feature works against a local model endpoint so nothing leaves the office | — | 2 | ●●● | A configuration choice, kept out of the core; important for firms with confidentiality clauses |
| AI-12 | **Code compliance Q&A**: "does this stair meet NBR 9077?" answered by checks the agent runs, citing the rule values the user supplied | — | 3 | ●● | No bundled copyrighted standards text; rules as user data |

## 11. Collaboration

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| COL-01 | **Element reservation teamwork**: a self-hostable server, users reserve the elements they edit, send and receive changes, as small deltas, never whole files | Archicad (Teamwork / BIMcloud delta server), Revit (worksharing) | 5 | ●●● | Roadmap 1.x. Command logs replayed and checked by `Model::check` server-side |
| COL-02 | **Real-time multiplayer**: see others' cursors and edits live, like Figma | Arcol, Snaptrude, Qonic, Motif | 5 | ●●● | After COL-01; ownership still governs edits |
| COL-03 | **History, diff and restore**: compare any two versions by GUID (added, removed, changed, with which parameters), shown in colour in every view | Archicad (Compare), Git | 3 | ●●● | Roadmap 1.x stage 2. Also the base of DOC-08 and QA-10 |
| COL-04 | **Branches**: try a redesign on a branch and merge it back, with conflicts shown per element | Git, Speckle (branches) | 4 | ●● | Design options cover part of it inside one model; branches cover the whole project |
| COL-05 | **Git-friendly files**: a project saved as a folder of small files (one per element kind, or per level), sorted and stable, so Git diffs and merges are readable | — | 3 | ●● | The file is JSON already; stable ordering and splitting is the work |
| COL-06 | **Comments pinned in the model**: a note on an element or a point in 3D, with a thread and a status, kept in the project and exported to BCF | BIM 360, Archicad (Issue Manager) | 3 | ●●● | Roadmap 0.9 has BCF |
| COL-07 | **Linked consultant models with change notifications**: a link reloads and says what changed (by GUID) since the last time | Revit (Coordination Monitor), Archicad | 3 | ●● | After links and COL-03 |
| COL-08 | **Speckle connector**: send and receive to Speckle streams | Speckle (open source, Apache-2.0) | 3 | ●● | Speckle's SDKs are open; a crate of its own, outside the core |
| COL-09 | **Read-only share link**: publish a view-only web model that a client can orbit and comment on | BIMx, Autodesk Viewer | 4 | ●● | After VIS-09 |

## 12. Interoperability

Several of these are on the roadmap (DXF, glTF, gbXML, BCF, point clouds, linked models); listed here only where there's
more to say.

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| IO-01 | **IFC as a first-class working format**: open an IFC, edit it, save it back with every GlobalId and property set of other tools kept as they were | Bonsai (BlenderBIM), Qonic | 4 | ●●● | Keep what isn't understood (unknown psets, relationships) alongside the model and write it back |
| IO-02 | **IFC4.3 export** for infrastructure-adjacent projects (alignment, site) | IFC 4.3 (ISO 16739-1:2024) | 3 | ●● | |
| IO-03 | **IFC references with live reload** in views, cut in sections, snapped to | Archicad, Revit (IFC link) | 3 | ●●● | |
| IO-04 | **PDF underlay with vector snapping**: a PDF page placed in a view at its scale, its lines snappable | Revit 2022+, Archicad | 3 | ●●● | Read the PDF's vector content (public spec); calibrate scale by two points |
| IO-05 | **Image underlay with scale calibration** (a scanned survey) | Every tool | 2 | ●● | |
| IO-06 | **Rhino 3DM import/export** via openNURBS (permissively licensed) | Archicad, Vectorworks, Revit | 4 | ●● | Check openNURBS licence fits MIT/Apache; meshes first |
| IO-07 | **USD export** for visualisation pipelines (Omniverse, Blender) | Revit, Archicad (via add-ons) | 3 | ●● | USD is open (TOST licence: check) |
| IO-08 | **OpenStreetMap context**: import surrounding buildings (as masses with heights), streets and terrain for a location | Archicad, Vectorworks, Forma | 3 | ●●● | ODbL data, attribution kept in the project; opt-in network call |
| IO-09 | **Excel/ODS round trip of parameters**: export a schedule, edit in a spreadsheet, import back by element GUID | Revit add-ins (Ideate BIMLink) | 2 | ●●● | Roadmap 0.9 has XLSX export; the round trip is the point |
| IO-10 | **SketchUp-style simple mesh import** (OBJ, STL, glTF) as a family or a mass | Every tool | 2 | ●● | Families already hold triangles |
| IO-11 | **COBie export** for facility handover | Revit (add-in), Archicad | 3 | ●● | COBie is a public IFC subset and spreadsheet format |
| IO-12 | **Visual programming**: a node editor over BIMCraft's commands and queries, for parametric design without code | Dynamo, Grasshopper (Archicad live connection), Marionette | 5 | ●●● | Each node is a command or query; a graph is a script with data flow. Possibly a separate crate on top of `Session` |
| IO-13 | **Grasshopper / Rhino.Inside-style live link**: an external parametric tool drives BIMCraft elements live | Archicad–Grasshopper, Rhino.Inside.Revit | 3 | ●● | MCP or a local socket already gives the transport |
| IO-14 | **DWG** | Every commercial tool | 5 | ●● | Read through acadrust (MPL-2.0, unmodified; `crates/dwg`); writing still open |

## 13. Site and urban context

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| SITE-01 | **Terrain from points, contours or a survey file**, with sub-regions (paving, grass) and building pads that cut it | Revit (toposolid), Archicad (Mesh), Vectorworks (Site Model) | 4 | ●●● | Roadmap 2.x Site |
| SITE-02 | **Cut and fill**: volumes between existing and proposed terrain, coloured in plan | Vectorworks, Civil 3D | 3 | ●● | After SITE-01 |
| SITE-03 | **Slope analysis map** with a legend of slope ranges | Vectorworks (site model analysis legends) | 2 | ●● | |
| SITE-04 | **Geolocation and true north**: project at real coordinates (EPSG code), survey point, written to IFC `IfcMapConversion` | Revit (shared coordinates), Archicad | 3 | ●●● | Revit's shared coordinates confuse everybody; make it one clear dialog |
| SITE-05 | **Hardscape and landscaping**: paths, walls, steps on terrain; trees and planting areas with species data | Vectorworks (Landmark) | 4 | ●● | |
| SITE-06 | **Property lines with setbacks** and a live zoning envelope (height limits, setbacks, sky exposure planes) as a mass | Revit (property lines), TestFit, Forma | 3 | ●●● | Masses exist; the envelope is a lofted mass computed from the lines and rules |
| SITE-07 | **Parking layout generator** on a site or a slab | TestFit, Vectorworks | 3 | ●● | |

## 14. Interiors and furnishing

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| INT-01 | **Room finishes by room**: floor, base, wall and ceiling finishes set per room, applied to the faces (Paint) and scheduled | Archicad (Zone-based finishes, add-ons), Vectorworks 2026 interiors | 3 | ●●● | Paint exists; this sets it from a room's parameters and keeps it in sync |
| INT-02 | **Wall finish layers per room side**: a finish (tile up to 1.2 m) that's a thin layer on one side of a wall, inside one room only | Vectorworks (wall finishes), Revit (parts or thin walls) | 3 | ●●● | Revit users model thin walls for this; a real feature beats it |
| INT-03 | **Casework and kitchen generator**: cabinets along a wall with counters, kickboards, and appliances, as one run, re-flowing when the wall changes | Archicad 29 (kitchen cabinets), Chief Architect, Vectorworks | 4 | ●●● | A run of families along a path |
| INT-04 | **Tile layout**: tiles on a floor or wall face with start point, joints and cut tiles counted | Vectorworks, interiors plug-ins | 3 | ●● | Texture lays patterns by face axes; the count needs the cut |
| INT-05 | **Furniture layout per room type**: one click lays out a bedroom, office or meeting room by rules | AI-08 | 3 | ●● | |
| INT-06 | **FF&E schedule with images and costs** | Vectorworks, interiors tools | 2 | ●● | DOC-10 |
| INT-07 | **Interior elevations of a room in one click**: four elevations of a selected room, cropped to it, placed on a sheet | Revit (manual), Archicad (Interior Elevation tool) | 2 | ●●● | |

## 15. Construction, fabrication and cost

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| CON-01 | **Quantity take-off and cost**: quantities by type, material and layer; unit costs from a price list the user imports (SINAPI in Brazil) giving a budget | BricsCAD (BOM Manager), Vectorworks, Allplan | 3 | ●●● | Schedules plus DATA-03 lookups; SINAPI is a public Caixa table |
| CON-02 | **4D: dates on elements** (or on phases), a timeline slider in the 3D view showing what's built when | Navisworks (TimeLiner), Synchro | 3 | ●● | Phases exist; this adds dates and a slider |
| CON-03 | **Timber framing generator**: studs, plates, headers and noggins from a wall's layer, with a cut list | Vectorworks, Chief Architect, Archicad add-ons | 4 | ●● | |
| CON-04 | **CLT / precast panelisation**: split walls and slabs into panels by maximum size, with connections, shop drawings and IFC assemblies | Allplan Precast, Tekla, hsbcad | 4 | ●● | Builds on MOD-25 Parts |
| CON-05 | **Assembly / shop drawings**: one element or a group gets its own sheet with views, dimensions and a parts list, automatically | Tekla, Revit (assemblies) | 4 | ●● | |
| CON-06 | **Masonry coursing**: walls dimensioned and checked against a block module (e.g. 39 × 19 cm blocks), openings snapped to coursing, block count | — | 3 | ●●● | Very common in Brazil (alvenaria estrutural); a module setting in snaps and a check |
| CON-07 | **Formwork and scaffolding quantities** from concrete elements | Allplan, Tekla | 3 | ● | |
| CON-08 | **Fabrication export**: BTL / BTLx for timber CNC, DSTV for steel | hsbcad, Tekla | 4 | ● | Public specifications |

## 16. Structure and MEP

The disciplines' own plan is in [docs/disciplines.md](docs/disciplines.md); ideas beyond it:

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| DIS-01 | **Analytical model** from the physical one (nodes, members, panels, supports), exported to an open format for analysis tools | Revit, Archicad (SAF) | 4 | ●● | SAF (Structural Analysis Format) is an open xlsx-based format |
| DIS-02 | **Rebar generated in concrete elements** by rules (cover, spacing, bar sizes) for a beam, a column, a footing and a slab; bends and hooks; bars drawn in plans, and seen through their host in 3D; a Structure tab tool | Revit, Allplan, Tekla | 4 | ●● | Bars, their bar schedule, IFC, the 3D view and sections exist (`rebar`), and an Eberick IFC comes in with its marks, its cover and its short beams; `reinforce` puts bars in a beam, a column, a footing or a floor from the cover and the counts, diameters and spacings the engineer gives. Several hosts at once, hooks on a beam's and a column's bars, bars in plans and presets kept in the project exist. Round columns and footings, sloped floors, hooks on a slab's bars and on ties exist. What is left: bends other than a hook, round beams, bars beyond a section's cut seen through the concrete, and any rule from a standard. Rules from NBR 6118 raise the question of data from a paid standard, as the sizing pull requests do |
| DIS-03 | **Fittings placed automatically** where runs meet (elbows, tees, reducers), from a routing preference per system | Revit (routing preferences), MEP Designer | 4 | ●●● | Fittings are families already; connectors at run ends are the new concept |
| DIS-04 | **Pipe and duct sizing** by flow (fixture units for drainage, NBR 8160; velocity for supply) | DDScad, Revit | 3 | ●● | |
| DIS-05 | **Circuits and load schedules**: devices assigned to circuits and panels, loads summed, panel schedule on a sheet | Revit, DDScad | 4 | ●●● | |
| DIS-06 | **Lightning protection by the rolling sphere method** (NBR 5419): the protected volume drawn in 3D, unprotected roof areas highlighted | — | 3 | ●● | Fits the SPDA tab; a distinctive feature |
| DIS-07 | **Ducts and HVAC**: rectangular and round ducts as runs, air terminals, equipment | Revit, MEP Designer | 3 | ●●● | A run category per the disciplines design |
| DIS-08 | **System browser**: every system with its runs and fixtures as a tree, flow direction checked, disconnected ends flagged | Revit | 3 | ●● | |
| DIS-09 | **Slopes kept on drainage runs**: drag a pipe's end and the slope stays at its set percentage | Revit | 2 | ●● | |

## 17. Platform and engine

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| ENG-01 | **Plug-ins in WASM**: third-party extensions run sandboxed, calling commands and queries only, so a plug-in can't crash the app or reach the disk or network without permission | Figma plug-ins, Zed extensions | 5 | ●●● | Fits "never crash" better than native plug-ins; commands are the API |
| ENG-02 | **BIMCraft in the browser**: the core compiled to WASM with a web front end; first a viewer, then editing | Arcol, Snaptrude, Qonic, Motif | 5 | ●●● | Core is UI-free already; file access and threads are the work |
| ENG-03 | **Python scripting** (PyO3) in the desktop with a console | Revit (pyRevit, Dynamo Python), Archicad (Python API) | 3 | ●●● | Roadmap API track |
| ENG-04 | **Lightweight embedded scripting** (Rhai or Lua) for user formulas, macros and rule checks | — | 3 | ●● | Pure Rust, sandboxable, no Python needed |
| ENG-05 | **Open file format specification**: a documented JSON schema of `.bimcraft`, so other tools can read it | — | 2 | ●●● | Strength over Revit's closed `.rvt`; generate from the types |
| ENG-06 | **Headless server mode**: render, check and publish projects as a service (CI for buildings: every push renders the sheets and checks the rules) | — | 3 | ●●● | CLI does most of it; a GitHub Action template would show it off |
| ENG-07 | **Huge models**: 500k elements editable, by incremental checks, spatial indexes and partial loading of levels | Revit (worksets), Archicad (modules) | 5 | ●●● | Roadmap 0.5 incremental validation is step one |
| ENG-08 | **Mobile viewer** for site visits (Android/iOS), with comments and photos pinned to the model | BIMx, Autodesk Docs | 4 | ●● | After ENG-02 |
| ENG-09 | **Deterministic builds of drawings**: the same project always prints byte-identical PDFs, so diffs of outputs mean real changes | — | 2 | ●● | Good for ENG-06 |
| ENG-10 | **Undo across sessions**: the undo history saved with the project | Blender (no), some editors | 3 | ● | The command log makes it possible |

## 18. Local standards

BIMCraft already speaks Portuguese and Spanish, and its discipline tabs follow Brazilian standards (NBR 5419, NBR 14565).
Doing local practice really well is something global vendors do poorly.

| Id | Idea | Seen in | Diff. | Value | Notes |
|---|---|---|---|---|---|
| LOC-01 | **NBR 9050 accessibility checks**: turning circles, door widths, WC layouts, ramp slopes and landings | — | 3 | ●●● | Rule values are user data or written from the public requirements in our own words, never the standard's text |
| LOC-02 | **NBR 9077 / fire code egress**: exit widths by occupancy, travel distances, stair counts | — | 4 | ●● | With QA-08 |
| LOC-03 | **NBR 15575 performance**: thermal (U-values, AN-06) and acoustic checks of envelopes | — | 3 | ●● | |
| LOC-04 | **Municipal area tables** (quadro de áreas): built area, permeable area, occupancy rate, FAR by the city's definitions, printed as the prefeitura asks | — | 3 | ●●● | Templates per city as data |
| LOC-05 | **ABNT drawing conventions** template: lettering, line weights, symbols, title block per NBR 6492 / 10068 | — | 2 | ●●● | Original title blocks drawn to the convention |
| LOC-06 | **SINAPI / TCPO price lookup** for budgets | — | 3 | ●●● | CON-01 |
| LOC-07 | **NBR 15965 classification** tree for elements | — | 2 | ●● | DATA-04 |
| LOC-08 | **Other countries' presets**: imperial units with US conventions, UK Uniclass, German DIN 277 areas | — | 2 each | ●● | Same mechanism as LOC-04 and LOC-05 |

---

## Turned down

Ideas decided against, with why, so they aren't proposed again without an answer to the objection.

| Idea | Why not |
|---|---|
| Bundling Revit families or templates, or an `.rfa` library converter for redistribution | Clean-room rule: a user's file may be read, never redistributed or turned into bundled content |
| A ViewCube copy | Autodesk's trademark and design; a navigation gizmo of our own is on the roadmap |

---

## Sources

Feature knowledge comes from public product pages, release notes, reviews and user forums, read for concepts only:

- Archicad 29 new features: [Graphisoft help](https://help.graphisoft.com/AC/29/INT/_AC29_Help/005_NewFeatures/005_NewFeatures-1.htm),
  [Architosh](https://architosh.com/2025/10/graphisoft-releases-archicad-29-with-game-changing-tools/)
- Vectorworks 2026: [Architosh](https://architosh.com/2025/09/vectorworks-2026-now-available-key-new-features/),
  [review](https://architosh.com/2025/12/product-review-vectorworks-architect-2026/),
  [Vectorworks newsroom](https://www.vectorworks.net/newsroom/architecture-features-architect-2026)
- Vectorworks data tags and graphic legends: [help](https://app-help.vectorworks.net/2025/eng/VW2025_Guide/Annotation2/Concept__Data_tags.htm),
  [newsroom](https://www.vectorworks.net/newsroom/3-graphic-legend-essentials-for-tidiness-style)
- Archicad compared with Revit: [Graphisoft community](https://community.graphisoft.com/t5/Modeling/Features-of-Archicad-not-in-Revit/td-p/289991),
  [renovation filter overrides](https://community.graphisoft.com/t5/Documentation/Renovation-filter-overrides/td-p/325848),
  [graphic overrides (Architosh)](https://architosh.com/2017/02/archicad-20-a-deep-dive-into-its-best-new-features/2/)
- Revit Ideas: [Ideate "wish list granted"](https://ideatesoftware.com/wish-list-granted-series),
  [Revit Ideas board](https://forums.autodesk.com/t5/revit-ideas/two-vote-levels-on-ideas-board-s/idi-p/11434277)
- BricsCAD BIM (Propagate, Automatch, classify): [reseller listing](https://novedge.com/products/buy-bricscad-bim-subscription),
  [PBC Today](https://www.pbctoday.co.uk/news/digital-construction-news/bim-software/101288/)
- Allplan 2025: [press release](https://www.mynewsdesk.com/nemetschek-ag/pressreleases/allplan-2025-revolutionizing-design-and-construction-with-ai-powered-tools-and-real-time-collaboration-3349314)
- Browser BIM (Snaptrude, Arcol, Qonic, Motif): [AEC Magazine](https://aecmag.com/bim/snaptrude-advances/),
  [ENR on Arcol](https://www.enr.com/articles/60723-arcol-brings-bim-to-a-browser-gives-design-concept-choice),
  [ArchDaily on Snaptrude](https://www.archdaily.com/1009121/snaptrude-the-browser-based-bim-tool-thats-changing-the-way-architects-work)
- Open standards: buildingSMART IFC, IDS, BCF and bSDD; ISO 32000 (PDF); ASPRS LAS; SAF.
