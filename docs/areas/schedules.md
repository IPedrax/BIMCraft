# Project parameters and schedules

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.
Managing parameters as a set (dictionaries, import and export, batch changes): [parameters](parameters.md).

(A distribution board's panel schedule is not one of these: it is a view of its own, `panel_schedule.rs`, in [structure and MEP](structure-and-mep.md).)

A project parameter (`Model::parameters`) is a value each element of its categories may hold in its `params`, of its kind;
a component's `params` hold its family's instance values too, so a parameter for components can't take a family
parameter's name, nor any category's own field's (`schedule::own_field`). A schedule's fields are its category's own
(`OWN` in `schedule.rs`, read from `element_json`), the project parameters for it and, for components, the families'
parameters, or calculated: a formula over the row's others, read as the family formula engine reads (lengths in mm, areas in
m²), a field with no value leaving it with none (an unset yes or no is no). Filters compare numbers as the schedule shows
them, so two that show the same are equal. The seven schedules every project has (`schedule::VIEWS`) aren't elements
until changed or placed (`schedule::standard`), and a schedule named as one of them, in any language or case, is that one; renaming a parameter renames it in schedules and in the formulas naming it. The last of them is the sheet list (`of: "sheet"`), the project's sheets by their numbers, a category
no view draws.
A component's Room Number and Room Name (as Revit's Room: Number and Room: Name) are the room it is in, found where it is
read and never stored (`Rooms::component`, the one rule `elements`, schedules, `load_points` and IFC share): the room on its
level whose boundary at the walls' centre lines holds its family's room calculation point (else its insertion point) in plan,
and whose height holds it, so an outlet on a wall's face is in the room on that side whatever the project measures rooms at
([families](families.md), `room_calculation_point`). `element_json` gives it as `room` (the room's GUID), `room_number` and
`room_name`; a component outside every room, or over or under it, has none.
A multi-category schedule (`of: "multicategory"`, `schedule::MULTI`, as Revit's Multi-Category Schedule) has a row for each
element of the categories it lists (`Schedule::categories`; none, every built one, `schedule::built`: not rooms, levels,
masses, mass floors, framing members and sheets) and the fields they share (`SHARED`): Category (the element's, as elements name theirs), Family, Type, Level, Count,
Width, Height, Length, Thickness and Area, each its own category's field of that name where it has one holding the same and
empty where not (an area worked out for walls, length by height, and doors and windows, width by height; a roof's plan
area), and Material, its type's structure layer's (or its thickest's), a run's type's or a family's first material
parameter's. Its filters, sorting, grouping, totals and calculated fields are any schedule's; only it lists categories.
Two categories have rows that are no elements, made in `schedule::sources`: `massfloor`, a mass at each level it crosses
([masses](massing.md)), and `framingmember` (`schedule {category: framingmembers}`, Framing Members), a row for each member of
each framed wall, floor and roof, worked out as it is read and pointing at its host: Host, Host Type, Framing Type, Role, Section (one ply as
sawn, `38 x 89`; plies before it, `2 x 38 x 184`), Length (the cut length to the mm), Stock Length (0 where it is longer than
every stock length of its framing type), Level, Key and Count. Grouped by Role, Section and Length with Count, it is the cut list
([structure and MEP](structure-and-mep.md#wood-framing)).
A table goes to a file as CSV (`schedule::to_csv`, text a spreadsheet would read as a formula guarded with `'`) or, for a path
ending `.xlsx`, as a workbook of one sheet (`schedule::to_xlsx` over `xlsx.rs`, which writes the parts and the zip by hand on
`flate2`): headings in bold, numbers as numbers so they sum, text as inline strings, which a spreadsheet never takes for a formula.

## The same numbers as Revit

`crosscheck.rs` holds the numbers schedules show against Revit's ([tools/revit-crosscheck](../../tools/revit-crosscheck/README.md)):
`export_crosscheck` describes a project in plain numbers (walls by their centre lines, floors, shafts, doors and windows,
rooms by their points), a script of ours builds it again in Revit and writes what Revit reports, `kind,id,quantity,value`
by our element ids, and `compare_crosscheck` (and the test `the_small_tower_has_the_numbers_revit_gives_it`, once Revit's
table of the small two-storey tower is committed) compares them quantity by quantity within `crosscheck::tolerance`.
Compared: each room's area, perimeter, whether it is enclosed, its unbounded height and its volume, each wall's length
along its centre line and its doors and windows, each floor's area less its openings and shafts, and its volume. Revit
reports more (a wall's area and volume) than we work out yet: those come back as `only_theirs`, the list of what is still
to do. Where the two define a quantity differently, the difference goes here, with the rule the comparison applies.

- A room's volume is its area times its unbounded height (from its level and base offset up to its upper limit and limit
  offset), which is Revit's while nothing under its top bounds it. Revit also stops a room's volume at a room-bounding
  ceiling, floor or roof lower than its top, and a ceiling bounds rooms unless told not to; we don't yet. The description
  gives Revit each room's limits as ours. The tower's rooms reach 300 mm under the level above (3.7 m, the `room`
  command's default), over their ceilings at 2.7 m: Revit's volume of them is their area times 2.7 m where ours is times
  3.7 m, the one difference the comparison is expected to show until volumes stop at ceilings.
- Each level's computation height (where walls and columns are cut to bound rooms) goes to Revit as the level's.

## Schedules of bars

`schedule {template: "steel_list" | "steel_summary"}` fills a schedule of category `rebar` with the fields, filters, sort and grouping of a steel list or a steel summary where the call gives none, so every other parameter of the command still applies. The rule for marks, and the commands `steel_list` and `cut_plan`, are in [quantities](quantities.md).

`schedule {template: "materials_summary", level?}` makes a schedule of the six categories that have quantities (beam, column, structural column, footing, strip footing, floor), grouped by level and category, with the count, Volume, Formwork Area and Rebar Weight of each group totalled. It cannot split by a project parameter, because a schedule of several categories has none among its fields: `quantities` with `by` does. The same four fields (Rebar Ratio too) are on each of those categories' own schedules, appended after the default columns, and a multi-category schedule reads each category's own. A ratio is not a grouped column, because a total of ratios would add them: `quantities` gives it.

## Editable ODS type catalogue

Use `catalogue_export {"path":"types.ods"}` to write conduit, cable tray, wire
and cable geometry types. Edit the first sheet and use
`catalogue_import {"path":"types.ods"}` to apply it in one undo step. Keep the
headings and each existing type's GUID, category and name. Leave the GUID blank
for a new type. Width and depth are millimetres, Round is true or false, and
Material names a material in the project. The entire import rolls back if a
row fails validation.

This exchange is explicit. The project saves no workbook path and opening it
never reads or writes a workbook. Use regular .ods files. Both commands are
refused in read-only sessions and dry runs. The workbook is limited to 32 MiB
and 20,000 rows, including the headings.

## Fill an ODT template

Use `odt_template {"template":"cover.odt","path":"project-cover.odt","fields":{"ProjectName":"Library"}}`
from the CLI, a script, or MCP. The command copies a Writer document and replaces
the contents of named `text:variable-set` elements. Each requested variable must
occur once. Formula fields keep their formulas. Other document entries, including
styles and images, are copied. The source and project stay intact.

Use a regular .odt file for the source and a different .odt path for the copy.
The command validates the fields before replacing the destination atomically.
It accepts up to 256 fields, 256 archive entries, 32 MiB of content XML, and
256 MiB per archive. Co-Work guest sessions and dry runs refuse the export.

### Copy a linked Writer document set

Use `odt_set {"templates":["forms/master.odm","forms/cover.odt","info.odt"],"path":"issue","fields":{"ProjectName":"Library"}}`
to copy a master document and its linked documents into one directory. Supply all
members explicitly, with unique file names, and choose a new directory whose parent
already exists. Linked sections must refer to members of the set. A missing relative
path can be recovered by matching its file name to a member. Section names are checked
before publication and links in the copies point to the copied files.

Fields are replaced wherever they occur. A field may be absent from individual
documents, and an empty field map copies the set and updates its links. Existing
destinations are refused and the source documents stay intact. The set accepts up to
32 documents, 256 MiB of archives in each direction, and 128 MiB of content XML.
Co-Work guest sessions and dry runs refuse the export.
