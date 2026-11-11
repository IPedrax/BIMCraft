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
- **Winders are always drawable.** The walk line is worked out for every code: at a fixed distance from the narrow side
  where the code says so (the IBC's 12 inches, Japan's 300 mm), along the middle otherwise. Where a code does not allow
  winders (NBR, on an accessible route or an emergency stair), they are drawn all the same and reported as against the
  code. A tool is never taken away because one code forbids what it makes.

## Pieces

Each keeps old files reading (a step in `Model::migrate` and a `FORMAT` bump where an older version would misread), adds
its payloads to the hostile sweep, and is looked at as [looking at results](areas/looking-at-results.md) says.

| # | Piece | What it adds | Tested through |
|---|---|---|---|
| 1 | Building codes and `stair_check` (in progress) | `data/building-codes.json` (generic, NBR, IBC, DIN, Japan), the project's code and its own rules (`building_code`, Manage > Building Codes), a new stair starting with them, `landings: "auto"`, `stair_check` listing what a stair breaks with the number and the fix, and the sign in a stair's Properties. | The commands' JSON on stairs that break each rule; the dialog |
| 2 | Stair types | A library entry: monolithic, stringers, carriage or cantilevered; tread and riser thickness, nosing, open risers; materials. 3D, sections and IFC draw them. | Pixels of a section; `IfcStairFlight` and `IfcMember` in the STEP file |
| 3 | The path | Runs of their own widths, turnings at any angle, a landing's depth and outline of its own, the stair to a side of its baseline, drawn up or down. Today's `landings` migrate to it. | Layout rings; a file of format N read as N+1 |
| 4 | Winders | A turning that winds: equal goings on the walk line, the least going at the narrow end, balanced into the flights by a chosen construction. | Goings measured on the walk line and at the narrow end |
| 5 | Curved runs | Arcs in the baseline: curved flights, spirals round a post, length along arc or chord as the rules say. | The same measurements on an arc |
| 6 | `stair_solve` and locks | The solver and the locked numbers. | Every answer passes `stair_check`; least change first |
| 7 | Multistorey | `top_level` several levels up, each storey its own flights; a landing at each floor. | Unequal levels; a level moved |
| 8 | The plan symbol | Break mark on the slope, above and below, the level above, arrows, numbering, per type. | Pixels of plans of both levels |
| 9 | Headroom and the opening | The volume, clashes in `stair_check`, `opening: true` cutting the floor above. | A beam in the volume; the floor's hole |
| 10 | Railings | Balusters by tread or by spacing that stay whole across slope changes, rails continuous over landings, both sides and the well of a multistorey stair. | Baluster positions; no part outside its panel |
| 11 | The desktop tool | Draw the baseline with the stair shown live and its findings beside it; grips for ends, turnings and widths; the solver's answers as a list to pick from. | egui_kittest: clicks at model points |
| 12 | IFC both ways | `IfcStair` of `IfcStairFlight`, landings and railings with `Pset_StairFlightCommon`; import keeps a stair a stair. | IfcOpenShell |

Pieces 1 and 8 are worth doing first: they change no file format and answer the two loudest complaints (rules that only
warn, and the plan symbol).

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
