# Phases and design options

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

Phases (`Model::phases`, in order) and design options (`Model::option_sets`, `phasing.rs`) are values an element holds among its
`params`, under names no project parameter or family parameter may take (`phasing::CREATED`, `DEMOLISHED`, `OPTION`; `Phase Created` is the first
phase when it has no value, `Design Option` is `set: option`; `open` renames an older file's project parameter of such a name), so `modify`, copies, schedules (`From::Param` in `schedule::OWN`)
and view filters reach them as any other. `phasing::status` says what an element is at a phase's place in the order (new,
existing, demolished, temporary, or not there); a door or window with none of its own follows its wall (`phasing::value`), and
annotation, which holds no params, belongs to a view. A view's `Graphics` hold `phase` (the project's last unless it says),
`phase_filter` (every status by category unless it says; `Model::phase_filters` show each status by category, overridden or
not at all, and `Model::phase_overrides` draw the overridden ones, never hidden or see-through) and `options` (`{set: option}`,
a set's primary unless it says); `Look::of` hides what a phase, a filter or an option leaves out and paints what a filter
overrides, below the view's own overrides. `Session::phase` and `Session::option` are what commands make (`Session::stamp`
gives each new element its `Phase Created` (none for the first phase, but a door or window is named, or it would follow its
wall) and its `Design Option` where it holds none; a copy takes the session's and loses its `Phase Demolished`; `split`, undo,
redo and the commands that replace the project stamp nothing); a refused command leaves them as they were; they
belong to the session, not the model, and the desktop sets them before each command (`BimApp::made_in`): the phase of the view
on screen when it has a phase or a phase filter of its own, and the active design option (`activate`, which shows it in the
view too). What was made in the first phase holds no value, so a phase put in before the first, or the first deleted, names it first
(`phasing::pin_first`; a door or window follows its wall, which is named). Deleting or combining a phase moves what was in it to
`into` (`phasing::move_phase`), and renaming a phase, an option or a set takes the elements, the views and the rules of filters
and schedules along; accepting a set keeps the primary's elements and deletes the rest.
IFC carries them in property sets named `ifc::PHASING`, `BIMCraft_Phasing`. An element that holds a phase or an option gets one:
`Phase Created` (always: the first phase is named too, with `ifc::IMPLIED` as its description where the element holds none, but a
door or window holds only what it says, or one with no phase would stop following its wall), `Phase Demolished` and `Design
Option`, as text. The project gets one when an element holds a phase or an option, or it changed its phases, filters or overrides,
or has an option set: `Phases`, a list of the names in order, and, where it changed them, `Phase Filters`, `Phase Overrides` and
`Design Option Sets` as the JSON a project file holds them in; a project that uses none of it writes none, and the file is as it
was. A phased project also says, for any tool that reads IFC's own words, what the work does to each element: `Status`
(`ifc::ifc_status`) in its `Pset_*Common` (`ifc::common_pset`, the one set that holds `LoadBearing` too), `DEMOLISH` where it is
taken down, `TEMPORARY` where that is in the phase it was made in, `EXISTING` where it was made in the first phase and `NEW`
else; a door or window with no phase says its wall's. It is written, not read: the import still takes phases from `BIMCraft_Phasing`. `import.rs` reads them beside the project parameters: what the project can't have (a phase or an option it hasn't, a
demolition before the creation, a list or a JSON that breaks `phasing::check_project`) is left out and listed, and the element
stays (it is the property, not the element, that breaks the rule); an implied `Phase Created` is no value; no other set is read, so
one another tool writes for phases, `Phasing` say, changes nothing.
A schedule sorts and groups `Phase Created` and `Phase Demolished` by the phase's place in `Model::phases`, not by its name, no
`Phase Demolished` coming after every phase (`schedule::sorts_as`); its filters still compare the names. The desktop's Demolish
tool (DW, `BimApp::demolish`, in the Modify tab's Phasing panel) runs `modify {params: {Phase Demolished}}` on the element clicked in a
plan, a section or the 3D view: the phase of the view on screen (the project's last unless it has one), or null when the element
already comes down in it, one undo step a click. A door or window that only follows its wall gets a note instead, and an element that
holds no `params` (annotation, levels, grids, shafts) says so and changes nothing.
The view control bar's Renovation menu (`BimApp::renovation_menu`, `phasing.rs` in the desktop, the same in both profiles) says in plain
words which phase of the work a plan, a section or the 3D view shows: All (no phase and no phase filter of its own, the view as it
starts), Existing (the project's first phase, every status by its category), Demolition (the last phase through a filter that hides
what is new, draws what stood before by its category and what comes down overridden) and New (the last through one that draws what
stood before overridden and hides what comes down). A pick is one `graphics {phase, phase_filter}`, one undo step. The words map onto
filters by what they do, never by their names, which a project renames or has in its own language (`RENOVATION`, `renovation`,
`filter_for`): the project's `Show Previous + Demolished` and `Show Previous + New` to start with. A choice the project has no
filter for is greyed out and says so, a view whose phase and filter match none of them shows Custom, the menu is greyed out where
a view template sets the phase, and the Sketch scale leaves it out with the rest of phasing.
