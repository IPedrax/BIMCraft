# Project parameters and schedules

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

(A distribution board's panel schedule is not one of these: it is a view of its own, `panel_schedule.rs`, in [structure and MEP](structure-and-mep.md).)

A project parameter (`Model::parameters`) is a value each element of its categories may hold in its `params`, of its kind;
a component's `params` hold its family's instance values too, so a parameter for components can't take a family
parameter's name, nor any category's own field's (`schedule::own_field`). A schedule's fields are its category's own
(`OWN` in `schedule.rs`, read from `element_json`), the project parameters for it and, for components, the families'
parameters, or calculated: a formula over the row's others, read as the family formula engine reads (lengths in mm, areas in
m²), a field with no value leaving it with none (an unset yes or no is no). Filters compare numbers as the schedule shows
them, so two that show the same are equal. The six schedules every project has (`schedule::VIEWS`) aren't elements
until changed or placed (`schedule::standard`), and a schedule named as one of them, in any language or case, is that one; renaming a parameter renames it in schedules, but a formula naming it holds
the rename back.
A table goes to a file as CSV (`schedule::to_csv`, text a spreadsheet would read as a formula guarded with `'`) or, for a path
ending `.xlsx`, as a workbook of one sheet (`schedule::to_xlsx` over `xlsx.rs`, which writes the parts and the zip by hand on
`flate2`): headings in bold, numbers as numbers so they sum, text as inline strings, which a spreadsheet never takes for a formula.
