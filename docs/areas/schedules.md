# Project parameters and schedules

## Equipment provisions

`component` accepts `provision: {placement, quantity, system?, host?}`. Placement is
`placed`, `planned` (awaiting installation in the model), or `nonspatial` (placement
is unnecessary). Without a provision an existing component remains placed, count 1.
Quantities are whole pieces from 1 to 1,000,000; a placed component always counts 1.
Nonplaced components need no `at`; a retained point is only a location cache and is
excluded from plan/3D geometry, snapping and IFC geometry. Placed components still
require a point. `modify` changes these fields without changing the element GUID.
An optional host references another component or run by GUID, including after save/open.
Wire endpoints must be placed devices. Component schedules expose `Placement` and
`System`; `Count` uses the provision quantity, including grouped totals.

`equipment {name, quantity?, system?, host?, product?}` adds fasteners, software and
other nonspatial items directly. It creates or reuses a type in the geometry-free
generic `Specification Items` family (named in the project language), and a separate
bill position with its own GUID. Placement labels in schedules use the project language too.
Alternatively, `equipment {type, quantity?, system?, host?}` accounts for an existing
family type by its full library name or GUID, retaining its definition and GUID.
This form rejects `name`, `product`, missing types and placeholders; it never creates
a duplicate `Specification Items` type. Undo restores an imported type to the pending list.
`product` is the same description/brand/article/manufacturer/mass metadata as the
ODS catalogue. Omitting it preserves existing type data; supplying it updates all
positions of that type. Manage > Equipment Catalogue provides a name, system and
quantity entry form from plans and 3D. Adding an item clears its name and retains the
system and quantity for batch entry. File format 10 defaults older components to placed count 1 and
prevents older readers from showing or miscounting provisions.

## Editable ODS catalogue

`ods.rs` reads and writes a bounded OpenDocument spreadsheet. `catalogue_export {path}`
atomically exports the first sheet (`Каталог`); `catalogue_import {path}` imports it as
one undoable command. The schema is `GUID, Category, Name, Width_mm, Depth_mm,
Round, Material, Description, Brand, Article, Manufacturer, Mass_kg, Height_mm,
CarrierEnclosure, ClearWidth_mm, ClearDepth_mm` for wire, cable, cablecarrier and
family types. The original seven-, twelve- and thirteen-column schemas remain readable;
their absent measured-section columns preserve existing data. In the full schema,
blank section columns clear that metadata. CarrierEnclosure accepts `blind_box`,
`lidded_box`, `tray`, `ladder` or `conduit`; non-carrier rows leave all three blank.
Existing GUIDs identify types;
an empty GUID adds a new type and library synchronization assigns its identity.
An additive `Entry.catalogue_added` flag remembers this provenance after save/open;
older files default it to false. `catalogue_pending {}` lists these types when they
have no model instances, excluding built-in unused types and deleted-type placeholders.
Any instance, including a planned or nonspatial bill position, accounts for its type.
The query is read-only. Placing an instance hides the type; undoing placement lists
it again without changing its GUID.
Changing dimensions affects every instance of that type while retaining instance GUIDs.
Family rows use category `family-type` and the library name `family: type`. New rows
can create a type in an existing family. Device dimensions edit its non-instance,
non-formula length parameters named Width/Depth/Height (English or the project language).
Unavailable dimensions export as blank; entering one reports the missing editable
parameter. Device Round and Material cells stay blank. Geometry and type/instance GUIDs
are checked after saving and reopening a two-panel project.
Product data lives on the library entry, survives project save/open and follows Insert
from File. Blank mass means unknown; zero is an explicit known value. Model validation
rejects negative, nonfinite or excessive mass and oversized text. Empty editor padding
is omitted; meaningful repeated cells and total expanded memory are bounded. Text
spacing, tabs, line breaks and CDATA are retained. Literal strings never become formulas
on export. For typed float cells in Width_mm, Depth_mm, Height_mm, Mass_kg,
ClearWidth_mm and ClearDepth_mm, and
typed boolean cells in Round, the reader uses stored ODF values rather than localized
display text. Russian decimal commas, localized boolean labels and cells without
display text therefore retain their numeric meaning. Article codes keep their
displayed leading zeros. Repeated cells are interpreted by each destination column;
stored values obey the same size limits as text. Invalid and nonfinite catalogue
values still fail atomically. This follows [ODF 1.3, sections 19.388–19.389](https://docs.oasis-open.org/office/OpenDocument/v1.3/OpenDocument-v1.3-part3-schema.html).
A CLI-exported catalogue was resaved by LibreOffice Calc and reimported with
project data and GUIDs unchanged (QA artifacts under `target/ods-qa`).
Invalid rows roll back the complete import. Renaming an identified type through ODS
is not supported yet. `catalogue_link {path}` binds an absolute external path,
imports an existing catalogue or creates one; `{path:null}` detaches it without deleting
the spreadsheet. Opening a saved project imports external replacements; a broken or
missing catalogue leaves the saved model intact and reports `catalogue_not_loaded`.
`catalogue_sync {}` protects existing model rows while accepting new blank-GUID rows.
The session remembers GUIDs exported by its linked catalogue, independently of Undo/Redo.
Rows for owned types removed by Undo are dropped on the next synchronization, with
the original workbook backed up. Redo exports the original GUID again; an intervening
edit that clears Redo does not resurrect the removed type. New/Open resets this
runtime ownership set to the current document. Save and autosave also persist a bounded
set of cancelled GUIDs, so saving immediately after Undo, before the next poll, does
not restore the cancelled product on reopening. Cleanup backs up the old ODS and then
clears the saved cancellations; a failed read/write preserves them for a later retry.
Detaching or linking a different workbook clears that workbook-specific cancellation set.
Saving does not modify the live model, Undo history or external workbook. The background
worker carries the ownership set alongside the original model snapshot, preserving
the UI's snapshot identity without copying the model on the UI thread.
Save and autosave also mark newly created product GUIDs which have not reached the linked
workbook. On reopening, missing rows for these products are exported with a backup instead
of becoming placeholders. A damaged workbook preserves this metadata across another Save
and subsequent retry. Successful synchronization clears it, so later external replacements
remain authoritative when the project opens. A different external row carrying an unexported
GUID reports a conflict and preserves both the workbook and saved model; normal open-model
`catalogue_sync` can deliberately restore the model's version with a backup. Equivalent
trailing empty cells do not cause a conflict. Detaching or linking another workbook clears
these workbook-specific markers too. Format 11 prevents older readers from discarding
unexported products; earlier files default the marker set to empty.
The session also retains the last synchronized product rows independently of Undo/Redo.
Save and autosave persist comparison rows only for locally changed products. On opening,
an unchanged external row is replaced with the pending model row, with the original
workbook backed up. A row already equal to the model is accepted. A different external
edit or deletion reports a conflict and leaves both versions intact; saving this state
retains the comparison rows for recovery. Explicit open-model `catalogue_sync` selects
the model version and backs up the external workbook. After synchronization, later
closed-model ODS edits are authoritative again. Background `save_session` carries the
comparison basis along with the original Arc identity. Format 12 prevents older readers
from silently discarding pending edits; files through format 11 default to no pending edits.
Standalone import reconciles copied catalogues with local identities. New foreign
products receive new GUIDs; equivalent named rows merge, including equivalent numeric
spellings. A differing foreign product with an existing name requires `conflicts`:
`update` preserves the existing GUID and updates all instances; `copy` creates a
separate name and GUID, preserving the original. `error` refuses differing rows,
including known GUIDs; omitting it preserves ordinary known-GUID replacement.
Category collisions and unrelated library GUIDs are refused. Imports leave the source
workbook untouched and remain one undoable command. Linked catalogues also accept
unique copied products and write their new local GUIDs back with a backup.
Relinking the same workbook protects open-model changes and remembers owned GUIDs;
linking a different workbook uses the opening/import policy.
Before rewriting modified database rows it saves the exact original workbook as a
separate `bimcraft-ods-backup-*.ods` beside the catalogue and returns its path. Other
sheets, sheet names, styles and embedded archive entries are preserved. These commands
are available to every frontend. The desktop polls a bound file every two seconds and
after model edits; unchanged polling adds no undo step. Read failures show an error in
the status bar while preserving the model, and a repaired workbook is retried after it
changes. A row deleted outside the closed bound project becomes a placeholder when the
project opens: geometry, dimensions, instance identities and references remain, product
metadata is cleared, and catalogue exports and schedules omit its instances. The reply
lists placeholder GUIDs (`catalogue_placeholders` on opening, including unlinked projects).
Restoring its original GUID row while BIM is closed restores
the product; an open model protects placeholders from external replacements too.
Standalone `catalogue_import` remains a partial import and does not delete missing rows.
Format 9 prevents older readers from billing placeholder geometry; older files default
to ordinary types. Formatted specification sheets are still pending in the
[low-voltage roadmap](../low-voltage.md).

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

A project parameter (`Model::parameters`) is a value each element of its categories may hold in its `params`, of its kind;
a component's `params` hold its family's instance values too, so a parameter for components can't take a family
parameter's name, nor any category's own field's (`schedule::own_field`). A schedule's fields are its category's own
(`OWN` in `schedule.rs`, read from `element_json`), the project parameters for it and, for components, the families'
parameters, or calculated: a formula over the row's others, read as the family formula engine reads (lengths in mm, areas in
m²), a field with no value leaving it with none (an unset yes or no is no). Filters compare numbers as the schedule shows
them, so two that show the same are equal. The six schedules every project has (`schedule::VIEWS`) aren't elements
until changed or placed (`schedule::standard`), and a schedule named as one of them, in any language or case, is that one; renaming a parameter renames it in schedules, but a formula naming it holds
the rename back.
