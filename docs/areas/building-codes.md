# Building codes

A project's **building code** is the set of rules its elements are held to by law or by a standard of practice: how high a
riser may be, how deep a going, how much headroom there is over a step. It is apart from the **technical standard**
(`standards.rs`, Manage > Technical Standards), which is about drafting: line weights, sheets, object styles.

Today a code holds rules for stairs (and the handrail and guard heights that go with them). It is meant to grow past them:
ramps, doors and corridors, escape routes, guards at an edge, rooms' areas and openings. What follows holds for every
category a code comes to cover, not only stairs.

## Where it lives

- `crates/core/src/codes.rs`: the codes (`BuildingCode`), their rules (`StairRules`, one struct a category), the rules an
  element is held to (`limits`, `limits_of`: the project's, with what its type sets in their place), the findings
  (`Finding`, `stair_findings`) and the solver (`solve`, `Locks`, `Proposal`).
- `crates/core/data/building-codes.json`: the codes BIMCraft ships (generic, NBR 9050 and NBR 9077, IBC, DIN, Japan). Each rule
  names its `source` (the document, its edition and its item); `checked` says who checked the numbers against the text and
  where that is written. The NBR entries are checked; the rest were typed from memory and say so. Where a code gives no
  number, the entry holds a forgiving one and says why.
- `crates/ui-egui/src/codes.rs`: Manage > Building Codes, the dialog that picks a code, changes its rules and keeps a
  project's own.
- The commands: `building_code` (names one, changes any rule for the project, `{save}` keeps the rules in force under a
  name), `building_code_export` and `building_code_import` (one handed on as XML: `BuildingCode::to_xml`, `from_xml`, a
  `<rule name value/>` each, a rule left out being the generic code's, what the file has that we don't know listed as
  `left_out`), `stair_check` (the findings) and `stair_solve` (the solver, `apply` to make one). An element's JSON carries
  `code_check` (in short) and `code_findings` (each finding).

A project names a code (`Model::building_code`) and may change any of its rules (`Model::stair_rules`: a state's or a
city's own numbers). It keeps codes of its own too (`Model::building_codes`, found with BIMCraft's by `Model::code`).

## Principles

1. **Nothing is refused for breaking a rule.** What was drawn is built; what is outside a rule is a *finding*: the rule by
   name, what the element has, what the rule asks, and a sentence saying both and what to change. `Model::check` refuses
   only what cannot be the element at all.
2. **Rules are data, with sources.** A number is never written into the code; it is a field of the code's entry, with the
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
  walls by plan tile and height (`walls_near_at`, `walls_between`), runs and components by band of height (`groups`,
  `group_between`), the kinds a project has few of by kind (`rare`: columns, shafts, stairs, ceilings…), and `other` for
  the rest a plan scans (floors, roofs). If a check needs a kind the lookup doesn't index well, index it there (as
  `"ceiling"` was added to `RARE` for headroom), don't scan.
- **Cull with numbers you already have**, before any geometry: the element's box in plan and its heights against the
  candidate's. Only what is left gets an outline, a join, a solid.
- **Don't build what is shared every time.** A wall's or a run's outline searches its neighbours for joins; with no index
  that is the whole model, for each one (quadratic on a tower).
- **Measure on the highrise** (`sample::highrise(20)`, the `highrise` command) before calling a check done: well under a
  millisecond in a release build, and a test that fails if it grows with the building (as
  `a_stair_in_a_tower_measures_its_headroom_to_what_is_over_it_and_only_that` times the stair check in a 12-storey tower).

What happens when this is forgotten: `stairs::headroom` once made the outline of every floor, ceiling and beam of every
storey, each beam searching every run for its joins. Drawing a stair in the 20-storey highrise cost 1.9 s a frame for the
check under the cursor and 42 s for the solver's offer, and every edit re-checked a selected stair the same way. Taking
its candidates from the lookup and culling by box and height brought it to under a millisecond.

## Adding a category of rules

- Its rules are a struct of their own beside `StairRules` (a field of `BuildingCode`, as `stairs` is), each with a
  `source` in every entry of `building-codes.json` and a forgiving number where a code has none.
- Its findings are `Finding`s, from a function of their own beside `stair_findings`, run by a command (as `stair_check`)
  and put in the element's JSON as `code_check` and `code_findings`.
- It is fast and spatial from the start, timed on the highrise, with a test.
- The desktop shows it where the element is drawn and edited, as the stair tool does, and Manage > Building Codes gets a tab
  for its rules.

Stairs, what they check and why: [an advanced stair tool](../stair-tool-plan.md); their geometry and commands:
[levels, walls and roofs](levels-walls-roofs.md).
