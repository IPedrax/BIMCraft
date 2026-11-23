# Building codes

A project's **building codes** are the rules it is held to by law or by a standard of practice, in five disciplines:
architectural, structural, electrical, mechanical and plumbing. They range from how high a riser may be, through how many
units a drain carries, to how thick a beam's cover is. They are apart from the **technical standard** (`standards.rs`,
Manage > Technical Standards), which is about drafting: line weights, sheets, object styles.

**No code values in code.** A number that a standard, a utility, a fire brigade, a maker's table or a common practice gives
is a rule of a building code. It lives in the code's data with its source, and the project can change it. Rust holds the
method (a formula, a lookup, a check), never the number. Physical constants and the safety caps stay in Rust.
`code_values_live_in_codes` (`codes/rule_tests.rs`) reads every module that reads a code and fails on a numeric constant of
its own.

The plan this follows, with the catalogue of every parameter a code may hold: [building codes for all
standards](../building-code-plan.md).

## The pieces

- **A code** (`BuildingCode`) is an entry for one discipline. It has:
  - its name, region and edition;
  - the documents it cites (`documents`, by an id of its own: title, edition, publisher, where it was found and when);
  - who checked it (`checked`; empty while unchecked);
  - its **sections**, each a group of rules as JSON.

  Every rule has a **source** (`sources`, by `section.rule`, or `section` for all of it). A source is a text, or a document
  cited by id with its item and page, so a moved link changes one line. Every rule also has a **kind** (`kinds`):
  - `requirement`, the default;
  - `recommendation`;
  - `practice` (a maker's table, a handbook);
  - `assumption` (the project's to justify);
  - `historical`;
  - `forgiving` (no document gives it).
- **A section** is read by a module as a typed struct (`codes::Section`: its discipline, its key, and a `check` of what can't
  be rules). It is registered in `codes::sections()` with:
  - what the window calls it, and a line on what it holds;
  - what reads it (`read_by`). With nothing, it is a **reference**, shown and kept but checked by nothing yet.

  The data's section deserializes into the struct. A rule of a typed section must be one the struct has; a reference
  section takes any.
- **The data**: `crates/core/data/codes/<discipline>/<code>.json`, one file a code, listed in `codes/data.rs` (a test fails
  on a file that isn't).
  - Each discipline has one `default` code: what a project that chose nothing is held to.
  - An older edition the library keeps for projects pinned to it is `<code>@<edition>.json`, with `superseded_by`.
  - Region presets: `data/codes/presets.json`.

## Layers

A project (`Model::codes`, a `codes::Codes`) has, for each discipline (`Choice`):
- a **base** code;
- **amendments** over it, in order;
- the rules it **changed**, by section, each a rule's whole value.

The rules in force are the layers', the last one winning rule by rule (`codes::resolve_in`, `Model::resolve_code`):
1. the base;
2. each amendment;
3. the project's changes.

An amendment's `amends` names the base, or another amendment chosen: a member of a group of utilities over the group's
standard, over the national standard. Examples: São Paulo's IT 22 over NBR 5626/8160/10844; Friuli Venezia Giulia's
LR 44/1985 over Italy's DM 5 July 1975; a city's code over the IBC; a utility's standard (Cemig, Copel, Fecoergs…) over
NBR 5410. The amendments go in the order the project gives (the dialog's
list, dragged), the last winning rule by rule.

**A change goes into the amendment edited.** With `amend` (in the dialog, the amendment marked "changes go here", the last
one by default), `building_code`'s `rules` and `reset` change that amendment, not the project's own changes:
- one of the project's own amendments is changed in place;
- one of BIMCraft's becomes the project's copy under its name (`Codes::library_of` is what it was taken from); each number
  changed says so in its source, and a reset of the whole copy takes it away, so the library's is there again;
- a rule set to null leaves the amendment, so the layers under it give it again (the dialog's Take Out).

`create` makes an empty amendment of the project's own over the base (or `over`), which `amendments` then chooses.

**A section none of the layers has comes from BIMCraft's library**: the first code of its discipline that holds it, the
default first (`codes::library`). So a command works whatever the project chose. Its `from` says so (`library`). A
project's changes still apply over it. A section only amendments hold, that is not whole without the rules they leave out
(an amendment of the project's own that changes one door width), is the library's under what they change; one an amendment
holds whole (a utility's method) is that amendment's alone.

**The electrical demand's methods are the code's.** A utility's method (`unit_demand`, `entrance`, `building_demand`) is its
amendment over NBR 5410, a country's its base code (the NEC, the REBT). `demand` and `building_demand` given no `utility` (or
`standard`) take the one the project is held to, as it changed it, and say to hold it to one when it holds none. The
first Building Codes' `building_code {electrical: {demand, building_demand}}`, a code's `country` and `Model::electrical_methods`
are gone: this is what they did.

**Editions are pinned.** When a project chooses a code, its edition is kept (`Choice::editions`). A newer edition in the
library changes nothing until the code is chosen again. Where the library no longer has the edition pinned, the project is
held to the one there is, and `building_code` (`editions_gone`) and the dialog say so.

**Site and Climate** (`codes::Site`) are the numbers a code's maps give for the project's place: rain, wind, snow,
thunderstorm days, frost depth, climate zone, design temperatures. Each has the map it was read from. They are the project's
and shared by every discipline. `storm_size` reads the rain when it isn't given.

A module whose section is a type of its own with no form to write back (the electrical sections: `dr.rs`, `bath.rs`,
`loads.rs`, `building_demand.rs`, `entrance.rs`, and with a check of their numbers by `codes::Valid`, `nbr5410.rs`
(`circuits`), `conduit.rs` (`conduit_fill`), `dps.rs` (`surge`) and `demand.rs` (`unit_demand`)) registers it with `valid_as`
or `valid_checked`, reads the project's with
`Model::section_as` (and whether a layer of the project holds it or the library), and its library by a standard's or a
utility's name with `codes::library_sections`.

Reading a section is cheap: `Model::rules::<S>()` resolves it once and keeps it until the project's codes change (the
model's `code_cache`, keyed by the identity of `Model::codes`, a copy-on-write `Shared`). `codes::library_rules::<S>()` is
the library's, for a module's functions that have no project.

## Commands

- `building_code`: `{discipline?, set?, amendments?, keep_rules?, rules?: {section: {rule: value}}, stairs?, reset?, save?,
  delete?}`.
  - #386's `{set, stairs}` still works, for the architectural discipline.
  - The reply has the rules in force by section, what the project changed, and every code it may choose.
- `building_code_export` and `building_code_import`: XML, one discipline or all five in one file (`all`). Format 2:
  - `<section>` and `<rule name value|json source kind/>`, a table as JSON;
  - the documents as `<document id title edition url accessed/>`.

  Format 1 (#386's stairs-only file) is still read. What a file holds that this version doesn't know is listed as
  `left_out`. A rule a file leaves out of a section a module reads is the code's it is based on.
- `code_rules`: every rule in force, with its layer, source and kind. For agents.
- `site_climate`: the site's numbers.
- `code_check`: every finding of the elements given, or of all. `stair_check` and `stair_solve` stay for stairs.

An element a section checks has `code_check` (in short) and `code_findings` in its JSON. Today that is stairs, ramps, doors
and rooms (`codes/stairs.rs`, `codes/checks.rs`). A room is of the first kind (`rooms.kinds`) one of whose names its name
holds, and is held to the kind's least area, width and height and its natural light (`light_min`: the area of its windows
over its floor's, the IBC's 8 % for a habitable room). Its windows are those in the walls the lookup finds round it whose
middle lies on its boundary and whose height is the room's.

## Manage > Building Codes

The desktop's dialog (`crates/ui-egui/src/codes.rs`) has an **Overview** and a tab for each discipline.
- **The Overview** has:
  - a region preset that sets every tab;
  - Import All and Export All;
  - a card for each tab (its code, amendments, whether it was checked, the project's changes, the findings in the project);
  - Site and Climate, each number with its source.
- **A discipline's tab** has:
  - its code picker;
  - its amendments as a list in the order they are laid, each dragged by its handle (or moved by its arrows), taken away
    by its cross and marked as the one changes go into; more ticked from Choose Amendments (one ticked brings what it
    stands over), and Create Amendment makes one of the project's own. While one is on, a changed rule's From says it;
  - Import, Export, Save As, Delete and Reset Tab, for that discipline alone;
  - the sections down the left: `•` marks the project's changes, `(ref.)` a reference, and a search box looks through
    every rule;
  - the rules grid: Rule, Value, From and Source. A changed rule shows "Changed: the code says …" and Reset;
  - a table editor for a rule that is a table: cells edited in place, rows added and taken away, Copy and Paste as
    tab-separated text for a spreadsheet; the kinds of room have rows of their own (kind, room names, least area, width
    and height, natural light as a percentage);
  - the rule card: its full source, kind and layer.
- The Electrical tab's first sections are the project's Electrical Settings (`electrical.rs`: voltages, distribution
  systems, wire types), which Manage > Electrical Settings opens.
- The Overview's region preset says the region whose codes the tabs are.
- OK and Apply send the electrical settings (one undo step), then one `building_code` for each tab that changed (with
  `amend` while an amendment is edited), then `site_climate`.

## Principles

1. **Nothing is refused for breaking a rule.** What was drawn is built. What is outside a rule is a *finding*: the rule by
   name, what the element has, what the rule asks, and a sentence saying both and what to change. `Model::check` refuses
   only what cannot be the element at all, and rules that cannot be rules (a range upside down, a table of the wrong shape).
2. **Rules are data, with sources.** A number is never written into the code; it is a rule of a code's section, with the
   document it comes from. A type may set a rule in its own place (a stair type's riser range), never silently.
3. **The solver answers, the user chooses.** A solver proposes the nearest elements that fit, least change first, as the
   params the element's command takes. It builds nothing.
4. **A check is fast, and spatial.** See below: this one is not negotiable.

## Every check is fast

A code check does not run once, on request. It runs:

- under the cursor, every frame, while an element is drawn (the stair tool shows "Inside the rules" or what is outside,
  and offers the nearest stair that fits, which Tab takes);
- while a grip is dragged;
- on a placed element after **any** edit anywhere in the project (the Properties palette shows its findings, and its cache
  is of the model);
- inside a solver, once for every candidate it tries.

So a check must cost what is *near* the element, never what is in the building. The rules:

- **Never walk every element of the model.** Take the candidates from the model's lookup (`Model::lookup`, `derived.rs`):
  - walls by plan tile and height (`walls_near_at`, `walls_between`);
  - runs and components by band of height (`groups`, `group_between`);
  - the kinds a project has few of, by kind (`rare`: columns, shafts, stairs, ceilings…);
  - `other` for the rest a plan scans (floors, roofs).

  If a check needs a kind the lookup doesn't index well, index it there (as `"ceiling"` was added to `RARE` for headroom),
  don't scan.
- **Cull with numbers you already have**, before any geometry: the element's box in plan and its heights against the
  candidate's. Only what is left gets an outline, a join, a solid.
- **Don't build what is shared every time.** A wall's or a run's outline searches its neighbours for joins; with no index
  that is the whole model, for each one (quadratic on a tower).
- **Measure on the highrise** (`sample::highrise(20)`, the `highrise` command) before calling a check done: well under a
  millisecond in a release build, and a test that fails if it grows with the building. The stair check is timed this way in
  a 12-storey tower (`a_stair_in_a_tower_measures_its_headroom_to_what_is_over_it_and_only_that`), and the room and door
  checks on the highrise (`a_room_or_a_door_is_checked_in_the_same_time_however_tall_the_building`).

What happens when this is forgotten: `stairs::headroom` once made the outline of every floor, ceiling and beam of every
storey, each beam searching every run for its joins. Drawing a stair in the 20-storey highrise cost 1.9 s a frame for the
check under the cursor and 42 s for the solver's offer, and every edit re-checked a selected stair the same way. Taking
its candidates from the lookup and culling by box and height brought it to under a millisecond.

## Adding a section

1. Its rules are a struct beside the module that reads them, implementing `codes::Section` (its discipline, its key, and a
   `check` that says what can't be rules). Register it in `codes::sections()` with its label, a line on what it holds and
   what reads it.
2. Its numbers go in a code's file, each with a source and, where it isn't the document's requirement, a kind. A generic or
   default code holds forgiving numbers where a section must always read, never a copy of another code's table: that stays in
   its standard's code, and the library hands it on saying whose it is (a test in `codes/tests.rs` holds this).
3. The module takes its rules as an argument (`WaterRules::smallest_dn`). Its command reads `Model::rules::<S>()`, so the
   project's choice and changes hold. The old free functions may stay, reading `codes::library_rules`, and the module's tests
   pin the numbers through the move.
4. The module joins `READERS` in `codes/rule_tests.rs`, and its numeric constants go.
5. If it checks elements, its findings are `Finding`s from a function of its own (as `codes/checks.rs`), dispatched by
   `codes::findings`. They are fast and spatial from the start, timed on the highrise, with a test.

## Adding a country

- A new file in `data/codes/<discipline>/`, listed in `codes/data.rs`, with:
  - its `documents`, a source for each rule and `checked` empty;
  - only the numbers and the documents' names, items and pages taken from the documents; the words are ours.
- If it only changes another code, it is an amendment (`amends`), and holds only what it changes.
- A preset in `presets.json` if it makes a region.

Stairs, what they check and why: [an advanced stair tool](../stair-tool-plan.md). Their geometry and commands:
[levels, walls and roofs](levels-walls-roofs.md). The plumbing commands and the concrete check:
[structure and MEP](structure-and-mep.md).

**The electrical settings** (voltages, distribution systems, wire types) are sections of the electrical code too, an entry a
rule by its name: a code chosen brings its own into the project, and what the project changes goes into the code (see
[structure and MEP](structure-and-mep.md)). An entry the codes set to null leaves the project, and one they no longer name
goes back to what a new project has (or away, if a new project has none), unless an element still names it; one taken away
that a new project has is written as null, so an import doesn't bring it back. Renaming a voltage renames it in the code's
systems too, and a system naming a voltage the project wouldn't have is refused, as is taking away a voltage a system names
or a setting an element names (a board's system, a circuit's voltage or wire type). Taken away in an amendment, an entry is
null there, whoever had it.
The dialog shows them as the Electrical Settings tables, not in the rules grid.

**How the dialog shows a rule** that is more than a number (`codes/tree.rs`): an object as a small form, a list of plain values
as a row of fields, a list of rows as a table whose columns are named (an object inside a row, and one inside that, spread over columns of its own,
the columns a utility explains saying so when pointed at), and rows that hold lists of their own as cards. A section's own
words (`standard`, `source`, `id`, `name`, `country`, `*_source`, `rule`, `notes`, `not_covered`, `columns`) are not rules: the
From and Source columns say who and where (a `factor_source` is `factor`'s source), and its notes show as text over the grid.
