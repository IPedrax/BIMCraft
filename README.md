<p align="center">
  <img alt="BIMCraft" src="assets/app-icon/bimcraft.svg" width="128">
</p>

<h1 align="center">BIMCraft</h1>

<p align="center">
  <b>Building information modeling; an open-source, clean-room take on the Autodesk Revit workflow, rebuilt in pure Rust.</b>
</p>

<p align="center">
  Model a building the way Revit does: levels, walls, doors and windows, floors and ceilings, rooms, columns, stairs,
  ramps and railings, roofs and shafts that know about each other. Draw plans, sections, elevations and a 3D view from them, lay them out on sheets, and read and
  write IFC. Every action is a command, so agents can drive it as well as you can.<br>
  <i>By IPedrax.</i>
</p>

<p align="center">
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-Rust-167A63?style=flat-square&logo=rust&logoColor=white">
  <img alt="IFC2x3 and IFC4 import and export" src="https://img.shields.io/badge/IFC-import%20%C2%B7%20export-1F9D7F?style=flat-square">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-167A63?style=flat-square">
  <img alt="Agent-drivable over MCP" src="https://img.shields.io/badge/agents-MCP-1F9D7F?style=flat-square">
  <img alt="Status: early development" src="https://img.shields.io/badge/status-early%20development-f07a3a?style=flat-square">
</p>

<br>

<p align="center">
  <img src="docs/images/desktop.png" alt="The BIMCraft desktop app on the sample house: the ribbon's Architecture tab (Wall, Door, Window, Column, Component, Roof, Roof by Extrusion, Floor, Ceiling, Stair, Ramp, Railing, Room, Shaft, Dormer, Level, Grid), Properties with the plan's view range and the Project Browser, with its floor and ceiling plans, on the left, and the Level 1 plan with its furniture (a sofa, a dining table and chairs, a base cabinet, a bed, a wardrobe, a desk), dimensions, tags, a spot elevation, a note, the section, four elevation markers and the roof's edge dashed overhead" width="100%">
  <br><sub><b>The sample house</b> (<code>cargo run --release -p bimcraft -- --sample</code>): Revit's layout in BIMCraft's own icons and colours, with dimensions, door and window tags, a spot elevation, a note, a section and the four elevation marks, all built from BIMCraft commands.</sub>
</p>

<p align="center">
  <a href="#screenshots">Screenshots</a> ·
  <a href="#why-bimcraft">Why BIMCraft</a> ·
  <a href="#what-works-today">What works today</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#the-desktop-app">The desktop app</a> ·
  <a href="#drive-it-from-agents-mcp-and-the-cli">Agents, MCP and the CLI</a> ·
  <a href="#commands">Commands</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#roadmap">Roadmap</a> ·
  <a href="#downloads">Downloads</a> ·
  <a href="#license-and-credits">License and credits</a>
</p>

## Screenshots

<table>
  <tr>
    <td width="50%"><img src="docs/images/sample-3d.png" alt="The sample house in 3D from the south-east: its hipped roof over the walls, their brick outer layer textured with brick laid in courses, and the front door and two windows made of families, with boards, glass and a handle"></td>
    <td width="50%"><img src="docs/images/sample-section.png" alt="Section 1 of the sample house: the walls it cuts with their brick and block layers hatched, the slab and hipped roof in grey poché, the sofa and the partition beyond, the partition with its door, the levels with their heights, and grids 1 to 3"></td>
  </tr>
  <tr>
    <td><sub><b>3D view</b>: from the south-east, as Revit's default 3D view looks. Orbit, pan and zoom; click to select.</sub></td>
    <td><sub><b>Section</b>: the cut in poché, the roof along its slope, what lies beyond in hidden line, levels and grids.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/sample-sheet.png" alt="Sheet A102 of the sample house: Section 1 and the South and East elevations at 1 : 100, each titled, and the title box with the project, the sheet's name, its scale and its number"></td>
    <td width="50%"><img src="docs/images/desktop-dark.png" alt="The desktop app in dark mode: the View tab with Dark Mode and Crop View, the chrome dark, and the Level 1 plan drawn light on a dark ground, the walls' poché grey and the grids blue"></td>
  </tr>
  <tr>
    <td><sub><b>Sheets</b>: views placed at a scale under a title block, printed to PDF a page each.</sub></td>
    <td><sub><b>Dark mode</b>: the window and the views go dark; sheets stay white paper.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/schedule.png" alt="A schedule of the sample house's rooms in the desktop app: number, name, area, a Finish Height project parameter, an Accessible yes or no, and Paint, a calculated area, each room a row, the area and paint totalled under them; Properties shows the schedule's Fields, Filter and Sorting/Grouping, each with Edit"></td>
    <td width="50%"><img src="docs/images/schedule-properties.png" alt="Schedule Properties over the room schedule, on its Fields tab: the available fields, Add, Remove and Calculated Value, the scheduled fields in order with Move Up and Move Down, and the calculated Paint field picked, its heading, Calculate totals ticked and its formula, Perimeter times Finish Height over a million"></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/snap-guide.png" alt="The Wall tool in a plan: a first wall drawn level, a second running up and back from its end at a dashed angle guide, a graduated ruler along it reading 2530 mm and 60°, and a small arc at the corner showing the sharp angle between the two walls"></td>
    <td width="50%"><img src="docs/images/wall-grips.png" alt="A partition wall selected, its far end being dragged: a grip at each end, the wall as it would stand outlined in blue, a graduated ruler from the fixed end along a dashed guide at 75° reading 3520 mm and 75°"></td>
  </tr>
  <tr>
    <td><sub><b>Snaps and the ruler</b>: guides at angles from east and from the line before, so a 60° corner comes out exact.</sub></td>
    <td><sub><b>Wall grips</b>: drag an end, with the same snaps and guides from the other end.</sub></td>
  </tr>
  <tr>
    <td><sub><b>Schedules</b>: the fields chosen, project parameters and calculated ones among them, totalled.</sub></td>
    <td><sub><b>Schedule Properties</b>: fields, filters, sorting and grouping, as Revit's tabs have them.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/graphics-overrides.png" alt="The sample house's Level 1 plan with its own graphics: the exterior walls' cut filled pink and outlined dark red by a view filter, the doors' swings orange, the rooms in halftone, the furniture grey, and the grids and tags left out"></td>
    <td width="50%"><img src="docs/images/visibility-graphics.png" alt="Visibility/Graphics Overrides for Floor Plan: Level 1, on its Filters tab: the Exterior filter applied and visible, Add filter, Remove, Up and Down, and the filter's overrides below, projection and cut side by side, the cut's line colour dark red and its fill pink ticked"></td>
  </tr>
  <tr>
    <td><sub><b>Graphics overrides</b>: what a view hides and how it draws the rest, by category, filter and element.</sub></td>
    <td><sub><b>Visibility/Graphics</b>: categories, filters and elements, projection and cut each with their own line and fill.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/phases.png" alt="The sample house's Level 1 plan in a view whose phase filter shows what stood before in halftone and what comes down dashed red: the south, west and partition walls grey, the north wall, new in this phase, in black poché, and the east wall, taken down in it, outlined in dashed red; the status bar offers an Active design option, Main Model"></td>
    <td width="50%"><img src="docs/images/phasing.png" alt="The Phasing dialog on its Phase Filters tab: the Review filter picked, with Delete, a field and New Filter for another, and a drop-down for each status, New and Temporary by category, Existing and Demolished overridden"></td>
  </tr>
  <tr>
    <td><sub><b>Phases</b>: what stood before in halftone, what comes down dashed red, what is new as the category draws it.</sub></td>
    <td><sub><b>Phasing</b>: phases in order, and phase filters that say how a view shows each status.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/textures.png" alt="The sample house's south wall close up in the 3D view: the brick laid in courses at its real size, the front door of a family with its timber boards, frame and handle, and beside it a casement window of a family, its two panes of glass in their sashes"></td>
    <td width="50%"><img src="docs/images/paint.png" alt="The Paint tool in the 3D view: the Modify | Paint tab, Plaster picked in the Options Bar, and the south wall's outside painted with it, grainy, round the door and the window, which keep their own materials"></td>
  </tr>
  <tr>
    <td><sub><b>Textures</b>: a material's texture at its real size on the faces it lies on: brick in courses, boards, glass.</sub></td>
    <td><sub><b>Paint</b>: a material laid over one face, as Revit's Paint does; Remove Paint takes it off.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/family-editor.png" alt="The Family Editor on the Casement Window family: its name and category, the Parameters tab with Width, Height, Sill Height, Frame Width, Frame Depth and a Frame Material, each with its kind, value and formula, a preview of the window in a wall, and Load Family, Save Family, Add to Favorites, Apply, Cancel and OK below"></td>
    <td width="50%"><img src="docs/images/materials.png" alt="The Materials window on Brick: its name, colour, cut and surface pattern, a Texture drop-down with the tile's width and height, angle and offsets, a swatch of the texture, and the Save to Favorites and Favorites buttons"></td>
  </tr>
  <tr>
    <td><sub><b>Family Editor</b>: parameters, types, forms (extrusions, revolves, voids) and a door's or window's plan symbols, with a preview.</sub></td>
    <td><sub><b>Materials</b>: a texture from a pattern or a picture of yours, its tile size, and a favorite kept for every project.</sub></td>
  </tr>
</table>

## Why BIMCraft

- **The workflow you know.** Revit's ribbon, the green contextual Modify tab, the Options Bar, Properties with the Type
  Selector and Edit Type, the Project Browser, views in tabs, and Revit's shortcuts: WA, DR, WN, RM, CL, DI, TG, MV, CO,
  RO, MM, AL, TR, SL, ME, CS, SA, VV, EH, and KS to list them all. Click to select, Ctrl adds, Shift takes away, Tab reaches what's
  underneath.
- **A real building model.** Doors and windows live in their walls, rooms find their boundaries from the walls around
  them, walls joined at a corner stay joined when one moves, dimensions and tags follow what they measure and name, and
  every change is checked against the model's rules and can be undone.
- **Open files.** IFC2x3 and IFC4 come in from Revit and other BIM tools, and IFC4 goes back out, checked with
  IfcOpenShell. Sheets print to PDF; any view saves as PNG, plans as SVG. Revit's own `.rvt` format is closed, so IFC is
  the way in and out.
- **Built for agents.** Every tool, button and field is a command. Agents call any of them with JSON, inspect the model to
  check their work, and render any view to look at it, over MCP or the CLI.
- **Fast and native.** Pure Rust and egui: no Electron, no web view. Packaged for Windows, Linux and macOS; developed on
  Linux, and the macOS app isn't tried by hand yet.
- **Free.** MIT OR Apache-2.0, with no account and no subscription.

## What works today

BIMCraft is in early, fast development. Honest status:

| Area | Works today |
|---|---|
| Datums | Levels with elevations; grids that name themselves 1, 2, 3… or A, B, C… |
| Walls | Chains of walls, straight or curved (each turning `arc` degrees), on any of Revit's six location lines (the wall's or its core's centre line, a finish or a core face, exterior or interior); compound types with layers (structure, substrate, thermal, membrane, finish) from the exterior in, the exterior being to the left of start to end, so walls drawn clockwise face out; joins at any angle, mitred, three or more walls sharing a node, a wall ending on another stopped at its face, and sharp corners squared off; base level and offset; the top at a level or an unconnected height (by default, up to the next level), or attached to the underside of a roof or a floor (a base, onto one), following it where it's over the wall; flip, as Revit's arrows do |
| Doors, windows | Hosted in walls: place one at a point on the wall (`at`), or by wall and offset; marks number themselves; flip facing and hand; refused when they don't fit their wall or overlap. Of one of the project's own types (a width, a height, a sill), or of a family of their own: a new project comes with a single door, a double door, a fixed window and a casement window, and more load from `.json` files. A family's forms (frame, leaf, handle, sashes, panes) are drawn in the opening, its `Width` and `Height` size the hole in the wall, its `Sill Height` is a window's sill, its formulas read `Wall Thickness`, and its plan symbols (a door's swing) go with it as it flips; sections show them beyond their line, never cut |
| Floors | A boundary you give, open through any openings inside it, or the outline of the walls on the level, open over each loop of them standing free inside, a courtyard (a loop joined to the outer walls, or with a room in it, is rooms, and floored over); the top at the level or a height offset from it, or sloped along a slope arrow (from its tail, where it's at the offset, rising so many degrees toward its head, as Revit's slope arrow does), as thick as its type across the slope; walls attached to one follow it; in plan, its openings, the slope arrow and its slope |
| Ceilings | In the room around a point, out to its walls' faces (Revit's automatic ceiling) and open round any walls standing free inside it (a courtyard's loop, a core), or along a boundary, open through any openings inside it; their underside 2600 mm up by default, their type's thickness over it; types with square tiles, which the reflected ceiling plan draws, stopping at the openings |
| Shaft openings | A hole along an outline from a level (and an offset) up to the next level, a level or a height, cut through every floor, ceiling and roof it reaches into: in 3D, sections, areas and IFC; in plan, its outline and a cross where the plan cuts it, and the hole in a floor seen below |
| Columns, stairs, roofs | Columns of rectangular or round types, turned to any angle, from their level up to the next (or a height, or a level); cut in plan, in poché. Stairs from a start point toward an end point, in as many risers as keep each at most 180 mm, treads 280 mm deep, up to the next level: one straight run, or runs with a landing between each two (as deep as the stair is wide) after so many risers, the next run going on straight, turning left or right, or coming back beside the last; in plan, their risers up to the cut line and the rest dashed, landings over the cut plane dashed, and the walk line marked UP (DN from the level above). Roofs on a level and an offset from it, by footprint: flat, or over any outline every edge sloping up at one slope until the slopes meet, in hips, valleys and ridges (its straight skeleton), but the edges left upright as gable ends; round courtyards too, holes in the footprint whose edges are eaves like the outline's (or gables); by default over the walls of the level below, 500 mm past their faces, at 30°, and open over each loop of them standing free inside, a courtyard (a loop joined to the outer walls, or with a room in it, is rooms, and roofed over). Or by extrusion: a profile drawn along a line, swept across. Openings through them, and a roof that runs into another (a dormer's) ends where it meets that one's top. Dormers: walls standing on a roof's slope under a gable roof of their own, with a window in front, the roof open under them. In plan, their ridges, hips and valleys, and from the level below their edge dashed overhead |
| Rooms | Placed at a point, bounded by the walls around it, and following them when they change; free-standing islands (a shaft) are subtracted; area and perimeter at the wall centre line |
| Ramps, railings | Ramps from a start point toward an end point at a slope (8.33%, 1 in 12, by default) up to a level or a height, in one run or turning at landings as stairs do, solid down to their level; in plan, their outline, dashed over the cut plane, and the walk line. Railings down a side of a stair or a ramp, 50 mm in from its edge, climbing with it and going wherever it goes, or along a path on a level; a rail at their type's height over what they run along, and balusters their type's spacing apart; in plan, their line, dashed over the cut plane |
| Types | Wall, floor, ceiling, door, window, column, roof and railing types; editing one changes every element that uses it |
| Materials | A library of materials in each project, as Revit's Material Browser: each with a shading colour, a cut pattern and a surface pattern (solid, diagonal, crosshatch, horizontal, vertical, grid, brick, concrete or insulation), drawn in code. Wall, floor, roof and ceiling types are built of layers, each a function, a material from the library and a thickness of its own, adding up to the type's: a wall's from the exterior in, the others' from the top down. Plans and sections cut each layer and hatch it with its material's cut pattern where it's wide enough to hold it (a drafting pattern, the same on screen and on paper); a floor's top in plan and a ceiling's underside in its ceiling plan show their surface pattern at its size in the building; the 3D view shades each layer in its material's colour. A material may have a texture over its colour in the 3D view: a pattern drawn in code (brick, block, boards, concrete, plaster, tile, roof tiles, fabric, metal or insulation, each at its real size) or a picture you add (a PNG or a JPEG, kept in the project, at most 1024 px a side, up to 24 of them), laid on every face of what's made of the material at the size you give it, turned and moved as you say, by the face's own axes (across a wall, a slab's east and north); the library's materials come with theirs. Paint (PT) lays a material, with its texture, over one face of an element, a wall's exterior or a floor's top, over what its type is made of, as Revit's Paint does. A material, with its picture, can be kept in your Favorites, a file of your own (`favorites.json` in your configuration folder, or where `BIMCRAFT_FAVORITES` says) that every project reads; textures show in the 3D view alone (plans and sections keep their patterns), and IFC carries the colour |
| Editing | Modify any field, of one element or many in one step (a wall put on another location line stays where it is); move, copy, rotate and mirror, with the walls joined to them stretching to stay joined and doors and windows going with their walls (a mirrored wall runs back the other way, its exterior mirrored); trim or extend two walls to a corner; split a wall, an arc into two arcs; flip walls; delete (a wall takes its doors and windows with it, and they their tags; a roof or floor walls are attached to lets them go); undo and redo |
| Snaps and the ruler | Points picked while drawing stick to what the plan has there, as Revit's snaps do: the ends, middles and centres of walls (an arc's too), grids, columns and components, and the corners of floors, ceilings, roofs and shafts; where two straight walls or grids cross; the foot of the perpendicular from the last point to a straight wall or grid; and a wall's centre line, so a wall drawn onto it meets it. Each shows its own marker and its name. A direction from the last point sticks to multiples of an angle (5°, 15°, 30°, 45° or 90°; 15° by default), from east and from the line before it, so corners come out exact, sharp ones too: a dashed guide shows it, and where it crosses a wall or grid the point lands there. The line being drawn carries a ruler: graduations at round lengths, its length and its angle (at the corner it turns once there's a line before it, drawn as an arc). Manage > Snaps switches each snap and sets the angle steps, and Revit's SE, SM, SI, SC, SP and SN take one snap alone for the next point (SO none, SS back); Measure (ME) lays a ruler between two snapped points and reads its length, parts and angle; the `measure` command does the same |
| Annotation | Aligned dimensions between parallel references, one segment or a chain: wall faces, centre lines and ends, door and window centres and jambs, grids; angular dimensions between two lines that meet, and radial ones on round columns and curved walls; all measuring the model as it is, so they follow what they measure (a change that would turn a reference out of parallel with the others is refused). Door and window tags showing their marks, going where their doors and windows go, with or without a leader; tag everything not tagged yet in one go. Text notes in plans, sections, elevations and on sheets, of any size and angle, over several lines, with a leader; spot elevations reading the top of what they stand on in plan (a floor, a roof's slope, a stair's tread), or the height of a point in a section |
| Views | A floor plan per level with Revit's view range: cut at its cut plane (1200 mm up by default), what lies below seen from it down to the bottom, faint beyond it down to the view depth, and roofs overhead up to the top: wall poché with clean joins at any angle, compound walls' layers, hatched with their materials' cut patterns where they're wide enough (else lighter where they aren't structure), door swings, window glass, room tags, grid bubbles, dimensions, door and window tags; PNG and SVG. Sections and elevations along any line: the cut in poché (a roof's along its slope, a compound wall's, floor's, roof's or ceiling's with its layers), what lies beyond in hidden line, levels named with their heights, grids; the four building elevations in one go; their marks on every plan; PNG. Crop regions on plans and sections: the view shows only what's inside, grids stopping at its edge with their bubbles. A reflected ceiling plan per level: its walls cut at 2300 mm (over the doors, through the windows), looking up at the ceilings, their tiles and the shafts through them. A 3D view, from any angle: walls with their doors and windows cut out, slabs and ceilings with their shafts cut out, columns, stairs and roofs, door leaves and glass, shaded (each layer in its material's colour), with an edge wherever faces meet, but none between the facets of a curve (a curved wall's, a round form's, an imported mesh's where its triangles turn less than 20°), which shades smoothly across them; PNG |
| Visibility and graphics | What each view hides, and how it draws the rest, as Revit's Visibility/Graphics does, for each plan, section, elevation and 3D view: whole categories left out (walls, doors, rooms, grids, dimensions, levels in a section…), or single elements; lines and fills overridden by category, by element and by view filter: a colour, weight and dash for what the view cuts and for what it sees (a plan's below its cut plane, the annotation; in 3D, and in a section beyond its line, the edges' colour), the cut's poché colour or a 3D view's surface colour, halftone (halfway to the paper) and, in 3D, transparency, where what lies behind shows through and a click still picks the element (halftone and transparency only add: a filter or an element can't take its category's away). A hidden door or window leaves its opening in the wall, and snaps, Tag All, Select All and a view's framing take what it shows. View filters pick elements out by rules, a schedule filter's (a field of the category, how to compare it, a value, project parameters among the fields), on the categories chosen; a view applies them in order, the first prevailing where they disagree, and one that hides hides whatever the others say; an element's own override beats the filters', and those beat the category's. View templates keep a view's categories, filters, phase, design options and, for a plan, view range under a name, for other views to follow: what a template controls isn't the view's to change, and changed, every view following it follows. A project's filters and templates are saved with it; sheets print each view as its graphics say |
| Phasing and design options | Phases in order, as Revit's Phasing has them (a project starts with Existing and New Construction): each element is made in one (its `Phase Created`, the first unless it says) and may be taken down in a later one (`Phase Demolished`), so in a view's phase it is new, existing, demolished (taken down in it) or temporary (made and taken down in it), and what is made after the phase, or taken down before it, isn't there. A view has a phase (the project's last unless it says) and a phase filter, which shows each status as its category does, as the project's overrides for that status say (existing in halftone and demolished dashed red to begin with, both yours to change) or not at all; a project starts with Show All, Show Complete, Show New, Show Previous + Demolished and Show Previous + New. A door or window with no phase of its own follows its wall. Phases are added, inserted, renamed and combined, and what was made, taken down or shown in one goes along; phase filters are made and changed. Design options are sets of options, as Revit's Design Options: an element in an option of a set (`Design Option`, `set: option`) shows only in a view that shows that option, the set's primary unless the view chooses, and one in none shows everywhere. Options are added, renamed, made the primary and taken out (one with elements in it stays until they are moved); accepting the primary keeps its elements and takes the others' and the set away, and deleting a set takes its elements too. What a session makes is made in the phase of the view it is drawn in, when that view has a phase or a phase filter of its own, and in the active design option; a copy is made in them too, and isn't taken down with the element it was copied from, while what is made with a phase or an option given keeps it. Schedules and view filters read the phase and the option as fields (a schedule sorts and groups the phases in the project's order, Existing before New Construction, whatever their names; a rule that names a phase, an option or a set follows it when it is renamed or combined); the desktop's Demolish tool (DW) takes an element down in the phase of the view it is clicked in, and a second click takes it back; templates can control a view's phase and its options; sheets print each view in its phase; a project's phases, filters, overrides and sets are saved with it. A project saved before phases that holds a project parameter named `Phase Created`, `Phase Demolished` or `Design Option` opens with it renamed (`Phase Created Parameter`), its values with it |
| Sheets | ISO A0 to A4, landscape, with a title block (project, sheet name and number, scale); plans, sections, elevations and the 3D view placed on them at a scale and titled, schedules as ruled tables, text notes on the paper, a view on one sheet; PNG, and PDF: one sheet, or every sheet a page |
| Schedules | Schedule views of rooms, doors, windows, walls, floors, ceilings, roofs, columns, components, stairs, ramps, railings or levels, as Revit's Schedule/Quantities makes them: the fields chosen and ordered from the category's own (Area, Width, Count…), the project parameters for it and, for components, their families' parameters; calculated fields, formulas over the row's other fields as a family's are (`Area * 120`, `Perimeter * Finish Height / 1e6`); filters (equals, greater than, contains, begins with, has a value…) that every row holds to, comparing values as the schedule shows them; sorting by fields in turn, ascending or descending (the phase fields in the project's order of phases, not by name), and grouped, a row for each set of their values, its count and totals summed; totals under the rows. Every project has a Room, Door, Window, Wall, Floor and Level Schedule, each the project's own once changed; CSV; on sheets, totals and all |
| Project parameters | Values of your own that each element of their categories holds (text, numbers, lengths, yes or no, unset a no), as Revit's project parameters are: set in Properties or with `modify`, shown, filtered and sorted in schedules and read by their formulas, and written to IFC in a `Project Parameters` property set and read back from it; renamed, schedules follow; taken off a category, its elements and schedules lose them |
| Units | Lengths shown and typed in millimetres, centimetres, metres, fractional inches, decimal feet, or feet and fractional inches (to 1/16"), areas in m² or ft², in drawings, schedules and the desktop alike; a length typed with its own unit (`10' 6 1/2"`, `3 m`) reads as that. The model, files and every command keep millimetres |
| Languages | English, Brazilian Portuguese or Spanish, for the desktop and for each project: a project's language names what commands make (levels, sections, elevations, rooms, schedules) and is what its drawings print (SOBE or SUBE on stairs, ESCALA in the title block, schedule headings); what's already named stays. Commands' messages stay English, for agents |
| Families | BIMCraft's own parametric components, never Autodesk's `.rfa`: a family has parameters (lengths, numbers, angles, yes or no, materials; each type's, or each component's), types that set them, and forms, each a profile drawn in its own axes and swept across them, every size a number or a formula as Revit's are (`Width / 2`, `if(Depth > 600, 4, 2)`), each form of a material, there while a yes/no parameter or a formula says. A new project comes with ten of them (tables, chairs, beds, sofas, desks, wardrobes, base cabinets, WCs, washbasins, bathtubs), named and worded in its language; more load from .json files. A component places a family's type at a point, turned and mirrored, its instance parameters its own; plans show it from above, the parts under a higher one hidden (a table's legs under its top), and sections and elevations beyond their line, never cut, as Revit shows furniture; the 3D view shades each form in its material's colour, textured as its material is. A form may be turned about an axis in its plane (a pipe, a bowl, a shade: `axis`, and `from` and `to` in degrees, up to a full turn), and a `void` form cuts the solid forms listed before it that it passes through (a hole, a notch, a sink's bowl, a sash round its pane), exactly: a solid cut by a solid, in any shape. The Family Editor in the desktop edits a family's parameters, types, forms and symbols as text, shows it on a preview you can turn, and makes it the project's, saves it to a `.json` file, or keeps it, with the materials it's made of, in the Favorites |
| IFC import | IFC2x3 and IFC4 files, as Revit and other BIM tools write them: storeys, straight and curved walls with their doors and windows (cut to a roof over them, attached to it), floors level or sloped and ceilings, with the holes in them, rooms, columns, grids, stairs and ramps of straight flights with the landings between them, railings (by their axis, or a straight one by its body), roofs flat, sloping all round over any footprint with gable ends, courtyards and openings through them, or one profile swept across, and the openings through floors, ceilings and roofs (as shafts), each measured from its geometry, whether the file extrudes a profile, writes a Brep or a triangle mesh; material layer sets as their types' layers, stacked from the face the file says, with each material's colour; furniture, sanitary terminals, light fixtures and proxies as components of families made of their bodies, a family for each family the file names, a type for each type, and a mirror image of a type that type mirrored; project parameters from a `Project Parameters` property set, each of the kind its values are; phases and design options from a `BIMCraft_Phasing` property set (the project's phases, phase filters, overrides and option sets from the one on the project, each element's phase and option from its own; what the project can't have is left out and listed, and the element stays); walls that nearly meet are joined; a model at map coordinates is moved near the origin; what can't come in is listed, with why |
| Files | `.bimcraft` projects (JSON); IFC4 export with storeys, walls swept up from their plan outline (mitred where they join, an arc in short chords) or, attached to a roof, as closed shells, walls, floors, roofs and ceilings with their material layer sets (each material with its colour), slabs (sloped ones as closed shells), ceilings as coverings, spaces and columns, stairs and ramps as their flights, landings and railings (each railing with its rail's top line as its axis), roofs as closed shells of flat faces (hip, gable, shed or flat), openings that cut the walls and hold the doors and windows, an opening for each slab, ceiling and roof a shaft passes through, and components as furniture, sanitary terminals, light fixtures or proxies, named `Family:Type` as Revit names them, each form a closed face set in its material's colour, each element's values of the project parameters in a `Project Parameters` property set (dimensions, tags, notes and spot elevations belong to views, and stay out of it), and each element's phase and design option in a `BIMCraft_Phasing` property set: its `Phase Created` is always named in it, the first phase too (said to be implied, for another tool to read; a door or window says only what it holds, as one with no phase follows its wall), `Phase Demolished` and `Design Option` where it holds them, and the project's phases in order, with the phase filters, overrides and option sets it changed, in one on the project; a project that uses none of it writes none |
| Agents | MCP server, scripts, and every command callable with JSON |
| Desktop | Revit's layout: ribbon tabs with a contextual Modify tab, the Options Bar, Properties with the Type Selector and Type Properties, the Project Browser and views in tabs; a plan and a reflected ceiling plan per level that pan and zoom, sections and elevations, a 3D view that orbits, and sheets; tools for walls (straight or arcs, on the location line picked), doors, windows, columns, roofs by footprint (and Gable Edge, to stand a roof's edge upright) and by extrusion, dormers, floors (and a floor's slope arrow), ceilings, shaft openings, stairs and ramps (run by run, turning at landings), railings, rooms, levels, grids, dimensions (aligned, angular, radial), spot elevations, text, tags, sections and elevations; Flip, Attach Top/Base and Detach Top/Base for walls; a wall, floor, roof or ceiling type's layers in Type Properties, and the project's materials in Manage > Materials; project parameters in Manage > Project Parameters, and their values in Properties; schedules from View > Schedules, their fields, filters, sorting and totals in Schedule Properties; the Component tool, families and their types in the Type Selector, the Project Browser and Type Properties, a component's own parameters in Properties, and Load Family; Crop View with grips to drag a crop region's edges, and the view range in Properties; project units, metric or feet and inches; Revit's selection (click, Ctrl, Shift, Tab, window and crossing boxes); dragging and the Modify panel (Align, Offset, Mirror, Split, Move, Copy, Rotate, Trim/Extend to Corner) and its Phasing panel (Demolish); Paint and Remove Paint on its Geometry panel, for the 3D view; typed lengths and temporary dimensions; undo and redo; open, save and export; a light and a dark theme; English, Portuguese or Spanish. The Manage tab has the Materials window (with textures and pictures), the Favorites, and the Family Editor (New Family, Edit Family) |
| Updates | The desktop app asks GitHub for the latest release when it starts, on a thread of its own, so it never delays the start, and says so in a banner when one is newer: Update now, Later or Skip this version. Update now downloads the new version and checks its SHA-256 against the digest GitHub lists for it; Restart and install then asks to save, closes, replaces the program in place and starts again with the same project. That works for the copy the Windows installer made, an AppImage and the macOS app, where you can write to their folder. Any other copy (a package from your distribution, a tarball, a build from source) gets a Download button for the release's page, and a Flatpak, which has no network, never checks. File > Check for updates at startup switches the check off, and `BIMCRAFT_NO_UPDATE_CHECK` keeps the app from making any request. `bimcraft-cli` and the MCP server never phone home; see [Updates](#updates) for what the check does and doesn't protect against, and for what has run on which system |

The IFC4 export is checked with [IfcOpenShell](https://ifcopenshell.org) 0.9: the sample house has no schema or EXPRESS rule
errors, every element builds its geometry, and the walls' volumes (with the openings cut out), the roof's and the slabs' and
ceilings' (with their shafts cut out) match a hand calculation, as do the roofs' of an L-shaped house with a dormer and an
extrusion roof beside it, and of houses round a courtyard, their floors and ceilings open over it too, and of a house of
layered walls, floors, ceilings and roofs, each with its material layer set, and of a flat furnished from the library, a
mirrored chair among it. Import is checked against it too: the sample comes back the same from IFC, element by element, also with its
geometry turned into triangle meshes by IfcOpenShell, and a building IfcOpenShell's API writes comes in whole, its roofs
(slabs per slope, round a courtyard too, or a profile extruded) and its floors and ceilings round a courtyard (a profile with
a void, or a mesh) among it, and its walls' and slabs' layers on the faces their layer sets are stacked from, and its furniture (a stool type placed
twice, one turned, a proxy, a meshed basin) as components where it stood. Project parameters go out as property sets IfcOpenShell
reads back typed as they were (text, a real, a length, a boolean), and come in from a property set its API writes, in metres. A phased sample has no schema or EXPRESS rule errors either, and
IfcOpenShell reads each element's `BIMCraft_Phasing` set and the project's list of phases; they come back from IFC as they went
out, and from a set its API writes (labels and a list value), while a set another tool calls its own, `Phasing` say, is left unread.

## Quick start

The engine and the CLI need Rust 1.90 or later; the desktop app needs 1.95 (egui 0.36).

```sh
git clone https://github.com/IPedrax/bimcraft
cd bimcraft
cargo run --release -p bimcraft -- --sample       # opens the sample house
```

Without a window:

```sh
cargo build --release
./target/release/bimcraft-cli run --sample --save house.bimcraft --save house.ifc --save plan.png
```

That builds the sample house and saves the project, an IFC4 file and the Level 1 plan:

<p align="center">
  <img src="docs/images/sample-plan.png" alt="Level 1 of the sample house: grey wall poché, door swings, windows, its furniture seen from above, room tags with areas, grid bubbles, dimensions, door and window tags, a spot elevation and a note, and the roof's edge dashed overhead" width="100%">
</p>

`--cmd 'render {"view": "{3D}", "path": "house-3d.png"}'` adds the 3D view (`azimuth` and `elevation` turn it);
`render {"view": "Section 1"}` draws the sample's section, `South` (or `North`, `East`, `West`) one of its elevations, and
`render {"view": "A102"}` one of its sheets. `--save sheets.pdf` prints every sheet, a page each.

A script is one command per line, `id {json params}`, with lengths in millimetres:

```text
wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true, "type": "Exterior - 300mm"}
door {"at": [2000, 0]}
window {"at": [8000, 3000]}
room {"point": [4000, 3000], "name": "Living"}
floor {}
roof {}
```

```sh
bimcraft-cli run --script house.txt --save house.ifc
bimcraft-cli convert building.ifc building.bimcraft   # an IFC file from Revit or any BIM tool, as a project
bimcraft-cli info house.bimcraft                      # levels, counts, types and warnings
bimcraft-cli convert house.bimcraft plan.svg
bimcraft-cli commands                                 # every command and its parameters
```

## The desktop app

`bimcraft [FILE] [--sample]` opens a `.bimcraft` project, imports an `.ifc` file, or starts on the sample house. The layout
is Revit's, so it should feel familiar: the Quick Access Toolbar; the ribbon's File, Architecture, Annotate, View and Modify
tabs, with a green contextual tab (Modify | Place Wall, Modify | Walls) for the tool or the selection; the Options Bar;
Properties over the Project Browser; views in tabs with a navigation bar; and the tool's prompt in the status bar. The
icons and colours are BIMCraft's own, in a light theme or a dark one: BIMCraft starts in your system's, and View > Dark Mode
switches.

| To | Do |
|---|---|
| Draw walls | Wall (WA), click points; Enter builds the chain, Esc drops it, a click back on its start closes it. Ends snap to wall ends and location lines; Shift keeps a segment square. The Draw panel draws a Line, or a Start-End-Radius Arc: its start, its end, then a point it passes through. The Options Bar sets the height (up to a level, or unconnected), the Location Line (the line of the wall you click along) and Chain. A wall's exterior is to your left as you go, so draw clockwise for walls that face out |
| Flip, attach walls | Select walls, and Modify \| Walls has Flip (or Space) to turn their exterior side round about their location line, Attach Top/Base to pick a roof or a floor for their tops (or, as the Options Bar says, their bases) to follow, and Detach Top/Base. Properties shows their Location Line (pick another and the wall stays where it is) and whether they're attached |
| Place doors, windows | Door (DR) or Window (WN), click a wall; the Type Selector at the top of Properties picks the type |
| Place rooms | Room (RM), click inside walls; the outline and area show before you click, and the tag's area follows the walls |
| Place columns | Column (CL), click where it stands; it snaps to wall ends and centre lines. The Options Bar sets how high it goes (up to a level, or unconnected), the Type Selector rectangular or round |
| Add stairs, ramps | Stair: click where it starts (the middle of its first riser), then where its run ends. Past the end of a run of all its risers, it climbs to the level above in one run; short of it, a landing follows, and the next click says which way the next run goes from it: on, left, right, or back beside the last. Risers are at most 180 mm; the Options Bar sets its width and Railings, Properties its riser, tread and top. Ramp: the same clicks, its runs as long as the Options Bar's slope makes them, as high as its Height says |
| Add railings | Railing: click a stair or a ramp for a railing down each side, or click a path's points and press Enter for one along it on the plan's level; the Type Selector picks its type, Type Properties its height and baluster spacing |
| Roof the building | Roof, in a plan: click inside the walls, and a roof goes on the level above, over their outline and past their faces, whatever its shape, and open over a courtyard: a loop of walls standing free inside the others, with no room in it; its hips and valleys show before you click. The Options Bar sets the slope (0° for a flat roof), the overhang, and gable ends for a rectangle. Select a roof, and Modify \| Roofs has Gable Edge: click an edge of its footprint (a courtyard's too) to stand it upright, a gable end, and again to slope it. Roof by Extrusion: click a line's start and end, then how far across the roof is swept; its profile rises from the line at the Options Bar's slope to a ridge over its middle, or with Shed along its whole length |
| Dormers | Architecture > Opening > Dormer: click a roof's slope where the dormer's front wall goes; the Options Bar sets its width, its height over the roof at the front, its roof's slope and whether its front has a window. Its walls stand on the roof, its own gable roof runs back into the host, and the host opens under it |
| Floors, slopes | Floor: click inside the walls, and a slab goes under their outline, open over a courtyard (a loop of walls standing free inside the others, with no room in it), its top at the level; Properties sets its Height Offset From Level. Selected, Shape Editing > Slope Arrow: click its tail, then its head, and the floor rises toward the head at the Options Bar's slope; Properties changes the slope |
| Ceilings | Ceiling: click inside a room, and a ceiling fills it out to its walls' faces, open round any walls standing free inside it, at the Options Bar's height (2600 mm); the outline shows before you click, and the Type Selector picks a plain one or tiles. The Project Browser's Ceiling Plans open each level's reflected ceiling plan |
| Shafts | Architecture > Opening > Shaft: click two opposite corners; the Options Bar sets how high it goes (up to a level, or unconnected). It cuts the floors, ceilings and roofs on its way |
| Add levels, grids | Level (LL) takes an elevation in the Options Bar; Grid (GR) takes two clicks |
| Select | Click; Ctrl adds, Shift takes away, Tab reaches what's underneath (the wall under a door). Drag across empty floor for a box: left to right takes what's wholly inside, right to left what it crosses too. The selection stays as you open other views, the 3D view and schedules among them |
| Edit, delete | Properties edits the parameters of everything selected, in one step, grouped as Revit groups them; the Type Selector changes the type; Delete removes the selection |
| Move, copy | Drag a selected element, or Move (MV) and Copy (CO): click a base point, then where it goes. Walls joined to a moved wall stretch to stay joined, so room areas follow; doors and windows slide along their walls. Constrain keeps the move square; Multiple goes on copying |
| Rotate, mirror | Rotate (RO) turns the selection about its middle: click the angle's start, then its end, or type an angle in the Options Bar; Place Centre moves the centre. Mirror Pick Axis (MM) mirrors across a wall or grid you pick, Mirror Draw Axis (DM) across a line you click; Copy is on, as in Revit |
| Align, trim, split, offset | Align (AL): pick a reference line (a grid, a wall's face or centre line), then the wall to line up with it. Trim/Extend to Corner (TR): pick two walls on the parts to keep. Split Element (SL): click a wall where to cut it. Offset (OF): point to a wall, and a copy goes to the cursor's side at the Options Bar's distance |
| Dimension, tag | Annotate > Aligned (DI): pick parallel references, each the line nearest the cursor (a wall's face, centre line or end, a door's or window's centre or jamb, a grid), then click away from them where the dimension line goes; more picks make a chain. Angular: pick two lines that meet, then click where the arc goes; the angle on that side. Radial: pick a round column, then click where the radius goes. Annotate > Tag by Category (TG): click a door or window to tag it with its mark, with a leader when the Options Bar says so; Tag All tags the rest of the view. Drag a dimension or a tag to move it |
| Spot elevations, text | Annotate > Spot Elevation (EL): click a floor, a roof, a stair or a wall in plan, and it reads the height of its top there, following it; in a section, click a point to mark its height. Annotate > Text (TX): type the note in the Options Bar (`\n` breaks a line), then click its top left corner, in a plan, a section or on a sheet; Properties sets its size and angle |
| Crop, view range | View > Crop View crops a plan or a section to what it shows; drag the grips at its edges' middles to move them (a section's sides move its line). Off shows it all again. Properties of a plan holds its View Range: Top (clear it for the level above), Cut Plane, Bottom and View Depth, in mm above the level |
| Visibility/Graphics | View > Visibility/Graphics (VV, VG) opens the active view's dialog: the Model Categories and the Annotation Categories, each visible or not, with Override… for how it draws (projection and cut each have a line colour, weight and dash and a fill colour, each with a box that turns it on; halftone; transparency, seen in 3D); Filters applies the project's filters in order, each visible or not and overridden alike, with Add, Remove, Up and Down; Elements lists what's hidden or overridden one by one. Design Options picks which option of each set the view shows (the primary unless it chooses). OK and Apply send what changed as one `graphics` command, one undo step; Cancel drops it. A view that follows a view template shows what the template controls, and leaves it be |
| Hide in View | Select, then Modify > View: Hide Element (EH) leaves the elements out of the view, Hide Category (VH) every category the selection is of, Override Graphics (EO) opens Visibility/Graphics on its Elements tab with the selection added. Unhide Elements (EU) opens that tab, with what's hidden; tick a box to bring it back |
| Filters, view templates | View > Filters (FT): New makes a view filter; tick its categories and add rules as a schedule's filter has them. View > View Templates (VT): name one and Create from This View; tick what it controls (categories, filters, view range, phase and phase filter, design options); Follow in This View, or the View Template field in a view's Properties, makes the view follow it |
| Phasing | Manage > Phasing (PH). Project Phasing lists the phases in order: type a name and Add at End, Insert Before or Insert After the one picked, Rename it, or Combine with Previous or Next, which takes it out and gives what was made, taken down and shown in it to its neighbour. Phase Filters picks a filter (New Filter makes one) and sets how it shows each of New, Existing, Demolished and Temporary: By Category, Overridden or Not Displayed. Graphic Overrides edits how each status draws where a filter says Overridden, and Apply Overrides sends it as one command. A view's Phasing group in Properties holds its Phase Filter and its Phase (the last unless one is picked); a selected element's holds Phase Created and Phase Demolished, and its Design Option once the project has any |
| Demolish | Modify > Phasing > Demolish (DW), then click an element in a plan, a section or the 3D view: its `Phase Demolished` becomes the view's phase (the project's last unless the view has one), as one undo step, and it shows demolished there by the view's phase filter. Click it again, and it comes back. A door or window with no phase of its own comes down with its wall without being touched, and a click on it says so; something that holds no parameters, such as a dimension, says so and stays as it was. The tool stays on until Esc |
| Design options | Manage > Design Options (DO): New Set (with a name typed) makes a set with one option; pick an option and Add Option, Rename Option, Make Primary or Delete Option, or Edit Option, which makes it the active one. Accept Primary keeps the primary's elements and takes the others' and the set away; Delete Set takes the set with every element in its options. The status bar's Active design option (once there are sets) is what new elements are made in, Main Model for none, and the view on screen shows it too; it turns red in a view that shows another option of its set, and picking it again shows it there |
| Lengths | While a tool waits for its next point, type a length and press Enter: it goes that far toward the cursor. A selected wall shows its length as a temporary dimension; click the number to type a new one |
| Snaps and the ruler | While a tool picks points, what a click would land on is marked: a square for an end, a triangle for a middle, a cross for a crossing, a circle for a centre, a right angle for a perpendicular, a diamond for the nearest point of a wall, a dot on an angle guide, each named beside it. Past the last point a dashed guide runs at the angle the direction sticks to (from east, and from the line before), and the new line wears a ruler: its length and angle read beside it, and at a corner the angle between the two walls, shown as an arc, so a 60° corner is easy to hit. Shift keeps a step square. Manage > Snaps turns each snap off, and sets the angle steps or turns them off; typed over the plan, SE, SM, SI, SC, SP or SN snap to endpoints, midpoints, intersections, centres, perpendiculars or the nearest point alone for the next point, SO to nothing, and SS puts the snaps back (the status bar says which is waiting; a click, Esc, another tool or another view ends it) |
| Measure | Annotate > Measure (ME): click two points; a ruler with its graduations lies between them, reading its length, its angle and its parts along x and y. Esc takes the ruler away, then the tool |
| Turn a wall's points | Select one wall: a grip stands at each end. Drag one to move that end: its angle and length follow the cursor, with the snaps and angle guides from the other end, and the walls joined there follow; Ctrl keeps the wall's length, so it turns about its other end. Beside each end is the wall's angle: click it and type a new one, and the wall turns about that end (a wall joined at its other end stays joined, the corner sliding along the wall it meets) |
| Edit types | Edit Type opens Type Properties: change the type everywhere it's used, or Duplicate it into a new one. A wall type's layers, from the exterior in, or a floor's, roof's or ceiling's, from the top down: each one's function, material and thickness; Insert Layer, and Out and In (Up and Down) and Delete on each |
| Place components | Component (CM): pick a family's type in the Type Selector, or in the Project Browser's Families, then click where it goes; Space turns it a quarter turn first. Selected, its own parameters are in Properties, its type's in Type Properties (Edit Type), with Duplicate. Load Family, on Modify \| Place Component, loads a family saved as .json. A door's or a window's family is picked in the Type Selector of the Door or Window tool, or of a selected door or window, as `Family: Type` beside the project's own types, which a new door takes from the project's single door or fixed window; its Edit Type is the family type's |
| Family Editor | Manage > New Family (a category's template) and Edit Family (the family of what's selected, or of the component being placed): the family's name and category; Parameters (name, kind, instance, value, formula); Types (a name and the type parameters' values, a blank one the family's own; add, duplicate, delete); Forms (a profile's corners, a circle, a revolve's axis and degrees, a void, its plane, from and to, material and the parameter or formula it's visible by; moved up and down, deleted; Add Box, Cylinder, Revolve, Void); Symbols for a door or window. A preview of the type picked, turned by dragging. Apply makes the project's family what's typed, one undo step; OK also closes; Save Family writes a `.json` file; Load Family reads one; Add to Favorites keeps it in your Favorites |
| Favorites | Manage > Favorites: the materials and families you keep for every project, listed with a swatch; Add to Project (under another name when the project has that one) and Remove. A material comes in with the picture its texture is made of, a family with the materials it's made of |
| Paint | Modify > Paint (PT), in the 3D view: pick the material in the Options Bar, then click a face; Remove Paint takes it off the face clicked |
| Materials | Manage > Materials: the project's library. Pick a material, or New for another, and set its name, colour, cut pattern and surface pattern; Delete takes one out that no layer uses. A layer picks its material from the library. Texture picks a pattern drawn in code, or Picture… a PNG or JPEG file from your disk, for the 3D view; its tile's width and height, angle and offset follow; Save to Favorites keeps the material, and Favorites… opens them |
| Project parameters | Manage > Project Parameters: New makes one; name it, choose its type of parameter (text, number, length or yes/no) and the categories it's for. Select an element and its values are under Project Parameters in Properties; empty a field to take a value away |
| Schedules | View > Schedules > Schedule/Quantities: pick a category and name it. In Properties, a schedule's Fields, Filter and Sorting/Grouping each open Schedule Properties: add its category's fields, take them away and order them, give one a heading or Calculate totals, add a Calculated Value (`Area * 120`); filter its rows; sort them, and untick Itemize every instance for a row per group. The Room, Door, Window, Wall, Floor and Level Schedules open from the browser, and become the project's once changed. A schedule highlights the rows of what's selected; click a row to select what it lists (Ctrl adds, Shift takes away) |
| Views | The Project Browser opens floor plans, ceiling plans, elevations, sections, the 3D view and schedules in tabs; View > Close Inactive closes the others |
| Lay out sheets | View > Sheet makes one (A101, A102…); on it, Place View puts a view that's on no sheet yet: a plan, a section or an elevation at 1 : 100, the 3D view as it's turned on screen, or a schedule. Drag a view or a note to move it, set its scale in Properties, double-click it to open it. File > Export > Sheets as PDF prints them all |
| Cut a section, add an elevation | View > Section (CR): click its start and end; it looks to the left of the line, so drawn west to east it looks north. View > Elevation: click beside the building, and it looks at that side. Open them from the browser, or double-click their marks in a plan; in them, click to select what they cut or see |
| Look in 3D | The cube on the Quick Access Toolbar (or View > 3D View, or {3D} in the browser) opens the 3D view. Shift and the middle button orbit, as in Revit, and so does a drag; the middle button pans, the wheel zooms; Home in the navigation bar goes back to the south-east view. Click to select, as in plan |
| Move around | The wheel zooms at the cursor, the middle button pans, the navigation bar or ZF (ZE, ZA) zooms to fit; TL toggles Thin Lines |
| Shortcuts | Type a tool's two letters over a view, as in Revit (WA, DR, CR…). CS (Create Similar) starts the tool of the one element selected, with its type picked; SA selects every element of the selected ones' types that the view shows; PP shows or hides Properties; UN opens Project Units; VV (VG) opens Visibility/Graphics, FT Filters and VT View Templates; EH, VH and EU hide an element, a category, or list what's hidden; PH opens Phasing, DO Design Options and DW picks the Demolish tool. KS lists them all |
| Go dark | View > Dark Mode: the window, plans, sections and the 3D view go dark; sheets stay white paper |
| Units | Manage > Project Units (UN): millimetres, centimetres, metres, inches, feet, or feet and inches. Dimensions, spot elevations, room areas, schedules, Properties and the Options Bar follow; fields read what's typed in those units, or in the unit typed with it, and so do lengths typed while drawing |
| Language | View > User Interface > Language: English, Português (Brasil) or Español; the window starts in the system's language (on Windows and macOS, its display language). Manage > Project Language: the language the project's drawings print, and that new levels, views, sheets and rooms are named in. A new project takes the window's |
| Undo, redo | Ctrl+Z, Ctrl+Y, or the Quick Access Toolbar |
| Files | Ctrl+N, Ctrl+O, Ctrl+S; Open takes IFC files too, and a note says what came in and what was left out; the File tab saves, and exports IFC, the sheets as PDF, and the view on screen as PNG (or a plan as SVG) |
| Updates | File > Check for Updates Now looks by hand, and File > Check for updates at startup (on) says whether BIMCraft looks when it starts. A banner under the Options Bar offers a newer version: Update now (for the Windows installer's copy, an AppImage or the macOS app: it downloads and checks the update, then Restart and install asks to save, closes BIMCraft, replaces it and opens the same project again), or Download (every other copy: it opens the release's page), Later (it asks again next time), and Skip this version (not until a newer one) |

Each of these runs a command, so what you do in the window is checked, undone and saved exactly as from the CLI or an agent.

## Drive it from agents, MCP and the CLI

`bimcraft-cli mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io) over stdio, on a model of its own.
Add it to Claude Code:

```sh
claude mcp add bimcraft -- /path/to/bimcraft-cli mcp
```

or to any MCP client's configuration:

```json
{ "mcpServers": { "bimcraft": { "command": "/path/to/bimcraft-cli", "args": ["mcp"] } } }
```

| Tool | What it does |
|---|---|
| `execute {command, params}` | Any command with JSON parameters |
| `script {text}` | Several commands, one per line |
| `list_commands {filter?}` | The catalog, with parameters and shortcuts |
| `inspect_model` | Levels, counts, types, warnings and undo history |
| `query_elements {category?, level?}` | Elements with derived values: wall length and height, room area, door position |
| `schedule {name?, category?, fields?, filters?, sort?, grouped?}` | A schedule's columns, rows and totals; a new one, or one changed, as the command makes it |
| `render {view?, ceiling?, width?, height?, azimuth?, elevation?}` | A level's floor plan (or reflected ceiling plan), a section or elevation by name, a sheet by number, or with `view: "{3D}"` the 3D view, as a PNG the agent can look at |
| `new_model`, `open`, `save` | Projects; `open` imports `.ifc` files, `save` to `.ifc`, `.svg` or `.png` exports |

A command that fails comes back as a tool error saying what to fix (`door: no wall at [4000, 3000] on Level 1`), and it
changes nothing.

## Commands

| Command | Shortcut | Parameters |
|---|---|---|
| `wall` | WA | `points`, `closed?`, `type?`, `level?`, `top_level?`, `height?`, `base_offset?`, `arc?` (degrees each turns, counter-clockwise), `location?` (`centre`, `core-centre`, `exterior-finish`, `interior-finish`, `exterior-core`, `interior-core`), `attach_top?`, `attach_base?` (a roof or a floor) |
| `door` | DR | `at`, or `wall` and `offset?`; `type?`, `family?` (a door family's `type`, its first by default), `mark?`, `flip_facing?`, `flip_hand?` |
| `window` | WN | as `door`, plus `sill?` (a family's `Sill Height` by default) |
| `floor` | | `boundary?` (else the walls of the level), `openings?` (holes in it, inside it and apart; over the walls, by default each loop of them standing free inside, joined to none of the outer ones and with no room in it), `level?`, `type?`, `offset?` (its top above the level), `slope?` (degrees) and `arrow?` (its tail and head: it rises from the tail toward the head) |
| `ceiling` | | `point` (in a room: out to its walls' faces, open round the walls standing free inside it) or `boundary`, `openings?` (holes in it, inside it and apart), `level?`, `type?`, `offset?` (its underside above the level, 2600) |
| `shaft` | | `boundary`, `level?`, `top_level?`, `height?`, `base_offset?`: a hole through the floors, ceilings and roofs on its way |
| `room` | RM | `point`, `name?`, `number?`, `level?` |
| `column` | CL | `at`, `type?`, `angle?`, `level?`, `top_level?`, `height?`, `base_offset?` |
| `stair` | | `start`, `end?` or `angle?` (the way it climbs), `level?`, `top_level?`, `height?`, `width?`, `riser?` (the most each riser may rise), `tread?`, `landings?` ([{`at`, `turn`}]: after `at` risers, the next run going on (0), turned left or right (90, -90), or back beside it (180, -180)), `railings?` |
| `ramp` | | `start`, `end?` or `angle?`, `level?`, `top_level?`, `height?`, `width?`, `slope?` (percent, 8.33), `landings?` (`at` mm up), `railings?` |
| `railing` | | `host` (a stair or a ramp) and `side?` (`left` or `right`; both by default), or `path`, `level?`, `base_offset?`; `type?` |
| `roof` | | `level?` (the one above the lowest), `offset?`, `boundary?` (else the walls of the level below), `courtyards?` (holes in it with eaves of their own, inside it and apart; over the walls, by default each loop of them standing free inside, joined to none of the outer ones and with no room in it), `overhang?`, `slope?` (0 is flat), `gables?` (edges left upright, edge i from point i to the next, a courtyard's numbered on after the boundary's), `gable?` (a rectangle's short ends), `extrusion?` (`line`, `profile` of `[along, up]` points, `from?`, `to`: swept across to the line's left), `openings?`, `join?` (a roof it ends against), `type?` |
| `dormer` | | `host` (a roof), `at` (on its slope: the front wall's middle), `width?`, `height?` (over the roof at the front), `slope?`, `window?` (a window type), `type?` (its walls') |
| `dimension` | DI | `refs` (each `{id, face?}`: a wall's `centre`, `left` or `right` face, `start` or `end`; a door's or window's `centre`, or its jamb at the `start` or `end` side; a grid's line; or a point on one), `at`, `level?`, `kind?` (`aligned`, `angular` between two lines that meet, `radial` on a round column) |
| `tag` | TG | `ids?` (without them, every door and window on the level not tagged yet), `level?`, `offset?`, `leader?` |
| `text` | TX | `text`, `at` (its top left), `view?` (a level's plan by default; a section, an elevation, or a sheet by number), `size?` (mm on paper, 2.5), `angle?`, `leader?` |
| `spot` | EL | `at`, `view?`, `host?`: in a plan, the top of what stands there; in a section or an elevation, the point's height |
| `section` | CR | `start`, `end` (it looks to the left of the line), `depth?`, `name?` |
| `elevation` | | `facing?` (north, south, east or west: the side it shows), `at?`, or `start` and `end`; none of them: the four building elevations |
| `sheet` | | `number?` (A101, A102… by default), `name?`, `size?` (A0 to A4; A3 by default) |
| `place` | | `sheet` (its number), `view` (a level's, section's, elevation's, schedule's or 3D view's name: Room Schedule, `{3D}`…), `at?` (mm on the paper), `scale?` (100 for 1 : 100), `azimuth?` and `elevation?` for the 3D view |
| `crop` | | `view`, `region?` (two corners: plan mm, or along a section and up), or `null` to uncrop; none: to what the view shows |
| `graphics` | | `view?` (a level's, section's or elevation's name or id, or `{3D}`), `hide?` and `show?` ([categories]: walls, rooms, grids, levels…), `hide_elements?` and `show_elements?` ([ids]), `categories?` and `elements?` ({category or id: override}; an override is {`hidden?`, `projection?` and `cut?` ({`color?`, `weight?` (px), `dash?` (`solid`, `dashed`, `dotted`), `fill?`}), `halftone?`, `transparency?` (percent, seen in 3D)}, merged into the last, null takes it away), `filters?` ([{`filter`, plus an override's fields}, …], the first prevailing), `template?` (a view template's name, or null), `phase?` and `phase_filter?` (names, or null), `options?` ({set: option, null takes the choice away}), `reset?`: what a view hides and how it draws the rest; none of them: the view's settings as they are |
| `filter` | | `name`, `categories?` (any a schedule can be of), `rules?` ([{`field`, `op`, `value`}, …] as a schedule's `filters`), `delete?`: a view filter, and the ids it picks out; a view applies it with `graphics` |
| `template` | | `name`, `from?` (a view to take the categories, filters and a plan's view range from), `controls?` ([`categories`, `filters`, `range`, `phase`, `options`]), `view_range?`, the changes `graphics` makes to a view's categories, filters, phase and options, `delete?`: a view template, for views to follow with `graphics {template}` |
| `phase` | | `name?`, `before?` or `after?` (to add one in place), `rename?`, `delete?` with `into?` (the phase that takes what was in it), `now?` (the phase this session makes elements in), `overrides?` ({`new`, `existing`, `demolished` or `temporary`: an override, null takes it away}): the project's phases in order, and how a phase filter draws a status it says overridden. An element is made in a phase by its `Phase Created` parameter and taken down in one by `Phase Demolished` (`modify {params}`) |
| `phase_filter` | | `name`, `new?`, `existing?`, `demolished?`, `temporary?` (`by-category`, `overridden` or `not-displayed`), `delete?`: how a view shows each status of what is in its phase; a view applies one with `graphics {phase_filter}` |
| `option` | | `set?`, `options?` ([names]), `primary?`, `option?`, `rename?`, `delete?`, `accept?`, `now?` (`set: option`, or null: the option this session makes elements in): sets of design options; an element is in one by its `Design Option` parameter, and a view chooses which to show with `graphics {options}` |
| `level` | LL | `elevation`, `name?` |
| `measure` | ME | `from`, `to` (points, mm): what a ruler laid between them reads: `length`, `dx`, `dy`, and `angle` in degrees from east, counter-clockwise; it changes nothing |
| `units` | UN | `length?` (`mm`, `cm`, `m`, `in`, `ft` or `ft-in`): how lengths show in drawings, schedules and the desktop; commands keep mm |
| `language` | | `code?` (`en`, `pt` or `es`): the language the project's drawings print and new names come in; names already given stay |
| `grid` | GR | `start`, `end`, `name?` |
| `type` | | `category`, `name`, and its sizes: creates the type, or edits it everywhere it's used; a wall, floor, roof or ceiling type's `layers` [{`function?`, `material?`, `thickness`}, …], a wall's exterior first and the others' top first, make up its thickness |
| `family` | | `name`, `category?` (`furniture`, `casework`, `plumbing`, `lighting`, `generic`), `parameters?` [{`name`, `kind?` (`length`, `number`, `angle`, `yesno`, `material`), `instance?`, `value?`, `formula?`}, …], `types?` {type: {parameter: value}} (merged in; a null takes one away), `forms?` [{`plane?` (`xy`, `xz`, `yz`), `profile?` [[a, b], …] or `circle?` {`centre`, `radius`}, `from?`, `to`, `material?`, `visible?`}, …], `rename?`, `delete?`, `path?` (a .json file), `save?` (a .json file): a family, created, changed, loaded or saved; sizes in mm or formulas over its parameters. A form may have an `axis` [[a, b], [a, b]] in its plane (it's turned about it from `from` to `to` degrees, up to 360) and be a `void` (it cuts the solid forms listed before it); `category` may be `door` or `window`: a family hosted in a wall, with length parameters `Width` and `Height`, drawn in the opening's axes (x along the wall from its middle, y across it, + toward its exterior, z up from the sill) and reading `Wall Thickness`, and `symbols?` [{`line?` [[x, y], …] or `arc?` {`centre`, `radius`, `from?`, `to`}, `visible?`}, …]: the lines its plan draws |
| `component` | CM | `family`, `type?` (its first), `at`, `level?`, `angle?`, `offset?`, `mirrored?`, `params?` {parameter: value}: a family's type placed |
| `parameter` | | `name`, `kind?` (`text`, `number`, `length`, `yesno`), `categories?` (`wall`, `floor`, `ceiling`, `roof`, `door`, `window`, `room`, `column`, `component`, `stair`, `ramp`, `railing`), `rename?`, `delete?`: a project parameter; an element of its categories holds a value of it with `modify {id, params: {name: value}}` |
| `schedule` | | `name?`, `category?`, `fields?` [a field's name, or {`name`, `formula?`, `kind?` (`number`, `length`, `area`, `yesno`), `heading?`, `total?`}, …], `filters?` [{`field`, `op` (`equals`, `not-equals`, `greater`, `greater-or-equal`, `less`, `less-or-equal`, `contains`, `not-contains`, `begins-with`, `not-begins-with`, `ends-with`, `not-ends-with`, `has-value`, `has-no-value`), `value?`}, …], `sort?` [a field's name, or {`field`, `descending?`}, …] (`Phase Created` and `Phase Demolished` go by the project's order of phases, a phase never demolished after the last), `grouped?`, `path?` (.csv): a schedule made or changed, and its columns, rows and totals; `category` alone reads its own (Room Schedule…); `available` lists the fields there are |
| `material` | | `name`, `color?` (`#rrggbb` or `[r, g, b]`), `cut?` and `surface?` (`none`, `solid`, `diagonal`, `crosshatch`, `horizontal`, `vertical`, `grid`, `brick`, `concrete`, `insulation`), `texture?` ({`kind?` (`brick`, `block`, `boards`, `concrete`, `plaster`, `tile`, `roof`, `fabric`, `metal`, `insulation` or `image`), `size?` [width, height] mm, `angle?`, `offset?` [x, y], `path?` (a PNG or JPEG file taken into the project), `image?` (a picture's id)}, or null), `rename?`, `delete?`: a material of the project's library, created or changed; layers pick theirs from it |
| `paint` | PT | `id` or `ids`, `face?` (a wall's `exterior` or `interior`; a floor's, roof's or ceiling's `top` or `bottom`; `all`), `material` (null takes the paint off): a face of the elements painted with a material of the library, shown in the 3D view over what their types are made of |
| `favorite` | | `action?` (`list`, `save`, `load`, `delete`), `kind?` (`material` or `family`), `name?`, `as?`, `replace?`, `path?`: the favorites kept for every project in a file of the user's own; a material is kept with its picture, a family with the materials it's made of |
| `flip` | | `ids`: turns walls round about their location lines, their exterior side to the other side; their doors and windows stay where they are |
| `modify` | | `id` (or `ids`, several in one step) and the fields to change; into a field that is an object, such as a level's `view_range` (`top`, `cut`, `bottom`, `depth`), they merge |
| `move` | MV | `ids`, `by`; `end` to move one end of a wall |
| `copy` | CO | `ids`, `by` |
| `rotate` | RO | `ids`, `about`, `angle` in degrees, `copy?` |
| `mirror` | MM, DM | `ids`, `axis` (two points), `copy?` |
| `corner` | TR | two wall `ids`, `keep?` (a point on the part of each to keep) |
| `split` | SL | `id`, `at` |
| `delete` | DE | `ids` |
| `undo`, `redo` | | |
| `inspect`, `elements`, `render` | | read the model |
| `new`, `open`, `save` | | projects; `new` takes `name?` and `language?`; `open` imports IFC, listing what it left out; `save` exports by extension |

A `level` can be given by name or id; without one, commands use the lowest level. `render {view}` draws a plan (with
`ceiling: true`, its reflected ceiling plan), a section or an elevation by name, a sheet by number (a path ending .pdf prints it), or with `{3D}` (and `azimuth?`, `elevation?`) the 3D
view. `save` to a .pdf prints every sheet, a page each.

## Architecture

```text
crates/core            bimcraft-core: everything but the front ends; no UI
  geom.rs              points, rings, and the planar faces rooms are found from
  model.rs             the building model, and the rules checked after every command
  engine.rs            the session: commands, undo and redo, scripts
  commands.rs          the command set
  render.rs, font.rs   plans, reflected ceiling plans, sections and elevations as display lists drawn to SVG, PNG or a
                       canvas, cropped to their crop regions, in our own stroke lettering and materials' patterns
  view3d.rs            the 3D view: the model as solids, drawn with a depth buffer, shaded and edged; picking
  sheet.rs, pdf.rs     sheets: the title block, the views on them at their scales, schedules as tables, notes; PDF
  snap.rs              snapping: where a picked point lands (ends, middles, crossings, perpendiculars, angle guides) and the
                       ruler's numbers, so every front end agrees
  schedule.rs, ifc.rs  schedules: fields, calculated ones, filters, sorting, grouping and totals, and CSV; IFC4 export
  walls.rs             walls' geometry: arcs, faces and layers by location line, joins at any angle, attachment; holes
                       cut in plan
  stairs.rs            stairs' and ramps' runs and landings, their walk lines, and the lines railings follow
  family.rs            families: parameters, formulas, types and forms swept or turned into solids, voids cut from them; doors and
                       windows hosted in walls; the library a project starts with
  csg.rs               a solid cut from a solid, for voids: binary space partitions, bounded in time and depth
  texture.rs, paint.rs textures (patterns drawn in code, PNG and JPEG pictures, laid at their real size), and paint on faces
  favorites.rs         the user's favorites: materials and families kept for every project
  graphics.rs          visibility and graphics: what a view hides and how it draws the rest, by category, element and
                       view filter; view templates; the look the drawing code asks
  phasing.rs           phases and design options: an element's status in a view's phase, phase filters, the options a view
                       shows, and the rules that keep both sound
  units.rs             project units: lengths and areas as drawings and the desktop show them, typed lengths read
  language.rs          a project's language: the words its drawings print, the names commands give
  step.rs, import.rs   ISO 10303-21 files read; IFC import, every element measured from its geometry
crates/update          bimcraft-update: the updater, which only the desktop app uses; the core, the CLI and MCP never depend on it
  lib.rs               a release read from GitHub's reply, its file for this kind of install chosen and downloaded into a folder of its own, checked
  net.rs               HTTPS to GitHub and its release CDN, redirects followed by hand
  prefs.rs             update.json, beside the favorites in the user's own configuration folder
  detect.rs            which kind of install this is (the Windows installer's copy, an AppImage, the macOS app) and what replaces it
  install.rs           the Windows hand-over: a copy of the app waits for it to exit, runs the installer, starts the app again
  appimage.rs          an AppImage replaced by the new file once the new file has run
  macos.rs             the macOS app replaced from the disk image: mounted, copied, verified, run, swapped in
  exec.rs              the programs an update runs, under a time limit, and the check of a new build's version
  updater.rs           the state the banner draws (idle, offered, downloading, ready, installing), worked on threads of their own
crates/ui-egui         bimcraft-ui-egui: the desktop front end, a thin shell over the session
  lib.rs               the window: Quick Access Toolbar, ribbon tabs, Options Bar, browser, view tabs, status bar, files
  canvas.rs            the plan: pan, zoom, the navigation bar, picking, the tools and their previews
  orbit.rs             the 3D view: orbit, pan, zoom, picking
  section.rs           sections and elevations: their picture and drawing, pan, zoom, picking
  paper.rs             sheets: their drawing at paper density, scaled as they zoom; views picked, dragged, opened
  properties.rs        Properties: the Type Selector, parameters through `modify`, Type Properties through `type`, the Materials window
  editor.rs            the Family Editor: a family's parameters, types, forms and symbols as text, a preview, `family` commands
  favorites.rs         the Favorites window and the swatches of textures
  graphics.rs          Visibility/Graphics, Filters and View Templates, Hide in View
  phasing.rs           Phasing, Design Options, the active option and the Phasing groups of Properties
  lang.rs              the desktop's words in Portuguese and Spanish, and the language it starts in
  style.rs             a light and a dark palette, spacing, and the icons, drawn in code
  updates.rs           the update banner under the Options Bar, and the File menu's update items
apps/bimcraft          the desktop app
apps/bimcraft-cli      run, info, convert, commands, and the MCP server
```

The rules come from CADCraft. Everything is a command, and the desktop, the CLI, scripts and MCP all reach the same ones.
Production code never panics: `unwrap`, `expect` and `panic!` are lint errors, `unsafe` is forbidden, every parameter is
treated as hostile, and a command that fails or panics leaves the model as it was. The core knows nothing of a UI; the egui
front end only reads the model and runs commands, so another front end can replace it. The canvas draws the same display list
the PNG and SVG exports do, and picks elements from it. Agent instructions are in [CLAUDE.md](CLAUDE.md).

## Roadmap

Done: views and notes (view ranges, crop regions, text notes, spot elevations, angular and radial dimensions and dimensions to
wall ends and openings, schedules and the 3D view on sheets), project units (metric, or feet and inches), Portuguese and
Spanish, for the desktop and for projects, real walls (compound types with layers, joins at any angle, curved walls,
walls attached to roofs and floors, other location lines), sloped floors with openings, ceilings with openings and reflected ceiling plans, and shaft
openings, stairs of several runs with landings, ramps and railings, and roofs over any footprint (hips and valleys, gable
ends, courtyards), extrusion roofs and dormers, materials, with floors, roofs and ceilings built of layers as walls
are, families, BIMCraft's own parametric components (never Autodesk's `.rfa`), and project parameters and schedules with
chosen fields, filters, sorting and formulas, and visibility and graphics (overrides, view filters, view templates, phases and design options, which go out to IFC and come back), and families done: revolves and voids, doors and windows as families, the Family Editor, textures by material or from pictures you add, Paint, and favorites kept for every project, and updates (a check at startup, and the Windows installer's copy, an AppImage and the macOS app upgrading themselves).
Next, in order: locked and equal dimensions, arrays and groups; fill patterns of your own, perspective views and a section box; DWG and DXF, more of
IFC, and gbXML; structure, then MEP; site and massing; several people editing one model; and signed installers (macOS
signing and notarization are wired up, waiting on a Developer ID). Revit's own `.rvt` format is closed and undocumented, so BIMCraft won't read it itself;
IFC is the way in and out, and `.rvt` files will come in through Autodesk's own converter.

## Downloads

### Windows

| Build | Installer |
|---|---|
| x64 (64-bit Intel/AMD) | `bimcraft-<ver>-windows-x64.msi` |

The installer is a short setup wizard. It puts BIMCraft and `bimcraft-cli` in Program Files for every user, adds BIMCraft
to the Start menu, asks whether to put it on the desktop too, opens `.bimcraft` projects in it, and offers it under Open
with for `.ifc` files. It needs no Visual C++ runtime, and a newer version upgrades an older one, keeping the desktop
shortcut as it was. For a silent install with a desktop shortcut: `msiexec /i bimcraft-<ver>-windows-x64.msi /qn
DESKTOPSHORTCUT=1`. It isn't code-signed yet, so Windows SmartScreen asks before it runs: More info, then Run anyway.

### Linux

| Format | x86_64 | aarch64 (64-bit Arm) | Notes |
|---|---|---|---|
| AppImage | `bimcraft-<ver>-linux-x86_64.AppImage` | `bimcraft-<ver>-linux-aarch64.AppImage` | Runs anywhere; BIMCraft replaces it with a newer one itself (see [Updates](#updates)), or [AppImageUpdate](https://github.com/AppImageCommunity/AppImageUpdate) does, from the `.zsync` files |
| Flatpak | `bimcraft-<ver>-linux-x86_64.flatpak` | `bimcraft-<ver>-linux-aarch64.flatpak` | Sandboxed; `flatpak install --user <file>` |
| Debian, Ubuntu | `bimcraft-<ver>-linux-x86_64.deb` | `bimcraft-<ver>-linux-aarch64.deb` | `sudo apt install ./<file>` |
| Fedora, RHEL, openSUSE | `bimcraft-<ver>-linux-x86_64.rpm` | `bimcraft-<ver>-linux-aarch64.rpm` | `sudo dnf install ./<file>` |
| Arch | `bimcraft-<ver>-linux-x86_64.pkg.tar.zst` | `bimcraft-<ver>-linux-aarch64.pkg.tar.zst` | `sudo pacman -U <file>` |
| Tarball | `bimcraft-<ver>-linux-x86_64.tar.gz` | `bimcraft-<ver>-linux-aarch64.tar.gz` | Unpack anywhere |

RISC-V (riscv64): `bimcraft-<ver>-linux-riscv64.deb`, `.rpm` and `.tar.gz`, cross-compiled; they need glibc 2.39 or later
(Ubuntu 24.04, Debian 13).

The others run on any distro with glibc 2.35 or later (Ubuntu 22.04, Debian 12, Fedora 36 and after). The AppImage runs
as it is, with nothing installed and no FUSE needed (`chmod +x`, then run it). BIMCraft looks for a newer one itself and
replaces the file, and AppImageUpdate or AppImageLauncher do the same from the `.zsync` next to it, downloading only what
changed. The Flatpak runs in a sandbox that reaches the display, the GPU and your Documents folder, and opens any other file
through the file chooser; it has no network, so it never looks for updates (install the newer `.flatpak` the same way). The `.deb`, the
`.rpm` and the Arch package put `bimcraft` and `bimcraft-cli` on the path, add BIMCraft to the applications menu and open
`.bimcraft` projects and `.ifc` files in it. The tarball holds the same tree (`bin/`, `share/`).

### macOS

| Build | Download |
|---|---|
| Universal (Apple silicon and Intel), macOS 11 or later | `bimcraft-<ver>-macos-universal.dmg` |
| The CLI alone | `bimcraft-cli-<ver>-macos-universal.zip` |

Open the DMG and drag BIMCraft to Applications. It opens `.bimcraft` projects from Finder and offers itself under Open
With for `.ifc` files. Until releases are signed with a Developer ID, macOS asks before it opens the app the first time:
Control-click it, then Open (on macOS 15, System Settings > Privacy & Security > Open Anyway). BIMCraft looks for a newer
version itself and replaces the app in place (see [Updates](#updates)).

Versions are published on the [releases page](https://github.com/IPedrax/bimcraft/releases) as they're tagged, with a
`SHA256SUMS` of every file. Between releases, the Release workflow (Actions > Release > Run workflow) builds them all
for any commit, as that run's artifacts. The scripts behind them are in `packaging/` (`linux/package.sh`,
`linux/flatpak-bundle.sh`, `linux/arch-package.sh`, `macos/package.sh`, each with its `--help`), and run on your own
machine too.

### Updates

The desktop app looks for a newer release when it starts: one HTTPS request to `api.github.com` for this repository's latest
release, made on a thread of its own with a 15 second timeout, so the window never waits for it. Being offline, being rate
limited, or getting a reply that isn't a release is ignored without a word. It sends nothing of yours beyond what any HTTPS
request carries: your address, and a `User-Agent` naming BIMCraft's version (it follows `HTTPS_PROXY` and `ALL_PROXY` like
other tools do). A release counts when it's published (not a draft, not a pre-release) and its tag, `vMAJOR.MINOR.PATCH`, is
a later version than the one running, as semver orders them. A banner then offers it: Update now, Later (it asks again next
time), or Skip this version (it stays quiet until a newer one comes). File > Check for Updates Now looks by hand, and says
when there's nothing newer or the check failed.

The check is switched off in File > Check for updates at startup; that choice is kept in `update.json` beside your
favorites, in your own configuration folder (`%APPDATA%\BIMCraft`, `~/Library/Application Support/BIMCraft`, or
`$XDG_CONFIG_HOME/bimcraft`, else `~/.config/bimcraft`; `BIMCRAFT_FAVORITES` moves both), not in a project. Setting the environment variable `BIMCRAFT_NO_UPDATE_CHECK` (to anything but empty)
keeps the app from making any request, and hides the menu items. `bimcraft-cli` and the MCP server never check for anything.

**Windows.** Update now works for the copy the installer put in `Program Files\BIMCraft`. It downloads
`bimcraft-<ver>-windows-x64.msi` from the release into a new folder in your temp folder and checks it. Restart and install
then asks to save any unsaved changes, closes BIMCraft, and hands over to a copy of itself that waits for BIMCraft to be
gone, checks the file once more, runs Windows' own `msiexec` on it while holding the file open, so that nothing can change it
(`/passive`: a progress bar, and Windows asks permission, because the installer is for every user) and starts BIMCraft again
with the project that was open. If you cancel the permission prompt the old version starts again, and the offer comes back at
the next start unless you skipped it. The installer's log is `install.log` in that folder, which BIMCraft removes at a start
a day or more later.

**Linux.** Update now works for an AppImage whose folder you can write to, on x86_64 or aarch64 (if you start it through a
link, the file the link points to is the one replaced). It downloads `bimcraft-<ver>-linux-<arch>.AppImage` into a new folder
in your temp folder and checks it. Restart and install then asks to save, checks the file once more, and copies it next to your
AppImage under a hidden name, on the same disk, so that the last step can't be left half done. It runs that copy with
`--version`, as you would run it and, if that fails (no FUSE, say), once more asking it to unpack itself first, which needs
none, giving each try at most a minute; the copy must print `bimcraft <version>` for the release's version. Only then is it
renamed over your AppImage, and the new one starts (unpacking itself, if it had to for the check) with the project that was
open as BIMCraft closes. The file keeps its name, so a shortcut to it keeps working, even when the name holds the old version.
If the new build doesn't run, or says another version, your AppImage is as it was, the hidden copy is deleted and a note says
why. A package from your distribution (`.deb`, `.rpm`, Arch) is files that belong to root and to your package manager, so it is
told of the newer release and not replaced; so are the tarball, a build from source, and an AppImage in a folder you can't
write to. A Flatpak has no network and never checks. None of this has run against a real AppImage: it is tested with scripts
that stand in for one, which is what the checks on the new file and the swap need, but not the AppImage runtime itself.

**macOS.** Update now works for BIMCraft.app when its folder is one you can write to (Applications, for an administrator, or
`~/Applications`). It downloads `bimcraft-<ver>-macos-universal.dmg` into a new folder in your temp folder and checks it.
Restart and install then asks to save, checks the file once more, mounts the disk image read-only and out of Finder's sight,
copies BIMCraft.app out of it with `ditto` to a hidden app beside yours, lets the image go, and runs `codesign --verify
--strict --deep` on the copy, as the release script does. It runs the copy once with `--version`, which must print `bimcraft
<version>`, and swaps it in by two renames: the old app is put back if the second fails, and deleted once the new one is in.
The new app starts with the project that was open as BIMCraft closes. If a step before the swap fails, your app is as it was, and
a note says why; if the new app is in but won't start, the note says that, and BIMCraft stays open. **This has not run on a Mac.** The steps are tested only against shell scripts that stand in for `hdiutil`, `ditto`,
`codesign` and `open`, so three things stay open until someone tries it. Whether macOS asks permission, or refuses, when an
app signed ad hoc replaces its own bundle (macOS 13 and later guard apps against changes by others; if it refuses, the update
fails with that error and the old app stays). Whether Gatekeeper asks again about the new app: it is signed ad hoc like the
old one, and the copy BIMCraft makes carries no mark from a download. And whether `open` hands the project to the new app: Finder
hands projects over another way, which the app also reads, and if `open` doesn't do it the new version starts with a blank
project, while the project is still in the file you saved. An app run from the disk image itself, or from the folder it was
downloaded to, can't be replaced (macOS runs a downloaded app from a read-only copy until it is moved): it is told of the newer
version and sent to the page, so move BIMCraft to Applications first.

**Everything else.** A build from source, a copy kept anywhere else, and a Windows copy running as an administrator are told
of a newer release, and the banner's Download button opens its page. Replacing a program that came from `cargo` or a package
manager is not the updater's to do, and the copy that runs the Windows installer would have an administrator's rights, in a
folder other programs can write to. Run BIMCraft as yourself.

**What the check protects against.** The update is fetched only from this repository's release on github.com and GitHub's
release CDN, over HTTPS: the address is built from the release's tag, never taken from the reply, redirects are followed one
by one and only into this repository's releases on github.com or to the CDN, and an asset is taken only if it's named exactly
for that version and kind of install. The reply is read up to 1 MiB, and the download is held to the size GitHub lists for the
file (at most 128 MiB) and must match it to the byte. Its SHA-256 must equal the digest GitHub's API gives for that asset, when
it arrives and again just before it is installed; and where the release lists a `SHA256SUMS` file, the sum in it must be the
same one. The two come from different places (the release workflow writes the file, GitHub works out the digest when an asset is
uploaded), so a file replaced afterwards without the other being updated is refused. Nothing runs that failed a check, and the
file is deleted. A damaged or truncated download, a cache or a mirror serving something else, and a server that sends more than
it should are all stopped by this. On Linux and macOS the new build must also run and say its version before it takes the old
one's place, which shows that it loads here, though not that its window opens.

**What it doesn't.** None of the three files is signed by a developer identity that BIMCraft checks: the Windows installer isn't
code-signed (so Windows can't say who made it), the macOS app is signed ad hoc (which says that it is intact, not who made it),
and the AppImage isn't signed. The digest says only that the file is the one GitHub lists: someone who could publish a release
in this repository (a stolen token or account), or GitHub itself, would publish a digest that matches their file. On Linux and
macOS the new build is run once, as you, with `--version` before it replaces the old one; that is no more trust than starting it
after, and the digest check comes first, but it is a program from the internet that runs because you clicked. A program already
running as you could swap the file in your temp folder after the first check, and the second check, made just before the install,
catches that. On Linux and macOS nothing holds the file between that check and its use, which is a matter of moments, and a
program that could win that race could replace your AppImage or your app directly. On Windows the copy that runs the installer
holds the file open so that nothing can write to it until Windows Installer is done. If Windows Installer won't open a file held
that way, the copy lets go, checks it once more and runs it unheld, and then a swap is possible for as long as you take over the
permission prompt, which looks the same either way, since it can't say whose file it is. The updater doesn't mark the file as
downloaded from the internet, so SmartScreen doesn't ask as it does for one saved by a browser; the permission prompt still
comes from Windows. The certificates it trusts are Mozilla's, built in, so a network that inspects HTTPS with a certificate of
its own makes the check fail, quietly, and a corporate root isn't consulted. Signed installers are on the roadmap.

## License and credits

BIMCraft is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Copyright (c) 2026 IPedrax and the BIMCraft contributors.

BIMCraft's logo, its icons, its stroke lettering and its sample house are original work, drawn or built in code. Its
architecture, its rules and its MCP design follow [CADCraft](https://github.com/storytold/cadcraft) (MIT OR Apache-2.0) by
the ArtCraft team, as its logo follows CADCraft's style.

<sub>Autodesk and Revit are trademarks or registered trademarks of Autodesk, Inc. in the United States and/or other countries. BIMCraft is an independent, open-source project and is not affiliated with, sponsored by or endorsed by Autodesk, Inc.; these names are used only to describe the workflow it follows. IFC is an open standard of buildingSMART International, published as ISO 16739.</sub>

<p align="center">
  <img alt="" src="assets/app-icon/bimcraft-64.png" width="28"><br>
  <sub>Made by IPedrax, in pure Rust.</sub>
</p>
