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
| 6 | Balanced winders | Winders run into the flights so the narrow end holds the code's least; a choice of construction; the walk line an arc in plan; railings following it. | The winder run's Minimum Width at Inside Walkline and number of winders | Part done: `balance` on a turn, the treads of the flight on each side its winders run into; those treads and the winders are redrawn as steps equal along the walk line (the stair's middle) and along the inside string, each riser the line through its two points (`stairs::balanced`), so none comes to a point, and `stair_check` measures the narrow end and the walk-line going of what is drawn. In the Stair tool (Balance) and Properties (Balanced Treads). The walk line goes round a winding turn as an arc at the stair's middle, and a railing with it. Still to come: other constructions to choose from, `balance` chosen for the user to hold the code's narrow end, and the outside railing following the stair's square edge and not the arc (step 15) |
| 7 | The path | Runs of their own widths, turns at any angle, runs drawn one by one (a click each), up or down. Changes the file's format, with a migration. | Run by run creation; Actual Run Width per run | Part done: a turn by any angle from 5 to 135 degrees, on a landing (the runs share the turn's inside corner, the landing reaching to where their outside edges meet) or on winders, plain or balanced; in the Stair tool, Any Angle (the turn clicked, square when near square) and Chain (run after run, each click turning the next run and ending it). Nothing in the file's format had to change for these. Still to come: runs of their own widths, a landing's length on an angled turn, runs drawn apart and joined, drawn from the top down |
| 8 | Curved and spiral | Arc runs, a spiral round a post by full steps or by centre and ends, goings on the walk line. | Full-step spiral and centre-ends spiral | Part done: `spiral` {radius, sweep?} on a stair: it winds about a centre, every tread a piece of the ring and a riser above the last, as deep on its middle line as `tread` or turning `sweep` degrees in all; `stair_check` measures it on the code's walk line and at the narrow end and reports it under a code without winders; `flip` reverses it; in the Stair tool, Spiral (the centre, then the first riser) and Clockwise. Still to come: an arc run joined to straight runs, a landing in a spiral, a post at its centre (stair types, step 9), and its grips |
| 9 | Stair types and supports | A library entry: monolithic, stringers (closed or open), carriage, cantilevered; tread and riser thickness, nosing, open risers, materials; structure apart from finish; the support's own lines as location lines. | Assembled, cast-in-place and precast stairs; the Support component; Edit Type | Part done: stair types (`type` with category stair, a library entry like the other types; a project starts with Monolithic 150mm, Stringers with open and with closed risers, and Cantilevered Treads): a `waist` makes a slab under the steps, sloping with them and never dipping under the floor or a landing; with none, treads of `tread_thickness` stand free, with risers of `riser_thickness` and a stringer of `stringer_width` by `stringer_depth` down each side of a run where the type has them; `nosing` reaches a tread out over the one below. The 3D view and sections draw them (`stairs::parts`); a stair of no type stays solid to its level, as old files' stairs do. Still to come: materials, a centre stringer (carriage), a spiral's post, the support's lines as location lines, the plan and IFC following the type |
| 10 | Custom sketched stairs | A stair drawn by hand in sketch mode, as floors are (Edit Boundary, Finish, Cancel): its boundary (the two sides), its risers, each a line drawn where the user wants it, and its path (the way up); landings sketched as outlines of any shape. Any run or landing of a made stair converted to a sketch and edited, and back while it is still a plain one. The risers drawn are counted against the height and the code like any stair's, and `stair_check` measures each tread on the walk line, so a sketched stair is reported, never refused. | Create Sketch (Boundary, Riser, Stair Path), Convert, Edit Sketch | |
| 11 | Solver and locks | `stair_solve`: lock what matters, get the stairs that fit the code, least change first. | Desired Number of Risers, and nothing more | Part done: `stair_solve` (`codes::solve`): the stairs nearest one that fit the code, least change first, over the count of risers, the tread the step rule allows, the least width and the landings due (the stair's own kept and lengthened, more where a flight is too long); `lock` keeps what it names; when none fits, the nearest with what each still breaks; `apply` makes one. The Building Code Check window lists them, each with Apply. Still to come: locks kept on the stair itself, the solver choosing winders' balance, and solving while a stair is being drawn |
| 12 | Multistorey | One stair across several levels, following them when they move, a landing at each floor. **Storeys of different heights**: each storey has its own count of risers and riser height inside the code, the going kept the same where it can be; the flights of a taller storey are longer or gain a landing, and `stair_check` reports a storey that cannot be made inside the rules, and how much two storeys' risers differ (the code's uniformity is within a flight, and a project may ask it between storeys). | Multistory Stairs > Connect Levels, which groups storeys of equal height and gives a stair of its own to one that differs | Part done: `multistorey` on a stair with a top level several levels up: it is the stair of its first storey (which is what is drawn, gripped, checked and solved) and stands again on each level on the way (`Model::storeys`, `stairs::storeys`), each storey with the risers its own height asks and the first's landings after the same share of them (one flight where a run would be left a riser). `stair_check` holds each storey's riser to the code and reports how far the storeys' risers differ (the code's uniformity). The plan of each level shows the storey standing on it; the 3D view, every storey; the Stair tool has a Height and Multistorey. Still to come: a storey edited apart from the others, a spiral, railings and IFC by storey |
| 13 | The plan symbol | Break mark on the slope, above and below per floor, arrows, tread numbers, per type. | Cut marks, stair paths and tread numbers, each an annotation of its own there | Part done: the break mark is a slanted line with a jog across the run the cut plane passes through, placed by height (so it is right after winders and on every storey); on a level a multistorey stair passes, the storey climbing from it is drawn to the break mark and marked UP, and past it the storey coming up from below is drawn as seen from above and marked DN, the two arrows meeting at the mark. Still to come: a break mark where the plane cuts winders or a spiral, tread numbers, what shows above the cut chosen per type or view, and the plan following the stair type (nosings, stringers) |
| 14 | Headroom and the opening | The headroom volume checked against floors and beams; the opening in the floor above cut from it. | None: a shaft is drawn by hand | |
| 15 | Railings | Balusters by tread or by spacing that stay whole across slope changes, rails continuous over landings and winders, heights from the code, both sides and the well. | Railing on Treads or Stringer | |
| 16 | IFC both ways | `IfcStair` of `IfcStairFlight`, landings, winder treads and railings with `Pset_StairFlightCommon`; import keeps a stair a stair. | IFC export of stairs | |
| 17 | Finish | The codes' numbers checked against the documents, the dialog in the project's units, translations. | | |

## Open questions

- **The codes' numbers are unchecked.** NBR 9050/9077, the IBC, DIN 18065 and Japan's Building Standards Law are in
  the data, typed from memory, one row each (the general occupancy). Someone with the texts must check them, and say
  whether a code needs a row for each occupancy (a dwelling, a school).
- **Is a stair's structure its type's, or may a flight differ?** Archicad allows it and its users report trouble with
  mixed structures; this plan says the type's.
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
