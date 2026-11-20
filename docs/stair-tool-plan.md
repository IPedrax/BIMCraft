# An advanced stair tool

A plan, in pieces, for stairs that are better than Revit's rather than as good. It starts from research: what people who
draw stairs for a living say is wrong with Revit's tool, and what Archicad, Vectorworks and BricsCAD do instead, good and
bad. Then what BIMCraft's stair was before, the principles the new one follows, the pieces in the order they were built,
and what was decided along the way. The pieces went in together (#386); the area write-ups say how they work now:
[levels, walls and roofs](areas/levels-walls-roofs.md) and [building codes](areas/building-codes.md).

Clean-room, as always: this reads public help pages, forums and blogs for *behaviour*, and copies no content, family,
icon or help text. The numbers of building codes quoted below are from memory and unchecked, as `standards.rs` says of its
own; a rule set is checked against the text of its standard by whoever contributes it.

## What is wrong with Revit's stairs

Collected from what users write about it in public: the Real World Revit wiki, RevitCat, BIM Pure and others (sources at
the end).

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

**Archicad** (the Stair Tool since 21) is the one users hold up against Revit.

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

## What BIMCraft had before

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
10. **A code check is fast, and spatial.** Every building-code check runs under the cursor while drawing, after any edit
   on a placed element, and for each candidate of the solver, so it takes what is near from the model's lookup and never
   walks every element. The rule, how and the story of why are the building code's own, not the stair's:
   [building codes](areas/building-codes.md#every-check-is-fast).

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

## The pieces

Each step ends in something to test in the app, keeps old files reading (a step in `Model::migrate` and a `FORMAT` bump
where an older version would misread), adds its payloads to the hostile sweep, and is looked at as
[looking at results](areas/looking-at-results.md) says. What was left of each, and what became of it, is in *Decided along
the way*. "Revit" names what its Stair tool has that the step answers (Modify | Create
Stair: Run, Landing and Support components; the run shapes straight, full-step spiral, centre-ends spiral, L-shape winder,
U-shape winder and sketch; Location Line, Offset, Actual Run Width and Automatic Landing in the Options Bar; Connect
Levels; Convert, Edit Sketch, Flip, Railing).

| # | Step | What it adds | Revit |
|---|---|---|---|
| 1 | Building codes | `data/building-codes.json` (generic, NBR, IBC, DIN, Japan), the project's code and its own rules (`building_code`, Manage > Building Codes), a new stair starting with them, `landings: "auto"`, `stair_check`, and the sign in a stair's Properties with a popup listing each rule broken. A project's own codes: its rules saved under a name (`building_code {save}`; the dialog says Custom once a rule is changed, and saves it), handed on as XML (`building_code_export`, `building_code_import`). | Its calculation rules only warn; here the code is data, and the project's to change |
| 2 | Winders, basic | A turn of 90 or 180 made of `winders` treads fanned round its inside corner, checked on the code's walk line; the Stair tool's Winders option. | L-shape and U-shape winder runs |
| 3 | Location line | `line` (left, centre, right) and `offset`: the line the stair is drawn by, which stays when the width changes; in the Options Bar and Properties. | Location Line (Run: Left, Center, Right; Exterior Support: Left, Right) and Offset |
| 4 | Landings of their own | A landing's `length`, prefilled from the code's landing rule in the tool and by `"auto"`. | Automatic Landing; the Landing component |
| 5 | Editing a placed stair | Properties for landings and winders, grips for the ends, the turns, the width and a landing's depth, the findings live while dragging, Flip (the way it climbs). | Editing a stair's components in its edit mode; Flip |
| 6 | Balanced winders | Winders run into the flights so the narrow end holds the code's least; a choice of construction; the walk line an arc in plan; railings following it. | The winder run's Minimum Width at Inside Walkline and number of winders |
| 7 | The path | Runs of their own widths, turns at any angle, runs drawn one by one (a click each), up or down. Changes the file's format, with a migration. | Run by run creation; Actual Run Width per run |
| 8 | Curved and spiral | Arc runs, a spiral round a post by full steps or by centre and ends, goings on the walk line. | Full-step spiral and centre-ends spiral |
| 9 | Stair types and supports | A library entry: monolithic, stringers (closed or open), carriage, cantilevered; tread and riser thickness, nosing, open risers, materials; structure apart from finish; the support's own lines as location lines. | Assembled, cast-in-place and precast stairs; the Support component; Edit Type |
| 10 | Custom sketched stairs | A stair drawn by hand in sketch mode, as floors are (Edit Boundary, Finish, Cancel): its boundary (the two sides), its risers, each a line drawn where the user wants it, and its path (the way up); landings sketched as outlines of any shape. Any run or landing of a made stair converted to a sketch and edited, and back while it is still a plain one. The risers drawn are counted against the height and the code like any stair's, and `stair_check` measures each tread on the walk line, so a sketched stair is reported, never refused. | Create Sketch (Boundary, Riser, Stair Path), Convert, Edit Sketch |
| 11 | Solver and locks | `stair_solve`: lock what matters, get the stairs that fit the code, least change first. | Desired Number of Risers, and nothing more |
| 12 | Multistorey | One stair across several levels, following them when they move, a landing at each floor. **Storeys of different heights**: each storey has its own count of risers and riser height inside the code, the going kept the same where it can be; the flights of a taller storey are longer or gain a landing, and `stair_check` reports a storey that cannot be made inside the rules, and how much two storeys' risers differ (the code's uniformity is within a flight, and a project may ask it between storeys). | Multistory Stairs > Connect Levels, which groups storeys of equal height and gives a stair of its own to one that differs |
| 13 | The plan symbol | Break mark on the slope, above and below per floor, arrows, tread numbers, per type. | Cut marks, stair paths and tread numbers, each an annotation of its own there |
| 14 | Headroom and the opening | The headroom volume checked against floors and beams; the opening in the floor above cut from it. | None: a shaft is drawn by hand |
| 15 | Railings | Balusters by tread or by spacing that stay whole across slope changes, rails continuous over landings and winders, heights from the code, both sides and the well. | Railing on Treads or Stringer |
| 16 | IFC both ways | `IfcStair` of `IfcStairFlight`, landings, winder treads and railings with `Pset_StairFlightCommon`; import keeps a stair a stair. | IFC export of stairs |
| 17 | Finish | The codes' numbers checked against the documents, the dialog in the project's units, translations. | |

## Decided along the way

What was left of each piece, and what became of it.

- **7, the path**: a width of its own after winders or an angled turn, done (per segment); a landing's length
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
  updated with `cargo i18n update`; `FORMAT` 21: an older version would refuse a dimension to a stair's edge, a view
  naming a stair's subcategories and a landing turned 5 to 135 degrees, and leave out a stair's path, spiral, location line,
  offsets and storeys, drawing it straight, shifted or a storey high, so the format went up (`FORMAT` in `model.rs`).
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
[RevitCat on railings](https://revitcat.blogspot.com/2019/01/how-do-you-solve-problem-like-ma.html) ·
[RevitCat on sketched stairs, 2015](https://revitcat.blogspot.com.au/2015_12_01_archive.html) and
[2016](https://revitcat.blogspot.com.au/2016_01_01_archive.html) ·
[BIM Pure: 13 tips](https://www.bimpure.com/blog/13-tips-to-understand-revit-stairs) ·
[Arkance: winders](https://emea.support.arkance.world/hc/sk/articles/36471275011474-Revit-Tip-Modelling-of-Stair-Winders) ·
[CAD Notes: multistory stairs](https://www.cad-notes.com/revit-2018-multistory-stairs/).

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
