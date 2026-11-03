# Parameters

Manage > Project Parameters is the one place for everything about parameters: project and shared parameters in one table,
their categories, the families' parameters, dictionaries, import and export. The core is `crates/core/src/parameters.rs` and
`parameters/table.rs`; the window is `crates/ui-egui/src/parameters.rs`. Values on elements, schedules and tags: [schedules](schedules.md).

## Identity

Every project parameter has a library GUID (`Kind::Parameter`), whatever it is called. There is no separate shared kind: any
parameter can be scheduled and tagged, and "shared" only says it came from a dictionary (`ProjectParameter::dictionary`, a
`Kind::Dictionary` entry, by GUID in a file). `parameter {name, dictionary: null}` makes it a project parameter again, its GUID
kept. A GUID is made once: there is no action that gives a parameter a new one. Duplicate makes a parameter of its own instead.
`same_as` holds other parameters' GUIDs it was said to be, or merged from: a file holding one of those means this parameter.

## Dictionaries

A dictionary is a plain JSON file (`.bimparams`: `format`, `guid`, `name`, `version`, `parameters`) of definitions. BIMCraft
reads it from a local path and never fetches one (the network rule in the root `CLAUDE.md`), so a team keeps it on a shared drive
or in a Git repository, whose history and review are the governance a parameter list needs. A project embeds the definitions it
uses and records where each dictionary was and at which version (`Model::dictionaries`), so it opens without the file, and a newer
file changes nothing until it's imported: Dictionaries > Check shows what it would change, as a dry run. The `dictionary` command
edits a file; the project isn't touched. A file of a newer `format` is refused.

## Import and export

`parameter_import` reads, plans (`parameters::plan`) and only then applies, so a dry run and an import share their code. A row is
matched by GUID first, then for a table with no GUIDs by name. A row of a name taken by another GUID is a `conflict` until `resolve`
says `same` (remembered in `same_as`), `separate` (comes in as `name 2`, keeping its GUID) or `skip`. A file can add categories, and
takes them off only with `replace_categories`, since that loses values. A change of kind that would strand values is a conflict.
Names are made nameable as an IFC import's are (`import::parameter_name`), and the row says so.

Categories are matched by our names, then Revit's (`match_category`, `OST_` names too). `map` settles the rest and is kept in
`Model::parameter_map`, so it is asked once. Revit categories with no category here land on `component`, all components: `broad`
says so. More categories to come only shorten that list.

Tables are `.xlsx` (`xlsx::read`: any workbook's first sheet) and `.csv` (comma or semicolon); columns by heading, in any order:
GUID, Name, Kind, Categories (split by `;`), Group, Description, Visible, User Modifiable, Hide When Empty. Export writes `.xlsx`,
`.csv` and `.bimparams`. Definitions only: no element's values go in or out.

### Revit's shared parameter files

Read, never written (`parameter_export` refuses `.txt`; the owner's decision, 2026-11-03). The layout comes from Autodesk's public Revit
API documentation, "Definition File", so it isn't under the `.rvt` fence: tab-separated blocks, each headed by a `*` line naming its
columns, and columns are found by those names. UTF-8 and UTF-16 by the file's mark. GUIDs are kept. The file names no categories, so
the import asks for them. A `DATATYPE` with no kind here (`FORCE`…) comes in as a number and is listed; a family type, a material or an
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

`hidden`, `read_only`, `hide_when_empty`, `group` and `description` are how Properties shows a parameter; a command can still set a
read-only one. Area parameters are m², volumes m³, angles degrees.

`family_parameter_transfer` copies a family's parameters to others, with the parameters their formulas read; a name a family
has is skipped and listed.

## The window

Edits are staged in `ParameterManager::rows` and go in one `parameter_batch` on Apply; the footer counts them. Import is a dry
run shown as a table with the mapping and decisions, run again after each choice. Dictionary edits go straight to their file.

## Not yet

- Type parameters: every project parameter is an instance's. Family parameters have type and instance already.
- A family parameter bound to a definition, so that a family's and a project's are one parameter.
- Nested family parameters: they wait for family nesting ([ROADMAP](../../ROADMAP.md)).
- Dragging categories between parameters: Copy Categories From does that job.
- The definition's GUID through IFC: a project parameter's name goes out in its property set, its GUID doesn't, so one that has
  been through IFC is a new parameter. Requirements from IDS and URIs to bSDD build on that.
- Schedules that span linked models by GUID.
