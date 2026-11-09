# Parameters

Manage > Project Parameters is the one place for everything about parameters: project and shared parameters in one table,
their categories, new parameters for families in bulk, transfer between families, dictionaries, import and export. The core is `crates/core/src/parameters.rs` and
`parameters/table.rs`; the window is `crates/ui-egui/src/parameters.rs`. Values on elements, schedules and tags: [schedules](schedules.md).

## Identity

Every project parameter has a library GUID (`Kind::Parameter`), whatever it is called. There is no separate shared kind: any
parameter can be scheduled and tagged, and "shared" only says it came from a dictionary (`ProjectParameter::dictionary`, a
`Kind::Dictionary` entry, by GUID in a file). `parameter {name, dictionary: null}` makes it a project parameter again, its GUID
kept. A GUID is made once: there is no action that gives a parameter a new one. Duplicate makes a parameter of its own instead.
`same_as` holds other parameters' GUIDs it was said to be, or merged from: a file holding one of those means this parameter.

## Instance and type

A parameter's `scope` says whose value it is. An instance parameter's value is each element's (`params`, set with `modify`). A type
parameter's is each type's, read by every element of the type: `ProjectParameter::type_values`, keyed by the GUID of the type's library
entry, so a rename can't lose it, and set with `type_parameter`. `parameters::value_of` is the one read: schedules, tags, view filters
and IFC go through it (IFC writes a type's value on each element, in `Project Parameters`). A `sync_library` drops the values of a type
that's gone. `modify` refuses a value on an element for a type parameter. Rooms, stairs, ramps and masses have no types (`TYPELESS`), so
a parameter for them can't be a type's.

Switching: instance to type wipes the elements' values; type to instance copies each type's value onto its elements (the owner's
decision, 2026-11-03). The window says which before Apply, and undo brings them back. Values are edited in the Type Properties of a type
(`type_value_rows`, for the types with such a dialog); Properties shows only instance parameters. A type duplicated in that dialog keeps
its values (`copy_to`).

## Dictionaries

A dictionary is a plain JSON file (`.bimparams`: `format`, `guid`, `name`, `version`, `revision`, `changed`, `parameters`) of definitions. BIMCraft
reads it from a local path and never fetches one (the network rule in the root `CLAUDE.md`), so a team keeps it on a shared drive
or in a Git repository, whose history and review are the governance a parameter list needs. A project embeds the definitions it
uses and records where each dictionary was and at which version (`Model::dictionaries`), so it opens without the file, and a newer
file changes nothing until it's imported: Dictionaries > Check shows what it would change, as a dry run. The `dictionary` command
edits a file; the project isn't touched. `revision` counts up with each real change to the file and `changed` dates it; a project keeps the
revision it took (`DictionaryRef::revision`), which is how it can say it's behind. `version` is only a label of the team's own: nothing
compares it, and what would change is always worked out from the definitions. A definition holds a default `scope`. A file of a newer `format` is refused.

A project keeps the dictionaries it has loaded (`dictionary_mount`, `Model::dictionaries`), any number, in its own file: they are there when the
window opens again and when the project does. Loading takes nothing in. A moved file is the same dictionary by its GUID and takes the new path, a
different one of a loaded name is refused, and one is taken off only when no parameter comes from it. A file that has gone says so, and Open
Dictionary finds it again. The window's dropdown says which is in use; the rest of the tab and the Families tab's Add Parameter read the files
(`ParameterManager::loaded`) when the tab opens.

## Import and export

`parameter_import` reads, plans (`parameters::plan`) and only then applies, so a dry run and an import share their code. A row is
matched by GUID first, then for a table with no GUIDs by name. A row of a name taken by another GUID is a `conflict` until `resolve`
says `same` (remembered in `same_as`), `separate` (comes in as `name 2`, keeping its GUID) or `skip`. A file can add categories, and
takes them off only with `replace_categories`, since that loses values. A change of kind that would strand values is a conflict.
`scopes` picks instance or type by the file's name, over what the file says; where neither says, a parameter here keeps its own and a new one
is an instance's, so a table with no such column changes nothing. A change of scope is a `modify` whose notes say what happens to the values.
Names are made nameable as an IFC import's are (`import::parameter_name`), and the row says so.

Categories are matched by our names, then Revit's (`match_category`, `OST_` names too). `map` settles the rest and is kept in
`Model::parameter_map`, so it is asked once. Revit categories with no category here land on `component`, all components: `broad`
says so. More categories to come only shorten that list.

Tables are `.xlsx` (`xlsx::read`: any workbook's first sheet) and `.csv` (comma or semicolon); columns by heading, in any order:
GUID, Name, Kind, Categories (split by `;`), Group, Description, Instance/Type, User Modifiable. Export writes `.xlsx`,
`.csv` and `.bimparams`. Definitions only: no element's values go in or out.

### Revit's shared parameter files

Read, never written (`parameter_export` refuses `.txt`; the owner's decision, 2026-11-03). The layout comes from Autodesk's public Revit
API documentation, "Definition File", so it isn't under the `.rvt` fence: tab-separated blocks, each headed by a `*` line naming its
columns, and columns are found by those names. UTF-8 and UTF-16 by the file's mark. GUIDs are kept. The file names no categories, so
the import asks for them, and instance or type. `VISIBLE` and `HIDEWHENNOVALUE` aren't read: nothing here hides a parameter, since a value
Properties doesn't show could only be set by a script (the owner's decision, 2026-11-03). A `DATATYPE` with no kind here (`FORCE`…) comes in as a number and is listed; a family type, a material or an
image is left out and listed. The file itself is never copied into a project.

## Between projects

Insert from File (`insert.rs`, `bring_parameters`) goes by GUID first, so a parameter renamed in either copy is still itself and
its values follow. One of a name taken here by another GUID comes in apart as `name 2` rather than being merged by accident, unless
`same_parameters` names it. Only what the inserted elements use comes along (a whole project brings all). A parameter takes on the
categories what comes in holds it for, and a value it can't hold (each project keeps its copy of a definition) is left off and listed.

## Batch, merge, purge

`parameter_batch` runs set, delete, duplicate and rename together, all or none and one undo step; `dry_run` restores the model after
checking it. `parameter_merge` moves values, schedule fields, filters, tags and calculated fields, refuses different kinds, stops on
values that differ unless `overwrite`, and adds the old GUID to `same_as`. `parameter_list` says what uses each (`usage`), which is what
`parameter_purge` goes by. A rename reaches tags and calculated fields too; `rename_in_formula` takes a name whole, spaces included.

`read_only` (the window's Editable), `group` and `description` are how Properties and Type Properties show a parameter; a command can
still set a read-only one. Area parameters are m², volumes m³, angles degrees.

`family_parameter_transfer` copies a family's parameters to others, with the parameters their formulas read; a name a family
has is skipped and listed.

## Families in bulk

`family_parameter_add` gives many families new parameters at once, all or none (`Model::check` goes over each family). A type parameter's
`value` is the family's own default, so it's every type's until a type sets its own in Type Properties. A family parameter may be `text`
(`family::Kind::Text`), which takes no formula and which a formula can't read; `group` puts it under a group of its own in Properties.

`definition` ties a family's parameter to a dictionary's or a project parameter's GUID, so the two are one parameter: a project parameter may
share its name with a family's only if tied (`family_clash`), a schedule has one column for it, and the element reads the family's value
(`schedule.rs` looks at the family first). A tied project parameter can't be renamed: there's no way yet to rename a family's parameter with
its forms' formulas, and two names would split the column. An integer, an area and a volume go in a family as numbers (`family_kind`).

## Project Information

As Revit's: an element of its own (`Element::ProjectInfo`, category `project`, `project_info.rs`), one in a project at most, with an id and a
GUID. The first `project_info` that sets something makes it; a read makes nothing and costs no undo step. It holds the fields title blocks
and IFC name a project by (`FIELDS`: number, client, address, status, issue date, author, organization and its description, building), all
text and one line but the address, and the values of the project parameters for `project` (a category like any other: a rename, a kind
change and taking the category off reach its values as they reach an element's; it has no type, so `TYPELESS`). The project's name stays
`Model::name`, which `project_info` sets too. It can't be deleted or copied (`deletable`, and `Model::check` refuses a second); emptied, it
says nothing. Revit's `OST_ProjectInformation` is ours by name (`NAMED_CATEGORIES`).

A sheet's title box letters the client, then the number, status and issue date, right of the project's name (`sheet::title_block`). IFC
writes the status as `IfcProject.Phase`, the building's name and an `IfcPostalAddress` (its lines) on `IfcBuilding`, every field as typed in
`BIMCraft_ProjectInformation` on the project, and the project parameters in `Project Parameters` on the project, as any element's; the
import reads them back, and from another program's file the phase, the building's name and its address (lines, postal code and town,
region, country). Manage > Project Information (`crates/ui-egui/src/projectinfo.rs`) is the dialog: OK sends what changed in one
`project_info`.

## The window

Edits are staged in `ParameterManager::rows` and go in one `parameter_batch` on Apply; the footer counts them. Import is a dry
run shown as a table with the mapping, instance or type, and decisions, run again after each choice. A dictionary's table is staged too, and
Save Dictionary writes it as one revision; Update Project is an import of the definitions the project has, with `replace_categories` on, so a
category taken off the dictionary comes off the project after the preview says so. The Families tab's table is staged until its Apply.
Text in a table's narrow column goes through `wrapped`, which knows its height first: a cell that grew later would run over the next row.

## Not yet

- A value for each type in the tools that have no Type Properties yet; the Families tab's Modify (existing parameters are changed in the Family
  Editor) and Discipline, which has no meaning here yet.
- Renaming a project parameter a family is tied to.
- Nested family parameters: they wait for family nesting ([ROADMAP](../../ROADMAP.md)).
- Dragging categories between parameters: Copy Categories From does that job.
- The definition's GUID through IFC: a project parameter's name goes out in its property set, its GUID doesn't, so one that has
  been through IFC is a new parameter. Requirements from IDS and URIs to bSDD build on that.
- Schedules that span linked models by GUID.
- Sheet parameters (Revit's Sheets category) and title-block labels a user places: the title box's layout is fixed.
