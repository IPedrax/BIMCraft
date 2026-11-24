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
The `renovation` command reads the phases as a renovation palette does (Archicad's is where the idea is seen), with three
statuses over two phases: the work's (`phase`, the project's last unless it says, the view's on the desktop) and the one
before it, what stands (`phasing::renovation_phases`; refused in a project of one phase, or for the first). `{status}` sets
what the session draws in: existing is made in the phase before, new in the work's, and demolished is made in the phase
before and taken down in the work's, by `Session::demolish`, a third field beside `Session::phase` and `Session::option`
that `Session::stamp` writes as `Phase Demolished` (never before the phase made in), that a refused command, a transaction,
a session that may only read and Co-Work keep with the other two (`engine::MadeIn`, the guest's `demolish` beside its `phase`
and `option`), that renaming or deleting a phase moves, and that `phase {now}` clears; null follows the view again (no
phase of the session's own). `{status, ids}` gives elements the status in one undo step (`phasing::give_status`): existing
and demolished keep an earlier phase an element was made in, and a door or window is given a phase of its own, so it no
longer follows its wall. `ids` alone reads each one's status in the work's phase and the selection's (or `mixed`).
The view control bar's Renovation menu (`BimApp::renovation_menu`, `phasing.rs` in the desktop, the same in both profiles) says in plain
words which phase of the work a plan, a section or the 3D view shows: All (no phase and no phase filter of its own, the view as it
starts), Existing (the project's first phase, every status by its category), Demolition (the last phase through a filter that hides
what is new, draws what stood before by its category and what comes down overridden) and New (the last through one that draws what
stood before overridden and hides what comes down), and Comparative (the last through a filter that draws every status
overridden: what stands, what comes down and what is built in one view, as the phases' overrides colour them). A pick is one `graphics {phase, phase_filter}`, one undo step. The words map onto
filters by what they do, never by their names, which a project renames or has in its own language (`RENOVATION`, `renovation`,
`filter_for`): the project's `Show Previous + Demolished` and `Show Previous + New` to start with. A choice the project has no
filter for is greyed out and says so, but for Comparative, which no project starts with: its first pick makes the filter
(`phasing::COMPARATIVE_FILTER`, named `Comparative` in the project's language, a number after it when that name is taken) in
the same undo step as the view's `graphics`, a view whose phase and filter match none of them shows Custom, the menu is greyed out where
a view template sets the phase, and the Sketch scale leaves it out with the rest of phasing.

The Renovation palette (`Palette::Renovation`, `BimApp::renovation_palette` in the desktop's `phasing.rs`; Modify > Phasing >
Renovation Palette, or View > User Interface, closed in a new window, the same in both profiles) puts the three together. Draw in:
Follow the View (the tools as they always were, the palette's start) or Draw Existing, Draw To Demolish, Draw New, the window's
`BimApp::drawn_as`, never the project's and no undo step. It holds in every view until another is picked: `BimApp::made_in` gives
each command the phases of that status over the phase the view shows, the one before it what stands (`phasing::drawn_in`, which
the `renovation` command uses too), so with more than two phases a view of a later one draws a later stage of the work. A
`renovation {status}` an agent runs through the window (`exec`, the control port) becomes the palette's (`renovation_ran`).
Below, the selection's status in the view's phase (or Mixed), and Set Existing, Set To Demolish and Set New, one
`renovation {status, ids, phase}` each, one undo step, a door or window given its own phase. Last, The view shows: the
Renovation menu's five choices as buttons. In a project of one phase, or in a view of the first, the statuses are greyed
out and the palette says what it needs.
