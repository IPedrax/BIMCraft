# An advanced stair tool

A plan, in pieces, for stairs that are better than Revit's rather than as good. It starts from research: what people who
draw stairs for a living say is wrong with Revit's tool, and what Archicad, Vectorworks and BricsCAD do instead, good and
bad. Then what BIMCraft's stair is today, the principles the new one follows, and the pieces in the order they would be
built. Nothing here is built yet; each piece is a pull request of its own, test first.

Clean-room, as always: this reads public help pages, forums and blogs for *behaviour*, and copies no content, family,
icon or help text. The numbers of building codes quoted below are from memory and unchecked, as `standards.rs` says of its
own; a rule set is checked against the text of its standard by whoever contributes it.

## What is wrong with Revit's stairs

Collected from Autodesk's forums and Ideas board, the Real World Revit wiki, RevitCat and BIM Pure (sources at the end).
The Ideas pages themselves refuse automated readers, so their titles are quoted from pages that link them and their vote
counts are not known.

1. **It refuses instead of building.** "Can't create stairs" is the tool's best-known message. A sketch with one small
   fault does not finish; overlapping risers and a boundary of the user's own are beyond the run engine, so people
   sketch by hand, and the monolithic underside comes out warped round an L-turn (the advice is to model the soffit as an
   in-place component). A curved or elliptical stair wants every riser radial, which fights where the landing must go.
2. **Its rules are three numbers and a warning.** A stair type holds a maximum riser, a minimum tread and a minimum run
   width. There is no step rule (2R + G), no pitch, no limit on risers in a flight, no check of a landing's size and no
   headroom. Break a rule and it warns; it never proposes a stair that would fit.
3. **Winders are one region's idea of winders.** The default does not make what a UK joiner expects; the fix is another
   winder style and then editing the sketch lines by hand. There is no control of the going on the walk line or of the
   narrow end, which is what the codes actually rule.
4. **Landings are automatic only between runs.** One at the top or the bottom is always a sketch. Landings that are not
   parallel fail in cast-in-place stairs. A landing's path line only places the arrow.
5. **Multistorey stairs need equal storeys.** Unequal levels give a warning and a stair at the wrong height, or separate
   groups; the workaround is separate stairs, which no longer follow the levels. A component turned into a sketch is to
   be avoided in a multistorey stair.
6. **The plan symbol is the largest source of complaint.** The cut mark obeys a plan region only when the stair's whole
   bounding box lies in it; on a stair at an angle it follows a rule nobody could explain; a stair going *down* has no
   proper cut mark and its treads below do not obey the view range; landings and whole flights above the cut show unless
   their "Above" subcategories are switched off. A stair path cannot be made for a sketched stair, its text changes sides
   with the direction drawn, and its arrowheads and tread numbers cannot be styled.
7. **A stair joins nothing.** Not to a slab or to other concrete; a wall cannot attach under it; a void cannot cut it;
   the meeting with the top slab is hand work. "Extend Below Base" extends with a *negative* number.
8. **Railings break on stairs.** Baluster patterns that work on a level run leave gaps and untrimmed panels at every
   change of slope; the railing path is drawn in 2D only, which is hard past two flights; a multistorey stair's inner
   railing warns. People model a top rail and draw the rest in 2D.
9. **Small frictions.** A run is drawn bottom to top only (a reversed one is flipped afterwards); a stair drawn in 3D
   needs a level work plane; parameters are spread over the stair, its run type, its landing type and its support type.

## What the others do better

**Archicad** (the Stair Tool since 21) is the one users hold up against Revit, and Revit Ideas posts name it.

- A stair is a **baseline**: a polyline of segments and turnings, the stair to its left, centre or right, drawn upward
  or downward. Each turning is a landing or a winder, changed where it stands.
- **Rules and Standards** live in the template: ranges for riser and going, the step rules, the walking line's offset
  range, an automatic landing's minimum length in millimetres *or in goings*, and where a landing's extra length goes
  (start, end, both). Length is measured along the arc or along the chord, as the country's standard says.
- The **Solver**: when what was drawn cannot meet the rules, it shows the stairs that can, ordered from the least
  change, each previewed in place. This is the single feature Revit most lacks.
- **Four winder constructions** with equal goings on the walking line (single point, two points, principal lines,
  auxiliary walking line), with a count of goings or a maximum.
- **Structure apart from finish**: monolithic, beam, cantilevered or stringer under treads and risers that are their
  own components, each with a profile.
- **Story links**: the stair follows its storeys; a small change keeps the risers and changes their height within the
  rules, a large one marks every stair it broke.
- **The plan symbol and the ceiling-plan symbol are separate**, each with its own break marks, walking line, numbering
  and text; edit mode works in plan, section and 3D.
- **Headroom** is a volume (measured upright or square to the walking line, in from the edges) that collision detection
  reads.

Its users' complaints are a warning: winder settings where "one small difference" makes the wanted stair impossible;
landings of a free or curved shape (the advice is a slab and a solid operation); stairs whose path curves back on
itself; headroom that stops at the top tread where the IBC wants it over the landing; a member once edited is "custom"
and no longer follows the settings.

**Vectorworks** is weaker in shape and stronger in two places.

- **Lock and link.** Every number of the stair (total rise, riser, going, 2R + G, length, risers per flight, widths)
  can be locked; the rest are worked out, and an error dialog proposes a correction. Minimums and maximums can be
  loosened while designing.
- **The 2D look per floor.** The lower and the upper floor each have a break style (none, above only, below only,
  dashed kinds), a break height, angle and separation, and a break offset that slides the break along the walk line so
  the two floors' drawings meet without a gap; the walk line's markers and arrow direction are settings. Saved as a set.
- Total rise is bound to storey levels with offsets; landing corners can be square, chamfered or rounded.

Its limits: fixed configurations (straight, L, U, O, circular), so no flared, S-shaped or free stair; the walk line of
a winder is centred on the first flight only and cannot be set, which is exactly what the codes measure; winders get
pushed into the straight flights.

**BricsCAD BIM**: two clicks, the tool picks the configuration (straight, L, U) that best fits its numbers between them,
and **cuts the hole in the slab above from the headroom**.

## What BIMCraft has today

`Stair` (`model.rs`) is a start point, an angle, one width, a maximum riser, a tread and a list of landings, each after so
many risers and turning 0, ±90 or ±180 degrees, as deep as the stair is wide (`stairs::chain`). No stair type, no winders,
no curved run, no structure, nosing or stringers, no rules beyond the maximum riser. In plan: risers to the cut line, the
rest dashed, the walk line marked UP or DN. A railing follows the walk line moved to a side. It is honest and small, and it
already avoids two of Revit's faults: it cannot fail to build, and it follows its levels.

## Principles

1. **A stair always builds.** What was drawn is made; what breaks a rule is *reported*, never refused. `Model::check`
   refuses only what cannot be a stair at all (no rise, a width of nothing, a path that is not finite). Everything else is
   a finding of `stair_check`, with what to change.
2. **A path and rules, not a sketch.** The stair is a baseline (lines and arcs), a side, a direction, and the turnings
   on it. Risers come from the rules, so there is no riser line to draw wrong. A landing may carry an outline of its own
   (`sketch.rs`), and that is the only sketch.
3. **Rules are data, by building code.** A code is an entry of `data/building-codes.json` (as `load-points.json`):
   riser and going ranges, the step rule's range, minimum width, when a landing is due, a landing's least length, where
   the walk line lies and the least going there and at the narrow end, headroom, handrails. A project picks one
   (Manage > Building Codes, the `building_code` command) and may change any rule of it.
4. **The solver answers, the user chooses.** `stair_solve` reads a start, an end or a room's outline and the rise, and
   returns the stairs that fit the rules, least change first, each as the params `stair` would take. It builds nothing
   (as `timber_framing` reads and never builds), so an agent or the desktop can preview and pick.
5. **Lock what matters.** Any of risers, riser height, going and length may be fixed; the others follow.
6. **Every storey for itself.** A stair through several levels works out each storey from its own height, so unequal
   storeys are ordinary, and it follows them when they move.
7. **The plan symbol belongs to the view.** What is under the cut is solid, what is over it dashed or left out, the break
   where the cut plane meets the flight on *this* stair's slope, on the level above the whole stair with DN; no bounding
   boxes, no subcategories to remember. Arrow, text and numbering are the stair type's.
8. **Headroom is geometry.** The volume over the walk surface, including over the top landing; `stair_check` reports
   what stands in it, and the opening in the floor above can be cut from it.
9. **One place for parameters.** A stair type (a library entry, with an id and a GUID) holds structure, tread, riser and
   nosing; the instance holds the path and what is locked.

## Decided (2026-11-11)

- **Building codes are not technical standards.** Technical Standards is about drafting (pens, sheets, dimensions). The
  rules of stairs and railings are a project's **building code**, a menu and a command of its own, with a "Stairs &
  Railings" tab; other parts of a building may get tabs there later.
- **The code prefills, the user decides.** Choosing a code fills the rules and what a new stair starts with. Every rule
  can be changed (a state's fire brigade, a city), and one that is changed is marked with the code's own number beside
  it. A stair outside the rules is built, and a sign on it says where it is outside.
- **Where a code gives no number, a forgiving one stands in**, one that asks nothing in practice (the IBC has no step
  rule; DIN counts risers and not millimetres of rise), and the code's note says which.
- **A landing is due by rise or by risers, whichever the code has**: both are data, and the first one reached asks for
  the landing.
- **A code changed is Custom, and can be kept.** Once a rule differs from the code's, the dialog says Custom; the rules
  can be saved under a name of the project's own, picked again like any code, and exported and imported as XML.
- **A stair is drawn along a line, as Archicad's is** (decided 2026-11-12). The line is straight, an arc or a circle,
  picked in the ribbon; each stretch drawn is steps or a landing, picked in the ribbon too; any angle and run after run
  are simply how it works, and a double click finishes it at any time. Sizes and the location line stay on the Options
  Bar.
- **Dimension rules live on the stair type** (decided 2026-11-12): riser, tread, two risers and a tread, tread over
  riser, tread and riser together, the pitch, the landing's length and the walk line's offset, each a minimum and a
  maximum that can be switched on or off, prefilled from the building code. A rule switched on on the type takes over
  from the code for its stairs.
- **How a stair shows in plan is its type's** (decided 2026-11-12): one of Archicad's nine ways (with or without the
  break mark, each side of it shown, hidden or left out) for each of three cases, the level below, the same level and
  the level above; the line type of what shows and of what is hidden, and the fill of the solid part.
- **Winders are always drawable.** The walk line is worked out for every code: at a fixed distance from the narrow side
  where the code says so (the IBC's 12 inches, Japan's 300 mm), along the middle otherwise. Where a code does not allow
  winders (NBR, on an accessible route or an emergency stair), they are drawn all the same and reported as against the
  code. A tool is never taken away because one code forbids what it makes.

## Roadmap

Each step ends in something to test in the app, keeps old files reading (a step in `Model::migrate` and a `FORMAT` bump
where an older version would misread), adds its payloads to the hostile sweep, and is looked at as
[looking at results](areas/looking-at-results.md) says. This table is kept up to date as the work goes: the state of
each step, and what was left out of it. "Revit" names what its Stair tool has that the step answers (Modify | Create
Stair: Run, Landing and Support components; the run shapes straight, full-step spiral, centre-ends spiral, L-shape winder,
U-shape winder and sketch; Location Line, Offset, Actual Run Width and Automatic Landing in the Options Bar; Connect
Levels; Convert, Edit Sketch, Flip, Railing).

| # | Step | What it adds | Revit | State |
|---|---|---|---|---|
| 1 | Building codes | `data/building-codes.json` (generic, NBR, IBC, DIN, Japan), the project's code and its own rules (`building_code`, Manage > Building Codes), a new stair starting with them, `landings: "auto"`, `stair_check`, and the sign in a stair's Properties with a popup listing each rule broken. A project's own codes: its rules saved under a name (`building_code {save}`; the dialog says Custom once a rule is changed, and saves it), handed on as XML (`building_code_export`, `building_code_import`). | Its calculation rules only warn; here the code is data, and the project's to change | Done. The numbers are unchecked; headroom, nosing, uniformity and railing heights are stored and not yet checked |
| 2 | Winders, basic | A turn of 90 or 180 made of `winders` treads fanned round its inside corner, checked on the code's walk line; the Stair tool's Winders option. | L-shape and U-shape winder runs | Done. Point winders only: the narrow end is always reported where a code has a least depth there |
| 3 | Location line | `line` (left, centre, right) and `offset`: the line the stair is drawn by, which stays when the width changes; in the Options Bar and Properties. | Location Line (Run: Left, Center, Right; Exterior Support: Left, Right) and Offset | Done for the run's lines. The exterior support's lines come with stair types (step 9) |
| 4 | Landings of their own | A landing's `length`, prefilled from the code's landing rule in the tool and by `"auto"`. | Automatic Landing; the Landing component | Done for the depth. Every landing the tool makes follows the code: the run after the turn gets the landings due on its way. Still to come: a landing's own outline (step 10), and Automatic Landing switched off (runs drawn apart, joined later; with the path, step 7) |
| 5 | Editing a placed stair | Properties for landings and winders, grips for the ends, the turns, the width and a landing's depth, the findings live while dragging, Flip (the way it climbs). | Editing a stair's components in its edit mode; Flip | Part done: a Landings and Turns group in Properties (after how many risers, the way the next run goes, winders, the landing's length; Add Landing, Remove, By the Code), `flip` on a stair (the same ground, climbed the other way) and a Modify Stair panel with Flip and Code Check. Grips on the plan (`stairs::grips`, `stairs::dragged`): the foot moves the stair, the top turns it about its foot, a run's end before a turn gives or takes risers, a side sets the width from the location line, a landing's far edge its length; a drag shows the stair as it would be and how many rules it would be outside of. Done; grips in the 3D view are not |
| 6 | Balanced winders | Winders run into the flights so the narrow end holds the code's least; a choice of construction; the walk line an arc in plan; railings following it. | The winder run's Minimum Width at Inside Walkline and number of winders | Part done: `balance` on a turn, the treads of the flight on each side its winders run into; those treads and the winders are redrawn as steps equal along the walk line (the stair's middle) and along the inside string, each riser the line through its two points (`stairs::balanced`), so none comes to a point, and `stair_check` measures the narrow end and the walk-line going of what is drawn. In the Stair tool (Balance) and Properties (Balanced Treads). The walk line goes round a winding turn as an arc at the stair's middle, and a railing with it. The solver chooses the `balance` that holds the code's narrow end. Still to come: other constructions to choose from, and the outside railing following the stair's square edge and not the arc (step 15) |
| 7 | The path | Runs of their own widths, turns at any angle, runs drawn one by one (a click each), up or down. Changes the file's format, with a migration. | Run by run creation; Actual Run Width per run | Part done: a turn by any angle from 5 to 135 degrees, on a landing (the runs share the turn's inside corner, the landing reaching to where their outside edges meet) or on winders, plain or balanced; in the Stair tool, Any Angle (the turn clicked, square when near square) and Chain (run after run, each click turning the next run and ending it). Nothing in the file's format had to change for these. A run after a landing takes a `width` of its own (after one that goes straight on, turns square or turns back): the landing is as wide as the wider run, as deep as the run leaving its side, or across both runs beside each other; the plan, 3D, the stair type's parts, IFC and the least width follow each run's own. A stair is drawn along a line (`path`: from its start, segment after segment, each a flight of steps or a landing as long as it is drawn, straight or an arc; the corner where two flights meet is a landing, or winders): the Stair tool draws it so, with Straight, Arc, Spiral and By Risers and Steps and Landing in the ribbon, a double click or Enter finishing it and the last flight taking the risers left. Still to come: a width of its own after winders or an angled turn, a landing's length on an angled turn, runs drawn apart and joined, drawn from the top down, railings on runs of other widths |
| 8 | Curved and spiral | Arc runs, a spiral round a post by full steps or by centre and ends, goings on the walk line. | Full-step spiral and centre-ends spiral | Part done: `spiral` {radius, sweep?} on a stair: it winds about a centre, every tread a piece of the ring and a riser above the last, as deep on its middle line as `tread` or turning `sweep` degrees in all; `stair_check` measures it on the code's walk line and at the narrow end and reports it under a code without winders; `flip` reverses it; in the Stair tool, Spiral (the centre, then the first riser) and Clockwise. Still to come: an arc run joined to straight runs, a landing in a spiral, a post at its centre (stair types, step 9). Its grips: the foot moves it, the centre sets its radius |
| 9 | Stair types and supports | A library entry: monolithic, stringers (closed or open), carriage, cantilevered; tread and riser thickness, nosing, open risers, materials; structure apart from finish; the support's own lines as location lines. | Assembled, cast-in-place and precast stairs; the Support component; Edit Type | Part done: stair types (`type` with category stair, a library entry like the other types; a project starts with Monolithic 150mm, Stringers with open and with closed risers, and Cantilevered Treads): a `waist` makes a slab under the steps, sloping with them and never dipping under the floor or a landing; with none, treads of `tread_thickness` stand free, with risers of `riser_thickness` and a stringer of `stringer_width` by `stringer_depth` down each side of a run where the type has them; `nosing` reaches a tread out over the one below. The 3D view and sections draw them (`stairs::parts`); a stair of no type stays solid to its level, as old files' stairs do. Still to come: **covers** (a finish on the treads and on the risers, each with its thickness and how far it stands out past the riser and the sides, as Archicad's tread and riser finishes), materials, a centre stringer (carriage), a spiral's post, the support's lines as location lines, the plan and IFC following the type |
| 10 | Custom sketched stairs | A stair drawn by hand in sketch mode, as floors are (Edit Boundary, Finish, Cancel): its boundary (the two sides), its risers, each a line drawn where the user wants it, and its path (the way up); landings sketched as outlines of any shape. Any run or landing of a made stair converted to a sketch and edited, and back while it is still a plain one. The risers drawn are counted against the height and the code like any stair's, and `stair_check` measures each tread on the walk line, so a sketched stair is reported, never refused. | Create Sketch (Boundary, Riser, Stair Path), Convert, Edit Sketch | Part done: a stair's `sketch`, a line for each riser from the first up, the tread after it the ground to the next riser or an outline of its own (a landing round a corner); as many risers as are drawn share the height. `stair_check` measures it as drawn (its shallowest tread on the line through the risers' middles, its narrowest riser), and it is moved, flipped, typed and built as any stair. `stair_sketch` converts a stair made by runs, landings and winders, every riser and corner where it was. In the Stair tool, Sketch (two clicks a riser, Enter the stair); Convert to Sketch in the ribbon; every riser end a grip. Still to come: boundaries that are not straight between risers, a tread's outline drawn in the tool, a single run or landing converted and the rest kept, converting a spiral, and converting back |
| 11 | Solver and locks | `stair_solve`: lock what matters, get the stairs that fit the code, least change first. | Desired Number of Risers, and nothing more | Part done: `stair_solve` (`codes::solve`): the stairs nearest one that fit the code, least change first, over the count of risers, the tread the step rule allows, the least width and the landings due (the stair's own kept and lengthened, more where a flight is too long); `lock` keeps what it names; when none fits, the nearest with what each still breaks; `apply` makes one. The Building Code Check window lists them, each with Apply. It balances winders that come to a point, by as few treads as give the narrow end the code's least. While a stair is being drawn, the cursor says whether it would be inside the rules, and which it would break. Still to come: locks kept on the stair itself, and the solver's answers offered while drawing |
| 12 | Multistorey | One stair across several levels, following them when they move, a landing at each floor. **Storeys of different heights**: each storey has its own count of risers and riser height inside the code, the going kept the same where it can be; the flights of a taller storey are longer or gain a landing, and `stair_check` reports a storey that cannot be made inside the rules, and how much two storeys' risers differ (the code's uniformity is within a flight, and a project may ask it between storeys). | Multistory Stairs > Connect Levels, which groups storeys of equal height and gives a stair of its own to one that differs | Part done: `multistorey` on a stair with a top level several levels up: it is the stair of its first storey (which is what is drawn, gripped, checked and solved) and stands again on each level on the way (`Model::storeys`, `stairs::storeys`), each storey with the risers its own height asks and the first's landings after the same share of them (one flight where a run would be left a riser). `stair_check` holds each storey's riser to the code and reports how far the storeys' risers differ (the code's uniformity). The plan of each level shows the storey standing on it; the 3D view, every storey; the Stair tool has a Height and Multistorey. Still to come: a storey edited apart from the others, a spiral, railings and IFC by storey |
| 13 | The plan symbol | Break mark on the slope, above and below per floor, arrows, tread numbers, per type. | Cut marks, stair paths and tread numbers, each an annotation of its own there | Part done: the break mark is a slanted line with a jog across the run the cut plane passes through, placed by height (so it is right after winders and on every storey); on a level a multistorey stair passes, the storey climbing from it is drawn to the break mark and marked UP, and past it the storey coming up from below is drawn as seen from above and marked DN, the two arrows meeting at the mark. Where the plane cuts winders, a spiral or a tread drawn by hand, the mark crosses the lowest tread over it, square to the walk line. Still to come: tread numbers, what shows above the cut chosen per type or view, and the plan following the stair type (nosings, stringers) |
| 14 | Headroom and the opening | The headroom volume checked against floors and beams; the opening in the floor above cut from it. | None: a shaft is drawn by hand | Part done: `stair_check` reports the least headroom from a step up to the underside of a floor standing over it (`stairs::headroom`; a floor cut open there, by an opening of its own or a shaft, is not over it; a step inside the floor's thickness has none), and `stair_opening` cuts each such floor open round the steps short of headroom, one outline round them however the stair turns (`stairs::united`), with Cut Opening in the check window. Still to come: beams, roofs and ceilings as obstacles, the headroom measured on the nosing line and not step by step, a margin round the opening, and an opening that follows the stair when it changes |
| 15 | Railings | Balusters by tread or by spacing that stay whole across slope changes, rails continuous over landings and winders, heights from the code, both sides and the well. | Railing on Treads or Stringer | |
| 16 | IFC both ways | `IfcStair` of `IfcStairFlight`, landings, winder treads and railings with `Pset_StairFlightCommon`; import keeps a stair a stair. | IFC export of stairs | |
| 17 | Finish | The codes' numbers checked against the documents, the dialog in the project's units, translations. | | Part done: the Building Codes dialog and the check window write lengths in the project's units (millimetres to the hundredth, a code's inch being 25.4). Still to come: the codes' numbers checked against their texts, the new labels translated, and whether a stair's new fields call for a new file format number |

## Handoff and to-do (2026-11-12)

Everything a new session needs to pick the work up. Branch `feat/advanced-stair-tool`, draft PR
[#386](https://github.com/IPedrax/BIMCraft/pull/386); push with
`git push https://github.com/dansity/BIMCraft HEAD:feat/advanced-stair-tool` (the only remote, `origin`, is upstream). The
last commit is `9d6d12a7` (stairs drawn along a line). **Railings are out of this work's scope**: they are a category of
their own, and the user decided this branch does nothing with them. Working mode
the user asked for: push through the coding, run only the tests near the change while working, the full checks
(`cargo fmt --check`, clippy, `cargo test --workspace`) once before each commit, commit and push often, keep this table
and the PR description up to date. An RPM when asked: `packaging/linux/package.sh --formats rpm` →
`dist/release/bimcraft-0.11.0-linux-x86_64.rpm`.

### 1. The batch in the working tree (not committed)

What it adds:

- On `StairType` (`model.rs`): `rules: TypeRules`, which has a min and a max each (`Option<f64>`) for the riser, going,
  step (2R+G), ratio (G/R), sum (G+R) and pitch, plus `landing_min` and `walk_offset_min`. It also gains
  `plan: PlanShows {below, same, above}` of `PlanShow`, and `line_visible`, `line_hidden` and `fill`.
- `Model::check` refuses a min above its max and over-long pattern names.
- The `type` command for a stair takes `rules` and `plan` as a merge patch, where `null` takes a rule off. It also takes
  `line_visible`, `line_hidden` and `fill`.
- `codes::limits` / `limits_of` give a stair its rules: the type's rules over the project's code. `stair_findings` and
  `solve` read them, and there are new findings for ratio, sum and pitch.
- `render::stair` draws each of the three cases by the type's `PlanShow` (`Shown`, `StairLook`, `shown_as`). Dashes come
  from the named line patterns and the fill from `fill_pattern`.
- `stairs::Goings` and `allot` stretch or shrink a path's treads within the rules so that a flight ends where it was drawn.
  `Run::going()` is each run's own going, and the plan, 3D and IFC read it.
- New desktop file `crates/ui-egui/src/stairtype.rs`, the stair page of Type Properties:
  - the rules grid: a tick and a field for the min, a tick and a field for the max; an unticked rule shows the code's
    number greyed;
  - the three plan pickers;
  - the line-pattern and fill pickers.

Steps to finish it:

1. Fix two test expectations in `crates/core/src/codes/tests.rs`. The code is right; the tests predate stretched treads.
   - About line 946, `a_stair_is_drawn_along_a_path_of_flights_and_landings`: the corner landing is now the corner alone,
     1000 × 1000. The old 1000 × 1000 + 100 × 1000 included the 100 mm that the flight's treads now take up.
   - About line 1009, `a_stair_type_has_rules_of_its_own_and_says_how_its_stairs_show_in_plan`: the solver proposes 19
     risers, not 18, because the type's `pitch_max: 30` also counts. Assert `>= 18`.
2. Run `cargo test -p bimcraft-core --lib -- codes::` and the ui-egui stair and type tests
   (`cargo test -p bimcraft-ui-egui stair`, `… type`).
3. Run clippy and `cargo i18n update`: there are new UI labels in `stairtype.rs` and `codes.rs` `TYPE_LABELS`.
4. If the hostile sweep has no payload for `type` with `rules` and `plan`, add one.
5. Write a desktop test that opens a stair type's Type Properties, ticks a rule and checks the `type` command it sends.
6. Commit as "Stairs: a stair type's own rules, plan display, line patterns and fill; treads stretched to a flight's length".
   Then push, update rows 7, 9 and 13 below and the PR description, and rebuild the RPM.
7. Tell the user what **Add Landings Due** is; they asked about the old **By the Code** button and are waiting for the
   answer:
   - By the Code replaced a stair's landings with the code's, all straight on, so it dropped turns. That was the bug they
     saw.
   - Add Landings Due runs the solver with the risers, tread and width locked. It keeps every landing the stair has and adds
     only those the code still asks for (a flight too long).

### 1b. Added to the batch in the second session (not committed; the user validates first)

- The batch's two stale test expectations fixed (stretched treads take 9 of 277.8 in 2500).
- **Covers** (step 9): `tread_cover`, `riser_cover`, `cover_nosing`, `cover_side`, `cover_material` and a structure
  `material` on `StairType`; `stairs::covers` builds them, `parts` sits the structure `tread_cover` lower (risers finish to
  finish); 3D paints each in its material; IFC writes them as one `IfcCovering` .FLOORING. among the stair's parts.
- **Step profile** for a monolithic stair: `profile` `square` or `slanted`, the riser leaning back from the tread's front
  edge by `nosing` (the user's sketch); runs and the riser onto a landing; the riser cover follows the slant. The user
  said the finishes will get profile options of their own later: ask which.
- **Middle stringer** (`carriage_width` × `carriage_depth`) and a spiral's **post** (`post`, its diameter).
- **Plan**: tread numbers (`numbers` on the type), stringers' inner lines, treads' front edges at their nosing; the fill
  was already drawn under every visible part.
- **Type Properties**: Finishes rows, Step Profile, Materials, Tread Numbers; the plan pickers draw a little flight for
  each way (`PlanShow::look` in the core, which the renderer reads too).
- **Building Codes**: riser, tread, step and handrail as one row each with min. and max. side by side.
- **Connect Levels** in Modify Stair: a level picked makes the stair multistorey up to it; Disconnect Levels undoes it.
- Hostile payloads for the type's new fields; `cargo i18n update` run.
- **Drawing along a line keeps what is clicked**: short of its top, the stair is what is drawn (`stairs::drawn_risers`),
  its top under its top level by the risers not drawn, so a flight clicked stays put when a landing is drawn next; a
  double click or Enter finishes it there. Stairs gained `base_offset` and `top_offset` (`Model::stair_span`), in
  Properties as Base Offset and Top Offset. A monolithic landing is a riser deep at least (the riser onto it had a gap).
- Third round: offsets measured **below** their levels (`stair_span`: level less offset); **Lock Tread Depth** in the
  ribbon's Segment panel (`Stair::lock_going`, on by default: one depth the whole stair; off, flights stretch); the
  location line's offset follows arcs (`pieces`), so a left or right line no longer drops when an arc is drawn; a curved
  flight's and a spiral's underside smooth (each tread's corners share heights with its neighbours, cut into triangles,
  shaded as one surface; the triangles' edges still show where edges are drawn); `start_tread` / `end_tread` on the type
  (Begin with Riser / End with Riser in Type Properties); Type Properties greys the numbers the type does not use.
- Fourth round: a flight going straight on into the next (a straight one into an arc, no corner or landing) is
  **chained** (`Allotted::chained`, `chains`, `held`): its last tread is the step onto the next one's first riser, a
  tread deep, and the pieces after it move with it (`moved` in `pathed`), so no sliver of a landing is left where the
  locked depth does not divide the length. A monolithic landing is the slab's thickness again, and a flight's slab
  **goes on under a landing** until the undersides meet (`joint`, from a run's end and from an arc's last riser); a run
  from a landing stands on its underside, finish allowed for. The finish **stands out over every riser**: a nosing strip
  where a run climbs onto a landing, the floor or an end tread, and on the risers no run has (`Layout::nosings`: round
  an arc, a spiral's), with their riser covers. The type's plan lines are **line styles** (`line_visible`,
  `line_hidden`), Medium Lines and Hidden Lines unless it names others (`lines::MEDIUM`, `lines::HIDDEN`).
- Fifth round: drawing reads "Step k of n - height" (the count from the height asked when the top is unconnected, not
  from the stair cut short). A landing's **free edges** (no riser or tread on them) have grips (`Grip::Edge`,
  `stairs::landing_edges`): dragged, the edge moves square to itself, kept as `Stair::edges` ([{landing, normal, by}]),
  applied in `laid` so every storey, cover and the plan follow. Flip forgets them.
- Sixth round: **a width for each flight and landing** of a stair drawn along a path (`PathSeg::width`; `pieces` moves
  each segment by half its own width for the location line and mitres where two meet; a corner takes of each side what
  the other's width needs; a landing's sides are drawn stretch by stretch, `beside`). The Width box while drawing gives
  the next stretch its width. **Parts picked on their own**: a click on the stair already selected picks the flight or
  landing under it (`stairs::stair_parts`, `part_at`; `canvas.stair_part`), outlined on the canvas, and Properties lists
  Flights and Landings, each with its Width, the picked one first. **Draw from Top** in the ribbon: the stair is drawn
  down from where it arrives, and one finished short hangs from its top (`base_offset` negative). Winders' finishes
  stand out over their risers (an edge shared with the step below); a landing's edge on a run's last riser is left out
  of the plan (the nosing line stands for it); the walk line runs onto a start or end tread (UP clear of it); IFC
  writes treads that are no run's as one-tread IfcStairFlights (SPIRAL, CURVED or WINDER), not landings; the smooth
  underside's shared sides are left out of the 3D view (no triangle lines); headroom also measures to ceilings and
  beams. The flight-to-landing wedge is left out where the landing is too short to hold it (before winders).

### 2. What the user's screenshots showed (the reference for what is left)

- **Revit's Modify | Create Stair ribbon** (for step 7 and the tool):
  - Mode: Cancel and Finish.
  - Components: Run, Landing, Support, and the run shapes (straight, full-step spiral, centre-ends spiral, L winder,
    U winder, sketch).
  - Multistorey Stairs: Connect Levels.
  - Work Plane.
  - Tools: Convert, Edit Sketch, Flip, Railing.
  - Options Bar: Location Line (Run: Center), Offset, Actual Run Width, Automatic Landing.
  - Properties: the type (Assembled Stair) and Edit Type.
  - Ours now: the Draw panel (Straight, Arc, Spiral, By Risers) and the Segment panel (Steps, Landing) in the ribbon; size
    and location line in the Options Bar. **Still to do: Support as a component drawn on its own, and Connect Levels as a
    ribbon button.**
- **Archicad's floor plan display, one of nine for each of three cases** (level below, own level, level above). This is
  done in the working tree; the `PlanShow` names map to Archicad's list in its order:
  - with break mark, visible / hidden → `break_visible_hidden`;
  - no break mark, visible → `no_break_visible`;
  - below the break mark, visible → `below_visible`;
  - above the break mark, visible → `above_visible`;
  - no break mark, hidden → `no_break_hidden`;
  - above the break mark, hidden → `above_hidden`;
  - below the break mark, hidden → `below_hidden`;
  - with break mark, all visible → `break_all_visible`;
  - with break mark, hidden / visible → `break_hidden_visible`;
  - and our own `not_shown`.

  Defaults: the level below `not_shown`, the own level `break_visible_hidden`, the level above `no_break_visible`.
  **Still to do:**
  - a preview icon for each choice in the picker, as Archicad's list has;
  - the fill drawn under the visible part in every case;
  - a view able to override the type's choice (Visibility/Graphics).
- **Min and max with two fields** (Revit's Maximum Riser Height and Actual Tread Depth, each given a second box). This is
  done as the rules grid. **Still to do:** the same min/max pair in the Building Codes dialog for the project's code, not
  only on the type.
- **Archicad's Rules and Standards panel.** The numbers are its defaults, in metres. "Done" means in the working tree.

  | Rule | Ticked | Min | Max | Here |
  |---|---|---|---|---|
  | Riser height | both | 0.150 | 0.200 | Done |
  | Tread depth | both | 0.250 | 0.350 | Done |
  | 2 risers + 1 tread | both | 0.600 | 0.650 | Done |
  | Tread / riser ratio | off | 0.100 | 1.000 | Done |
  | Tread + riser | off | 0.450 | 0.600 | Done |
  | Pitch | off | 20° | 30° | Done |
  | Automatic landing length | on | 1.000 | | Done as `landing_min` |
  | Walk-line offset range | on | 0.300 | | Done as `walk_offset_min` |

  **Still to do, as on that panel:**
  - **Baseline lock** (ticked and greyed there).
  - **Length calculation**, a choice that was set to "along arc (previous version)": how a curved flight's length is
    measured.
  - **Landing extra-length distribution**, a choice that was set to "evenly": where the length a landing gains beyond its
    minimum goes.
  - **Walk line at a landing**, a choice that was set to "angular", against rounded.
- **Covers: Archicad's tread and riser finishes** (step 9). The picture showed a section of two steps with these numbers:
  - 0.025, the thickness of the tread's cover;
  - 0.010 under it, how far the tread's cover stands out at the nosing;
  - 0.015, the thickness of the riser's cover;
  - two greyed 0.010 fields, the side overhangs, which apply only when a side is open.

  To do:
  - `tread_cover`, `riser_cover`, `cover_nosing` and `cover_side` on `StairType`, each with a material;
  - `stairs::parts` adds the cover solids on top of the structure;
  - the riser height stays measured finish to finish, so the structure steps sit lower by the cover;
  - IFC writes the covers as `IfcCovering` or as a layer;
  - the Type Properties page gets a little diagram like Archicad's.
- **A monolithic stair converted to a sketch lost the continuity of its underside.** Fixed and committed: a tread that is
  not part of a straight run gets its own sloping underside in `stairs::parts`.

### 1c. Seventh round (2026-11-13)

- **Stair types as a choice of construction**: `StairType::construction` (monolithic, stringers on both sides, a centre
  stringer, treads fixed in a wall), a drop-down in Type Properties with a little side view of each; the sizes shown are those
  the construction uses (`StairDraft::used`), and the others are kept for switching back. New types are made with Duplicate,
  as for every type; the project starts with a Centre Stringer type as well.
- **Stringers round landings** (`landing_stringers`), along each free edge, in plan as their inner lines; a centre stringer
  goes on under the treads round an arc.
- **Finish profiles**: square, rounded top edge, chamfered top edge, bullnose (`FinishProfile`, cut in slices at the nosing).
- **Plan symbols**: the walk line's head (filled, open, outlined, tick, none), its foot (none, dot, circle), rounded corners
  at a landing (Archicad's Walk line at landing), and the cut mark (zigzag, double, straight, wave).
- **Visibility/Graphics**: the Stairs category has subcategories, Runs, Risers, Landings, Stringers and Supports, Finishes,
  Walk Line and Arrow, Cut Marks, Tread Numbers and Headroom, each hidden or overridden on its own in plan, section and 3D.
  **Headroom** is a see-through block over every tread and landing as high as the code asks, off until a view shows it.
  A plan view may override the type's Floor Plan Display (Properties > Graphics, `stair_display`).
- **Dimensions snap to stairs**: every side, end, riser and landing edge is a reference in plan (`Face::Edge`); 3D dimensions
  snap to the stair's edges as to any solid's. Sections and elevations have no dimensions yet, for any element.
- **IFC**: flights, landings, stringers (`IfcMember .STRINGER.`) and finishes as the type builds them; the stair's shape as
  IFC4 names it (straight, quarter turn, half turn, winding, curved, spiral); `Pset_StairCommon` and
  `Pset_StairFlightCommon`, `Qto_StairFlightBaseQuantities`, the structure's material; and `BIMCraft_Stair`, from which an
  import makes the same stair of the same type again.
- **Building codes, from the review on #386**: NBR 9050 (accessible route) and NBR 9077 (emergency exit) are two codes; every
  number of theirs names its item (`sources`), shown beside it in Manage > Building Codes and carried in the XML (`source`);
  `checked` says who checked them; `nosing_min` is a rule now (9077 asks 15 mm), and open risers are checked. A project made
  with `nbr` is held to NBR 9050.
- **While drawing**: Riser and Tread boxes in the Options Bar (empty, as the rules say), and when the stair would break a rule,
  the nearest one that fits is offered under the cursor: Tab takes it.
- **Also**: landing edge grips on every stair, not only those drawn along a line; a spiral converts to a sketch, and
  **Convert Back** makes a converted stair what it was; **Separate Storeys** makes each storey of a multistorey stair its own
  stair; `stair_opening` takes a `margin`; headroom is measured over the whole step (its nosing line too) and under roofs.
- **Words**: the stair tools' tips, the Type Properties page (a Model part and a Graphics part, a note under each choice) and
  the code check's advice rewritten in plain words; new icons for Draw from Top, Code Check, Convert to Sketch and Connect
  Levels.

### 3. The to-do, closed

Each item of the earlier list, and what became of it.

- **7, the path**: a width of its own after winders or an angled turn, done (sixth round, per segment); a landing's length
  on an angled turn, done (the corner takes what each side's width needs); flights drawn apart and joined later, done as a
  landing stretch drawn between them; drawn from the top down, done (Draw from Top); a full circle in the path: an arc
  turns up to 340° and a full circle is two arcs, and a stair of more turns is the Spiral shape. Decided, not built: a
  single 360° segment has no chord to draw it by.
- **9, stair types**: covers, materials, a centre stringer, a spiral's post, the plan's nosings and stringers, IFC following
  the type: done. The support's lines as location lines: decided against; the location line stays the run's left, centre or
  right with an offset, which is what one matches to a drawing.
- **10, sketch**: converting a spiral and converting back, done. Boundaries not straight between risers and a tread's own
  outline: the model holds them (`SketchStep::outline`) and Convert to Sketch fills them; drawing them by hand stays the
  riser-by-riser tool. Converting one run and keeping the rest: decided against, a stair is one sketch or none; Separate
  Storeys and Convert Back cover the cases that came up.
- **11, solver**: its proposals while drawing, done (Tab). Locks kept on the stair: decided against; Lock Tread Depth is
  kept, the rest are the solver's choices of the moment.
- **12, multistorey**: a storey edited apart, done (Separate Storeys). A multistorey spiral works as any multistorey stair.
- **13, plan symbol**: tread numbers, the plan following the type, a view overriding it: done.
- **14, headroom**: beams, ceilings and roofs as obstacles, measured over the whole step, a margin round the opening: done.
  An opening that follows the stair: decided against; the shafts `stair_opening` makes are elements of their own, run again
  after a change.
- **15, railings**: out of scope (a category of their own).
- **16, IFC both ways**: done, both ways for BIMCraft's own files; another tool's stair is still measured from its flights.
- **17, finish**: the NBR numbers checked by a contributor (the others say they are unchecked, rule by rule); translations
  updated with `cargo i18n update`; `FORMAT`: no bump. Every new field has a default and is left out when it has it, so an
  older version reads a file without them as before; one with them is read leniently (`lenient.rs` leaves out what it doesn't
  know and says so).
- **Archicad's Rules and Standards panel**: baseline lock is how BIMCraft always works (the location line holds); length
  calculation is along the walk line, the only way; the landing's extra length goes where the user drags its free edge;
  walk line at a landing, angular or rounded, done (`walk_rounded`).
- **Revit's ribbon**: Connect Levels is a ribbon button (Modify Stair); Support as a component drawn on its own: decided
  against, supports come from the type (side, centre or landing stringers).

## Open questions

- **The codes' numbers**: NBR 9050 and NBR 9077 were checked against their text by a contributor (#386). The IBC, DIN 18065
  and Japan's Building Standards Law are typed from memory, and each rule's `source` says so; someone with the texts should
  check them, and say whether a code needs a row for each occupancy (a dwelling, a school).
- **Is a stair's structure its type's, or may a flight differ?** Archicad allows it and its users report trouble with
  mixed structures; this plan says the type's.
- **Railings** are out of scope for this work.
- **Ramps** share `chain` today. They take the path (piece 3) and the plan symbol with the stair; rules of their own later.

## Sources

Revit: [Real World Revit: Stairs](https://revit.miraheze.org/wiki/Stairs) ·
[RevitCat on railings and the Ideas list](https://revitcat.blogspot.com/2019/01/how-do-you-solve-problem-like-ma.html) ·
[RevitCat on sketched stairs, 2015](https://revitcat.blogspot.com.au/2015_12_01_archive.html) and
[2016](https://revitcat.blogspot.com.au/2016_01_01_archive.html) ·
[BIM Pure: 13 tips](https://www.bimpure.com/blog/13-tips-to-understand-revit-stairs) ·
[Arkance: winders](https://emea.support.arkance.world/hc/sk/articles/36471275011474-Revit-Tip-Modelling-of-Stair-Winders) ·
[CAD Notes: multistory stairs](https://www.cad-notes.com/revit-2018-multistory-stairs/) ·
Autodesk forums: [L-shaped winders](https://forums.autodesk.com/t5/revit-api-forum/how-to-fix-the-infamous-quot-can-t-create-stairs-quot-error-for/m-p/14155839),
[elliptical sketch](https://forums.autodesk.com/t5/revit-architecture-forum/elliptical-stair-sketch/m-p/11306622),
[multiple level stairs](https://forums.autodesk.com/t5/revit-architecture-forum/multiple-level-stairs/m-p/6487241) ·
Revit Ideas (titles only): [A Stair Tool that actually works](https://forums.autodesk.com/t5/revit-ideas/a-stair-tool-that-actually-works/idc-p/9669941/highlight/true),
[Stair modeling - smart sketch](https://forums.autodesk.com/t5/revit-ideas/stair-modeling-smart-sketch/idc-p/8717914).

Archicad: [Rules and Standards](https://help.graphisoft.com/AC/28/INT/_AC28_Help/040_ElementsVB/040_ElementsVB-188.htm) ·
[Winder types](https://help.graphisoft.com/AC/24/INT/_AC24_Help/040_ElementsVB/040_ElementsVB-189.htm) ·
[AECbytes on the Stair Tool](https://www.aecbytes.com/tipsandtricks/2019/issue85-archicad.html) ·
community: [Stair tool nightmares](https://community.graphisoft.com/t5/Modeling/Stair-tool-nightmares/m-p/325564/highlight/true),
[landing shape](https://community.graphisoft.com/t5/Modeling/How-can-I-change-the-landing-shape-of-a-stair/m-p/220048/highlight/true),
[headroom over the top landing](https://community.graphisoft.com/t5/Documentation/Stair-headroom-over-top-landing/td-p/715573).

Vectorworks: [Geometry tab](https://app-help.vectorworks.net/2024/eng/VW2024_Guide/Stairs/Stair_settings_Geometry_tab.htm) ·
[2D Graphics tab](https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Stairs/Stair_settings_2D_Graphics_tab.htm) ·
[Stair Tool pain points](https://forum.vectorworks.net/topic/58406-stair-tool-pain-points/) (read through search excerpts only).

BricsCAD: [Stair tool](https://help.bricsys.com/en-us/document/bricscad-bim/modeling-techniques/stair-tool).
