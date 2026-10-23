# The discipline tabs: design

A client asked for the complementary-project tabs Revit users know: structure (estrutural), plumbing and drainage
(hidrossanitário), electrical (elétrico), lightning protection (SPDA) and data and voice (lógica, dados e voz). This note
is how BIMCraft builds all five with as little new machinery as possible. All five are built, each a follow-up that added data
to what is described here, not new engine code. The rules in [CLAUDE.md](../CLAUDE.md) apply
to all of it (everything is a command, `Model::check` validates, the core has no UI, clean-room).

## What the five have in common

Look at what each tab draws and there are three shapes, and BIMCraft has two of them already.

| Shape | Examples | What builds it |
|---|---|---|
| A vertical or point thing with a type and a size | columns, footings; sanitary fixtures, panels, luminaires, outlets, racks, air terminals | `Column` (vertical members) and the families with `Component` (devices and fixtures) |
| A straight thing between two points | beams, strip footings; pipes, conduits, cable trays, down conductors, data cabling | **`Run`**, new |
| A slab or a region | structural floors, foundation slabs | `Floor`, with a `structural` flag |

So the new element is one: the run. Everything else is a category added to something that exists.

## (a) The run

`model::Run` is a straight element: from `start` to `end` in plan, at a height over its `level`, of a type with a size.

```
Run { type, level, start, end, offset, end_offset?, justify, system, params }
```

- **Height.** `offset` is the height, over the level, of the run's `justify` line (`top`, `centre` or `bottom`) at the
  start; `end_offset` is the height at the end, and when it is missing the run is level. A sloped run (a drain falling
  1%, a brace) is two offsets. A vertical run (a riser, a down conductor) has the same `start` and `end` and two
  offsets. Beams default to `top` justification, as engineers think of them (top of beam at the top of slab); the pipes
  and cables to come default to `centre`, the commands set it, not the type.
- **Section.** The type's `RunType { width, depth, round, material }`: a rectangle `width` across by `depth` deep, or
  round, `width` across. It is square to the run's axis, as a pipe's or a member's is.
- **Chains.** The command takes `points` and makes one run for each segment, as `wall` does. A fitting, an elbow or a
  tee is not a run: runs that meet butt against each other until the MEP slice adds connectors (ROADMAP 2.x).
- **System.** `system` is a name ("Água fria", "Circuito 3", "SPDA 1"). Nothing else about a system is stored: a
  schedule groups and filters by it like any field, and a fixture carries its system as a project parameter named
  `System`, which exists already. Systems are a name, not a registry, because a registry would need rules for renaming,
  deleting and merging that no tab has asked for.

In plan, a run or a footing is drawn from the level it hangs under: its top is flush with that level, so a beam placed at
the level is seen by that level's plan, which an element whose top is that low would not otherwise be (a plan sees down to
its view depth, which is the level by default); its centre line is dashed, to pick it by; one whose underside is over the cut plane and under the view range's top is
dashed, as a roof's eaves are; one that crosses the cut plane is cut, in poché. A section cuts a rectangular run across its
section, and the 3D view draws it as a sloped prism, or a round one as a tube.

A run is one Rust type serialized under several categories, as `Door` and `Window` share `Opening`. The category is the
`Element` variant, so Visibility/Graphics, schedules, project parameters, phases, view filters and the IFC class all
work per category without a line of per-category machinery. Today: `beam`, `stripfooting`, `pipe`, `conduit`, `cabletray`, `wire`, `downconductor`, `groundconductor` and `cable`. A tab adds a variant, a name
in `model::run_arm!` and a row to each table below:

| Category | Tab | Revit's name |
|---|---|---|
| `beam`, `stripfooting` | Structure | Structural Framing, Structural Foundation: Wall |
| `pipe` | Plumbing | Pipe |
| `conduit`, `cabletray`, `wire` | Electrical | Conduit, Cable Tray, Wire |
| `downconductor`, `groundconductor` | SPDA | (none: Revit has no lightning protection) |
| `cable` | Data | Wire / Cable Tray |

Types live in `Types::runs`, a map from category to type name to `RunType`, so a tab adds no field to `Types`. The model
starts with a few (`Beam 200 x 500`…), and the `type` command creates and changes them. Geometry is in `runs.rs`: the
centre line's ends, the footprint in plan, the prism or tube in 3D. The plan, the section, the 3D view, the snaps and
IFC call it and know no category. A rectangular run is a sloped prism, so sections cut it; a round one is a tube mesh, a convex
solid, which a section cuts by the plane through its line (`Solid::cuts`: where the tube's edges cross the plane, their hull, a
ring, an ellipse when the line is oblique). A component's forms, which are not convex, are shown beyond the line and never cut.

## (b) Points and verticals

- **Vertical members.** `Column` (a plan outline, a base, a height) is reused as it is, under two more categories:
  `structuralcolumn` and `footing` (an isolated pad, Revit's Foundation: Isolated, which is a column laid down: its
  `height` is its thickness and its top is at the level). Each has its own types (`Types::structural_column`,
  `Types::footing`, both `ColumnType`).
- **Devices and fixtures.** Placed with the Component machinery from families, by adding `family::Category` values: sanitary
  fixtures (`Plumbing`, exists), electrical equipment and devices, communications devices, lightning terminals. A
  family gives a fixture its forms, parameters and types, and a section never cuts it. A starter library of original
  families goes with each tab and is its own piece of work (ROADMAP 0.8).
- **Slabs.** A structural floor is a `Floor` with `structural: true`. Same category, so the same types, schedule and
  Visibility/Graphics, and IFC writes `Pset_SlabCommon.LoadBearing`.

## (c) Systems and schedules

Every category has an entry in `schedule::OWN`, so Structure gets Beam, Structural Column, Footing and Strip Footing
schedules from the `schedule` command and the Manage tab, with Type, Level, Width, Depth, Length, Offset, System and a
Count. A system's schedule is the schedule of its category, grouped or filtered on `System`: a pipe schedule by system
is a pipe schedule with `System` as a sort-and-group field. No second schedule engine.

## (d) IFC

IFC4 (ISO 16739-1:2018). Class names are checked against the IFC4 and IFC4X3_ADD2 schemas that ship with IfcOpenShell
0.9. Built is the Structure rows.

| BIMCraft | IFC class and predefined type | Built |
|---|---|---|
| `beam` | `IfcBeam` `.BEAM.`; braces `IfcMember` `.BRACE.` | beam (braces are a sloped beam of a brace type: not yet) |
| `structuralcolumn` | `IfcColumn` `.COLUMN.`, with `Pset_ColumnCommon.LoadBearing` | yes |
| `footing` | `IfcFooting` `.PAD_FOOTING.` | yes |
| `stripfooting` | `IfcFooting` `.STRIP_FOOTING.` (a grade beam, baldrame, is `.FOOTING_BEAM.`) | yes |
| structural `floor` | `IfcSlab` `.FLOOR.` (`.BASESLAB.` for a foundation slab) with `Pset_SlabCommon.LoadBearing` | yes |
| `pipe` | `IfcPipeSegment` `.RIGIDSEGMENT.`; fittings `IfcPipeFitting`, valves `IfcValve` | yes |
| sanitary fixtures | `IfcSanitaryTerminal` (`.NOTDEFINED.`: a family has one category and its forms, not a kind of fixture; `.WASHHANDBASIN.`, `.TOILETPAN.`, `.SINK.`, `.SHOWER.` would need one per kind); floor drains `IfcWasteTerminal` `.FLOORTRAP.` | yes |
| `conduit` | `IfcCableCarrierSegment` `.CONDUITSEGMENT.` | yes |
| `cabletray` | `IfcCableCarrierSegment` `.CABLETRAYSEGMENT.` (`.CABLELADDERSEGMENT.`, which only another tool writes, is read as a tray) | yes |
| `wire`, `cable` | `IfcCableSegment` `.CABLESEGMENT.`, which BIMCraft's `Category` tells apart | yes |
| panels, boards | `IfcElectricDistributionBoard` `.DISTRIBUTIONBOARD.` | yes |
| luminaires | `IfcLightFixture` | yes |
| outlets, switches | `IfcOutlet` (`.POWEROUTLET.`, `.DATAOUTLET.`, `.TELEPHONEOUTLET.`), `IfcSwitchingDevice` (`.TOGGLESWITCH.`) | power outlets and switches |
| data equipment | `IfcCommunicationsAppliance` `.NETWORKAPPLIANCE.` (a patch panel; a router, a switch would be `.ROUTER.`, `.NETWORKAPPLIANCE.`). A rack has no class: an `IfcBuildingElementProxy` of object type `Rack`, which a filter finds and an `IfcElectricDistributionBoard` would hide among boards | yes |
| `downconductor`, `groundconductor` | `IfcCableSegment` `.CONDUCTORSEGMENT.`, which BIMCraft's `Category` tells apart | yes |
| air terminals, earthing rods, inspection boxes | **no IFC class.** IFC4 and IFC4X3 have none for lightning protection. `IfcBuildingElementProxy` (`.USERDEFINED.`) with `ObjectType` `LightningTerminal`, `EarthingRod` or `InspectionBox`, so a reader can filter on it, and the family and type in its name. Surge protection has `IfcProtectiveDevice` `.VARISTOR.`; an `IfcDistributionChamberElement` `.INSPECTIONCHAMBER.` exists for the box but would hide it from a filter on the others | yes |

A run is written as an extrusion along its own axis (`IfcExtrudedAreaSolid`), so a reader gets the profile and the
length and not a mesh. Its system is an `IfcSystem`, named for it, that an `IfcRelAssignsToGroup` assigns the runs in it to;
`BIMCraft_Run` carries what has no attribute: the line its heights are at (`Justification`) and its `Category`, for the classes
that several categories share. One table, `runs::IFC`, gives the class and predefined type of each category, and import reads it
back (`runs::from_ifc`); the same for a family's category (`family::Category::ifc`, `from_ifc`).

## (e) The tabs

Each tab is a ribbon tab with the panels a Revit user expects, built with the `tools` helper the Architecture tab uses.
A tool is a `Tool` value, a click handler, a preview and a tooltip with its shortcut. Shortcuts are Autodesk's published
defaults where there is one; where BIMCraft has already given the letters to something else, the tool gets two others
and says so.

| Tab (EN / PT / ES) | Tools and shortcuts | Brazilian vocabulary |
|---|---|---|
| **Structure** / Estrutura / Estructura | Column CL, Beam BM, Floor (Structural) SB; Foundation: Isolated FI, Wall FW (Revit's FT is Filters here) | pilar, viga, laje, sapata, sapata corrida, baldrame |
| **Plumbing** / Hidrossanitário / Fontanería | Pipe PI, Plumbing Fixture PX, Pipe Fitting PF, Pipe Accessory PA (built) | tubulação, peça sanitária, ramal, prumada, água fria (NBR 5626), esgoto (NBR 8160) |
| **Electrical** / Elétrico / Eléctrico | Conduit CN, Cable Tray CT, Wire EW, Electrical Equipment EE, Lighting Fixture LF, Device DV (built) | eletroduto, eletrocalha, quadro de distribuição, luminária, tomada, circuito (NBR 5410) |
| **Lightning Protection (SPDA)** / SPDA / SPDA | Air Terminal AT, Down Conductor DC, Earthing Electrode EG, Inspection Box IB, Earthing Conductor GC (built) | captor, condutor de descida, haste de aterramento, caixa de inspeção, malha (NBR 5419) |
| **Data** / Lógica, Dados e Voz / Datos y Voz | Data Outlet DT, Voice Outlet VO, Rack RK, Patch Panel PN, Cable Tray CT, Cabling CB (built) | ponto de dados, ponto de voz, rack, patch panel, cabeamento estruturado (NBR 14565) |

The English stays Revit's words; the Portuguese uses the terms the standards use. Revit has no SPDA, so its tools have
no Autodesk shortcut, and take free letters.

## Not in this design, on purpose

Connectors and fittings, auto-routing, sizing, flow and load calculations, circuits and panel schedules, analytical
models, rebar, beam systems and trusses. Each is large, none changes the model above, and they come after the five
tabs can draw and schedule their systems (ROADMAP 2.x).

## What a tab adds

Structure was the worked example and the other four followed it; a tab is these steps and no engine code of its own. Each of
the four took a few lines in each of these places, and the tests are rows in two tables.

1. **Core, a run category.** An `Element` variant over `Run` and its name in `model::run_arm!` (every place that treats runs alike
   takes it from there), in `Element::category` and `of_run`, in `RUN_CATEGORIES` and `PARAMETER_CATEGORIES`, in `graphics::MODEL` and
   `paint::faces`; a row in `schedule::OWN`; its default types in `Model::new` (and a material or two in the library) and
   `Model::default_type`; a row in `runs::IFC` (its class and predefined type, both ways); a command with its shortcut (`chain` in
   commands.rs takes the category, so it is a few lines, a row in `ALL` and one in `command_of`). `runs::may_stand` already says a
   riser may stand; `runs::default_justify` is the middle.
2. **Core, a fixture or a device.** A `family::Category` value, in `PLACED`, with its `ifc` row (the class, the predefined type, and
   the object type a proxy carries where IFC has no class); `from_ifc` reads it back. Starter families in `fixtures.rs`, original,
   named through `language.rs`.
3. **Desktop.** A `Tab` variant and a `fn xxx_tab` in `mep.rs`: buttons for `Tool::Run(category, None)` and for the `FIXTURES`
   rows (a `FixtureTool` is a key, the kinds of family it places, its shortcut, its words and its icon); the title in
   `structure::title`; `properties.rs` (`icon`; the run rows and type rows already take any run category); icons in icons.rs; the
   family editor's labels in editor.rs; the Portuguese and Spanish rows in lang.rs (the words each standard uses).
4. **Tests.** A row in `CASES` (the run categories) and `FIXTURES` (the families) of `mep_tests.rs`, which make, refuse, move,
   draw in plan, cut in a section, show in 3D, snap to, schedule, hide, save, write to IFC and read back every category in them;
   and a row in `RUN_TOOLS` and `FIXTURE_TOOLS` of `tests/mep.rs`, which drive each tool, its options, its previews, its shortcut,
   its palette and Edit Type, its languages. The tab itself is a row in the tab and language tables there.

## Honest limits

- Runs that meet butt against each other: no mitre, elbow or tee, and no connectors, so a pipe network cannot yet be
  checked for continuity or sized. A fitting or a valve is a component of a family, placed by hand on a pipe's height; nothing
  says a pipe ends on it.
- A run's `system` is a name and an `IfcSystem`, with no rules: nothing checks that a pipe of "Esgoto" meets only pipes of it.
- A fixture or a device is a `component` to Visibility/Graphics and to schedules (the Component schedule has `Family` to group by),
  so a project cannot hide "Plumbing Fixtures" alone. Its family category is there for it; `Element` has no variant for each.
- A family has one category and no kind, so every sanitary terminal is `.NOTDEFINED.`: a basin, a WC, a sink and a shower are alike
  to an IFC reader.
- A conductor or a conduit under its level (an earthing ring, usually) shows in a plan whose view range reaches it (its View Depth).
- An inspection box is a proxy, so a filter on the other lightning kinds finds it; `IfcDistributionChamberElement`
  `.INSPECTIONCHAMBER.` is the real class. A patch panel is placed on the wall, not in a rack.
- A section cuts a tube by the plane through its line, which is exact for a convex mesh only; a component's forms are not, and are
  never cut.
- Braces are beams that slope; they will want their own type and `IfcMember`.
- Import finds a run's section as the box round its body in the frame the axis gives it, so a member rolled about its axis
  reads as the box. An upright run comes back running up with its width along east, whichever end it was drawn from.
- The Portuguese and Spanish words are the standards' as the author knows them; no native speaker has read them.

What the foundation's limits were, and is fixed: a section cutting a round run; the `IfcSystem`; a run's justification surviving
an import; Edit Type setting a run type's roundness and material.
