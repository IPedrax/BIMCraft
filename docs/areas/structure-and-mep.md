# Structure and the discipline tabs (Plumbing, Electrical, SPDA, Data)

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

Structure (`runs.rs`, `structure.rs`; the design for the five discipline tabs is in [docs/disciplines.md](../disciplines.md)). A run
(`model::Run`) is a straight element from `start` to `end` in plan; its `offset` mm over its level is the height of its `justify` line
(top, centre or bottom) at the start, `end_offset` at the end (level when none), `system` the name of the system it is in: a beam, a
strip footing, a pipe (Plumbing), and what the other tabs draw (conduits, cable trays, down conductors, data cabling) as further
`Element` variants over the same struct, as `Door` and `Window` share `Opening`; `model::run_arm!` is the pattern for any of them, so a
place that treats runs alike names no category, and a new category is a line there.
Joins: runs of one category lying flat whose heights overlap meet as walls do — mitred end to end at any angle, one ending on
another's span stopping at its face (`runs::outline`, `runs::cap`, as `walls::cap` shapes a wall's; `runs::arms` finds the
neighbours through `walls::Index`'s tiles for runs, or among every run when no index is given), and a strip footing also stops at
a wall's face (a wall never changes for a run: its own outline knows no runs; the footing's top touching the wall's base is the
joint). Runs meet strictly in height, as two walls do, but a footing meets a wall by that touch. `join` is each run's and each
wall's Allow Join, on unless turned off: an unjoined run's ends stay square and it stretches for no one (`relocate` passes it by).
The outline is the one source for what every view draws — the plan's fill and seams, the seen-below and overhead pictures, the
snaps' corners, the 3D prism (and a round run's tube, cut by the same end planes, `runs::end_cuts` and `runs::clip_tube`, its
cap layer first) and the sections — so a joint is the same everywhere. Its line (`start`–`end`) never changes: a join only
shapes the outline. IFC still writes a run's section swept along its axis (a reader comes back to the line and section it was
given), so a joint shows in the plan, the 3D view and the sections, not in an exported body. A run's end may be dragged with
`move {end}`, one run alone, and the runs joined there follow as walls' do (an end on an end goes with it; an end on the span
slides along its own line onto the crossing). Moved whole, a run's end that lay on a neighbour's line runs on or is cut back
to their new crossing, as a wall's does, and the neighbour keeps its own axis and only stretches to it — whether the pair is
two runs of one category or a strip footing meeting a wall. At a corner where a wall and a strip footing end together, with
another element also ending there, the wall's end and the footing's go together too, as two walls' do; a footing simply
carrying on under a wall keeps its length, so a wall dragged along its own axis may be shorter than the footing it carries.
Glue: a strip footing under a wall sticks to it as Revit's does — its line the
wall's (same direction and place, within `TOL`), its top on the wall's base — and while that holds it rides the wall in plan
(`relocate` moves it by the wall's own transformation) and keeps its top on the wall's base when the wall's base moves
(`modify`, `glue::on_base`). Nothing is stored: the condition is read from the geometry (`glue`), so moving the wall or the
footing away lets go, putting them back glues again, and deleting the wall leaves the footing where it is. Only `stripfooting`
glues; a sloped one (an `end_offset`) doesn't; a footing changed by hand soon no longer meets its line or its base and lets go. Its type is `Types::runs[category][name]`, a `RunType`
(`width` across by `depth` deep, or round, in a `material` of the library). The section is square to the axis, so a sloped run is
`depth / cos θ` deep vertically (`Geom::half`). `runs::Geom` is the one place that knows the geometry (centre line, `footprint`,
`prism` for a rectangle, `tube` for a round one, `z_span`); plans, sections (a prism is cut like any solid; a tube by the plane
through the section's line, `Solid::cuts`: the hull of where its edges cross it, a ring, an ellipse when oblique), 3D, snaps and IFC call
it and know no category. A run's heights are those of its `justify` line, which a beam or a footing has at the top by default and
anything else at the middle (`runs::default_justify`); the desktop keeps each run tool's own justification and offset
(`keep_run_options`). A beam or strip footing can't stand or lean past 75°
(`runs::check`, `may_stand`: what stands is a column; a pipe, a conduit or a conductor may stand, a riser). In plan (`render::stands`) a footing or a run
hangs under the level it is drawn on, its top flush with the view's bottom, and is seen (`seen_below` allows `TOL` for these only,
or a wall of the storey below would show in the plan above); through the cut plane it's cut; over it and under the view range's top
it's dashed; a faint dashed centre line is drawn where it's seen or overhead, which is also where a click picks it. A structural column and an
isolated footing are `Column`s in their own categories (`Element::StructuralColumn`, `Element::Footing`; ask `Element::column()`,
`Model::column_type(e)` and `column_outline(e)`, which take the element, not the column) with types of their own
(`Types::structural_column`, `Types::footing`); a footing is laid down, its `height` its thickness and its `base_offset` putting its
top at the level (the command's `thickness` and `offset`). A floor is `structural` or not, the one flag, and its schedule and IFC say it.
`runs_of` and `stood_of` build what a command would make, so a tool's sketch is the element its click makes. The desktop's
`Tool::Run(category, start)` and `Tool::Stand(category)` serve every category: a tab adds a ribbon button and its rows in
`structure.rs`, `properties.rs` and `lang.rs`, not a tool. The Floor tool makes structural floors while `floor_structural` is set (SB,
the Structure tab's button; every other `set_tool` clears it); CL is the structural column on the Structure tab and the architectural one
elsewhere, as in Revit. IFC writes a run as an extrusion along its own axis (`Step::body_along`, `aim`) of the class and predefined type
the one table `runs::IFC` gives its category (import reads the same table back, `runs::from_ifc`), each structural element with
`LoadBearing` in its `Pset_*Common`, a run's system as an `IfcSystem` (named for it, its GlobalIds following the name) the runs in it
are assigned to by an `IfcRelAssignsToGroup`, its `Justification` and `Category` in `BIMCraft_Run`, and its material as an `IfcMaterial`. A beam, a column, a structural column and a
pad footing carry their section on their type, the IFC4 way (`Step::typed_member`, `write_types`): the `IfcBeamType`, `IfcColumnType` or
`IfcFootingType` is related by `IfcRelAssociatesMaterial` to an `IfcMaterialProfileSet` of one `IfcMaterialProfile`, holding the type's
material (a beam type's; a column type has none, so the material is `$`) and its profile, an `IfcRectangleProfileDef` of the width by the
depth or an `IfcCircleProfileDef` of half the width, in millimetres as every length is. Each occurrence is related to an
`IfcMaterialProfileSetUsage` of that set (one for each cardinal point, shared by the occurrences placed by it): a beam's cardinal point is
the line its heights are on, the top (8), the middle (5) or the bottom (2) of its section, a column's and a footing's the middle (5). The
beam has no other material association, so the importer reads a run's material through the usage (`element_material`). The profile
definition (`Step::section_profile`, keyed by `Section`) is written once for a section, and the type's profile and its occurrences' bodies
share it; two types of one name and two sections (a column's and a structural column's) are two type objects. A strip footing, a pipe and
the other runs have no profile set, only the plain material.
Import (`import::run_in`) finds a run's axis as the direction its body's points spread furthest in (power iteration on their
covariance), so a beam rolled about its axis comes in as the box round it; an axis that stands (a riser) has its width along east and
runs up; the justification BIMCraft wrote puts the heights back on the line they were typed at (`Geom::half`), none meaning the middle;
a system comes from the IfcSystem the product is assigned to, and its category from the class and type, or BIMCraft's `Category`
where a class has several; `LoadBearing` makes a column or a floor structural.

**Sizing** is a command with no model behind it, so it needs no connectors: `circuit` (`nbr5410.rs`, its tables in `nbr5410/tables.rs` as NBR 5410:2004 prints them: Tabelas 36 and 37 current capacity for methods A1 to D, 40 and 41 temperature and soil, 42 and 45 grouping, 47, 48 and 58) sizes one electrical circuit for copper: the design current from watts or VA, the demand and power factors and the phases at the voltage `system` it is given (it reads nothing from the model), then the smallest section and breaker with I_B ≤ I_n ≤ I_Z (which gives I_2 = 1.45 I_n ≤ 1.45 I_Z for the breakers of NBR NM 60898) and a voltage drop within the limit (4 %, from the conductor's resistance at its working temperature and 0.1 ohm/km), with the neutral and the protective conductor. A section or breaker already chosen is never made smaller. It does not check aluminium, trays (methods E and F), several layers, short circuit, harmonics or the conditions of a reduced neutral, and says so; the circuit of the model is the other mode of the same command (below). `conduit_fill` (`conduit.rs`, its catalogues in `conduit/tables.rs`) checks a conduit's fill: the conductors of a circuit (phases, neutral, protective; by outside diameter) over the conduit's inside, at most 53 % for one cable, 31 % for two and 40 % for three or more (NBR 5410), and the smallest size of a family that takes them. The dimensions are the makers' own (Amanco Wavin PVC, Elecon steel, Corfio flexible cable), each family naming its document, page and address; the inside diameter is derived (outside less twice the wall); a cable the catalogue lacks (EPR, above 240 mm²) leaves the fill pending, never guessed. Trays are not sized. `drain_size` and `vent_size` (`drainage.rs`) size sanitary drains and vents as NBR 8160 asks, by the fixture units (UHC) they carry, with the standard's own tables: Tabela 3 (the units and smallest drain of each fixture), 5 (branches), 7 (building drains and collectors by slope), 6 (stacks by the building's floors), 8 (branch vents), 2 (vent stacks and headers by the DN served, its units and the longest length of each vent DN) and 1 (a trap's longest run to its vent). A drain is never smaller than its fixtures' own drains or than one flowing into it (`min_dn`), and a building drain is DN 100 at least. They are stateless, so none of this needs a pipe network; a network that carries the units along the runs comes with connectors. `storm_size` (`storm.rs`) sizes storm drains as NBR 10844 asks: the flow Q = I·A/60 from a rainfall and a roof area, a downpipe by the inner diameter of the standard's Figure 3 abacuses (read curve by curve: the water depth in the gutter, the downpipe's length and a sharp-edged or funnel outlet; 70 mm inside at least; off the chart nothing is read) and a horizontal drain by Manning at two thirds full (n 0.011, which reproduces its Table 4 within 1 %; 0.5 % slope at least). `water_size` (`water.rs`) sizes a cold water pipe of PVC soldável as NBR 5626 asks: the design flow of the fixtures by the method the project chooses (weights, Q = 0.3 √ΣP, or every fixture at once), the smallest DN within 3 m/s, the Darcy-Weisbach head loss over the length and the equivalent length of the fittings, and the pressure left (10 kPa at least at a point of use). `reservoir_size` (`reservoir.rs`) sizes a building's water reservation as NBR 5626:2020 asks: the potable volume is `days` of the day's consumption (24 h the least, three days at most unless the project shows how the quality is kept), a fire reserve is kept apart and added, and the supply has to refill the day in 6 h (3 h for a single-family house). It checks the compartments of an elevated reservoir (two, a house excepted), the smallest against the greatest peak over a maintenance, two independent pumps and the spare of several towers. The standard gives no litres per person, no split between the lower and the upper reservoir and no Forchheimer diameter, so the consumption and the split are the project's (each marked a configurable assumption), the usual 60 % is offered only as a practice suggestion, and an input not given leaves its check `pending` instead of stopping the others; every check says which of four kinds it is (current requirement, historical reference, engineering practice, configurable assumption). `hydrant_size` (`hydrant.rs`) sizes hydrants and hose reels as IT 22/2025 of the CBPMESP asks: the system type and fire reserve of Tabela 3 by occupation column and built area, the flow of the two most unfavourable jets of Tabela 2's type, and a pipe's speed (5 m/s at most), Hazen-Williams head loss (Tabela 1) and the head needed at its start. `load_points` (`loads.rs`) lists what a dwelling's rooms must have as NBR 5410:2004 item 9.5.2 asks: one ceiling lighting point per room (100 VA up to 6 m², 60 VA more for each whole 4 m² above) and the general-use outlets by the room's kind, which its name says or `kind` gives (a bathroom 1; kitchen, pantry, service area and laundry one for each 3.5 m of perimeter; room and bedroom one for each 5 m; veranda 1; the others 1 up to 6 m² and by perimeter above), with their power (600 VA each for the first 3 points of a bathroom, kitchen, pantry, service area or laundry and 100 VA after, or for the first 2 by the alternative where those rooms have more than 6 points together; 100 VA a point elsewhere) on the points a room has. It reads the model's rooms (inside the walls' finish) or the rooms it is given, and changes nothing. From the model it counts the power outlets and the luminaires that stand in each room (`Rooms::component`: the room whose boundary at the walls' centre lines holds the component's room calculation point, else its insertion point, and whose height holds it too, so a device on a wall's face is in the room on that side; standing in the last phase, primary design options), so a room drawn with too few outlets, or with no luminaire, is not `ok`; where the model has no outlet, or no luminaire, at all, it takes the least instead, and `counted` says which it counted. It leaves out the exceptions to the lighting point and to a veranda's outlet, where the outlets stand, and the TUE, and says so. Its numbers are data, not code: `data/load-points.json` holds one rule set for each standard (NBR 5410:2004, the default and only one so far), each row naming its standard and item, and a test fails on one that names none or on a kind of room left without a row. `standard` picks the set and the answer names it and each room's source, so a country's rules come in as a new set of rows and the sum (`RuleSet` in `loads.rs`) stays the same. IEC 60364 leaves the count of a dwelling's outlets to national rules, which is why the sets are by country. `dps_check` (`dps.rs`) works out the surge protection (DPS) of a power line as NBR 5410:2004 asks: it is required where the line is overhead (or the installation has an overhead line) and the region is AQ2 (more than 25 days of thunderstorms a year), or in any AQ3 region (5.4.2.1.1), unless the project takes a calculated, only material, risk, which the standard never allows where people's safety or health may be at risk; every external signal line asks for it at the entry. When required it gives where it stands by its purpose (the line's surges, at the entry or the main board, or the meter box of an existing individual unit; direct lightning, at the entry), the connection scheme or schemes of Figure 13 by the earthing (TT, TN-S, TN-C, IT) and the neutral, the least discharge (5 kA for each mode, 20 or 10 kA neutral to PE of scheme 3) and impulse (12.5 kA; 50 or 25 kA there) currents, and the connecting conductors (4 mm² copper, 16 mm² against direct lightning, 0.5 m in all). With the earthing it gives the least continuous operating voltage Uc of Tabela 49 for each pair of points the device connects (1.1 Uo; Uo between the neutral and the PE; √3 Uo phase to PE in IT; U in IT without a neutral), in volts when the phase-to-neutral voltage `uo` is given. With `system` (the nominal voltage) it gives the impulse withstand of Tabela 31 by category of product (IV to I) and the most the protection level Up may be, that of category II (equipment of use); a signal line at the entry needs 1.5 kV at least (its note 3). It also gives the follow current between neutral and PE (100 A at least in TN and TT) and the short-circuit the DPS must withstand (6.3.5.2.4 e), and, with `before_dr`, scheme 3 only for TT (6.3.5.2.6 b). It leaves out the overcurrent device of the DPS and its coordination, and says so; it changes nothing. `demand` (`demand.rs`) works out an installation's demand `D = a + b + c + d + e + f + g + h + i` and the category of service it asks of its utility. NBR 5410 gives no demand factors, so the numbers are the utility's own and the command names its utility and the edition (`utility` is required; only CPFL is there, GED 13 version 2.24 of 18/09/2020, for CPFL Paulista, Piratininga, Santa Cruz and RGE; another is refused until its tables are added from its standard): lighting and outlets (Tabela 2 by built area, or declared above 250 m², Tabela 3; Tabela 18 for other buildings), showers, taps and irons (Tabela 4), boilers (5), dryers, ovens and dishwashers (6), stoves (7), window air conditioners (8, and 9 for commercial use), motors by rank (14, 15 and 10), special equipment at a power factor of 0.75 (11) and hydromassage (10). The category of Tabela 1A (127/220 V) or 1B (220/380 V) comes of the installed load up to 25 kW and of D above, with its breaker, cable, conduit and earthing conductor. The GED's own Example 3 is a test (46.1 kW, D about 33 kVA, C3). It leaves out the other columns of Tabela 1, the cables of 1D and 1E and the starting of motors (Tabela 13), and says so; it changes nothing. For `utility` ENEL (ET 1569 version 01, 29/01/2024; low voltage only) it works out no demand, since the specification leaves it to the designer (item 7.7.2): it frames the supply by the installed load in Anexo A (`network` aerial, 127/220 V) or D (underground, surface, 127/220 V) and gives the category, the power made available, the breaker, the connection and entry conductors, the earthing conductor and the measurement; where two categories hold the load it names both and `phases` picks one; a demand the designer gives is carried as it is. It also has Anexo C (`aerial_120_240`) and E (`underground_submersible`, which prints no power made available but the junction box), framed the same way, and Anexo B (`aerial_delta`, 120/240 V delta with a neutral), which goes by the designer's demand current (`demand_current_a`) and gives the smallest breaker that holds it, the row serving one, two or three phases (`phases`). It leaves out the conduits, the poles and the meter box of the annexes. For `utility` CELESC (N-321.0001, revision of December 2025) it works out no demand either: the standard classes a unit by its installed load (items 5.3.2.1 to 5.3.2.4) and leaves a demand, where a three-phase unit needs one, to the designer (Tabela 01, note 1). It gives the category of Tabela 01 for `network` 220/380 (A single-phase at 220 V up to 13 kW, B two-phase up to 25 kW, C three-phase up to 75 kW, with the demand printed beside it) or 220/440 (A, and M single-phase on three wires up to 50 kW), with the breaker, the phases and wires, the connection branch in copper and aluminium, the entry branch and the earthing conductor in copper and copper-clad steel; `phases` takes the supply an appliance asks for whatever the load, and a designer's demand above the one printed is said. It leaves out the conduits, the mast and the load branch of Tabela 01, Tabela 02 (220 V without a neutral) and the probable demand of a building of several units (N-321.0003). `dr_check` (`dr.rs`) says which circuits NBR 5410:2004 asks additional protection by a high-sensitivity residual-current device (30 mA at most) for, from the points each serves (`place`: bath, external, kitchen, laundry, service, garage, wet, dry; `kind`: outlet, lighting, fixed): every point where there is a bathtub or a shower; outlets in external areas; internal outlets that may feed equipment outdoors; in a dwelling every point of a kitchen, laundry, service area, garage or other wet internal area, but for lights 2.5 m up or more (outside a dwelling, only their outlets); outlets only up to 32 A. An IT circuit that must not be cut is left out. It gives each circuit its reasons, and judges the device it has (`residual`) against 30 mA. It takes its circuits as a list (it does not read the model's circuits yet) and changes nothing; it leaves out the further rules for the volumes of a bathroom and for swimming pools, and says so. Its rules are data, not code, as `load_points`'s are: `data/dr-check.json` holds one rule set for each standard (NBR 5410:2004, the default and only one so far), each rule a place, a kind of point, a building and its exceptions, with the standard's item, and a test fails on a rule that names no source or a place or kind the command does not know. `standard` picks the set and the answer gives each reason's source, so another country's rules (IEC 60364-4-41 and BS 7671 ask it for any outlet up to 32 A, whatever the room) come in as a new set of rows and the check (`RuleSet` in `dr.rs`) stays the same. `bath_volumes` (`bath.rs`) takes the further rules for a bathroom, NBR 5410:2004 item 9.1: given the tub, tray or recess in plan (or the outlet of a shower with no tray), the floor one stands on and the rim, it says which volume each thing it is given stands in (0 the inside up to the rim; 1 over it up to 2.25 m, or 0.6 m round a shower with no tray; 2 out 0.60 m more, up to 3 m, so over volume 1 too; 3 out 2.40 m more, up to 2.25 m), the least IP code there (IPX7, 4, 3, 1; IPX5 in volumes 2 and 3 of a public bathroom) and whether an outlet, a switch, a box, a luminaire, a heater or other equipment may stand there by its class (I, II, III), how it is fed (mains, SELV and its voltage, its own separating transformer) and the residual-current device that protects it. Its numbers are data too, `data/bath-volumes.json`, every volume and every thing it allows naming its item, so the zones of IEC 60364-7-701 or BS 7671 section 701 (0 to 2, other reaches) would be a set of their own rows. The volumes are measured straight: the fixed walls and partitions they go around, the space under a tub, the door of a prefabricated cabin, the wiring and the supplementary bonding are not seen, and it says so. It takes its things as a list, reads nothing from the model and changes nothing.

**A circuit** (`Element::Circuit`, `electrical::Circuit`, the `circuit` category) is devices on a distribution board behind one breaker: its `panel` (a board, by GUID), `number` (its own on that board, the next free unless said), `name`, `rating_a` (the breaker, amperes; a standard one for the load unless said), `poles`, `voltage` (the name of a voltage of the project that the board's system supplies; unset, the one its poles imply there: line to neutral for one pole, line to line for more), `classification` (its devices'), `demand_factor` (above 0 up to 1), `devices` (components, by GUID) and `wires` (`hot`, `neutral?`, `ground?`, each a section in mm² of a wire type of the project). `visit_refs!` lists its panel and devices and `lib_refs_mut` its voltage and wire types, so a file holds GUIDs and a rename follows. A device draws what its family's **power connector** says for its type (`Family::connector_values`: voltage, poles, load in VA and in each pole, power factor, classification), so no load is typed on a circuit or a device. `Model::check` (`electrical::check_circuits`, one place) holds that the board has a distribution system; that the breaker is positive and not above the board's mains; that the poles are 1 to 3 and no more than the system's phases, and that the board's circuits take no more poles than its `slots`; that each member has a power connector, **has as many poles as the circuit and draws a voltage inside the band of the circuit's** (the refusal names both voltages, the device and the board's system); that a device is in one circuit only (naming the one that has it); that numbers on a board are distinct; and that each conductor's section is a row of its wire type's table (the refusal names the table and its sections). Its load is derived and never saved (`electrical::circuit_totals`): installed VA is the members' sum, the load of each pole their per-pole sums, the current VA over the voltage (over √3 times it for three poles, balanced), demand VA the installed times `demand_factor`; `element_json` says them (`installed_va`, `demand_va`, `current_a`, `pole_va`, `voltage_name`, `voltage_v`, `panel_id`, `device_ids`), `inspect` lists the circuits, and a board's `element_json` and the `panel` listing carry the totals of its circuits (`electrical::board_totals`, whose current is the demand's, balanced over the system's phases; A/B/C per phase wait for a phase on each circuit, which the load schedule gives). `Model::warnings` says a circuit with no device (a spare) and a breaker its hot wire cannot carry. `circuit` is **two commands in one id**: given a key of a circuit of the model (`devices`, `panel`, `id`, `add`, `remove`, `name`, `number`, `rating`, `poles`, `voltage`, `classification`, `demand_factor`, `wires`, `delete`, `list`) it makes, changes, adds devices to, takes devices out of, deletes or lists circuits (`modify` merges fields as for any element); given a key of the NBR 5410 sizing, or none, it sizes, as it always did and with no undo step (a command makes a step only when the model changed); keys of both are refused. `delete` takes a deleted device out of its circuit (a spare remains) and a deleted board's circuits with it; `copy` skips circuits, and refuses a selection of nothing else; Insert from File brings a circuit with its board and devices; a linked project's circuits are not brought. A circuit is an element of file format 18. **Its `number` is its slot** (`electrical::slots_of`): a circuit of `p` poles takes the slots `number`, `number`+2, … (Revit's boards have odd slots on the left and even on the right, so the next pole is the next slot down the same side), each on a phase (`slot_phase`: a row of two slots is one phase, A B C repeating over the system's phases). `check_circuits` refuses two circuits on one slot and a slot past `Max #1 Pole Breakers`, naming the slot and the circuit; a new circuit with no `number` takes the first free run that fits (`electrical::free_number`). **IFC** writes each circuit an `IfcDistributionCircuit` (electrical; the circuit's GUID is its GlobalId) assigned to its devices by `IfcRelAssignsToGroup` and aggregated under its board's `IfcDistributionSystem`, its breaker an `IfcProtectiveDevice` (`.CIRCUITBREAKER.`, no geometry, `Pset_ElectricalDeviceCommon` with `NumberOfPoles` and `RatedCurrent`) and the rest in `BIMCraft_Circuit` (`Number`, `Rating`, `Poles`, `DemandFactor`, `Panel` GlobalId, `Voltage`, `Classification`, `Wires` as JSON); import finds the board by that GlobalId and the devices among the group's members, and leaves a circuit out, with why, when it has no set, no board, no number, rating or poles, or names a voltage the project lacks (an unknown wire type drops only the wires). **In plans** a circuit is drawn at its board's level, in the `circuit` annotation category of Visibility/Graphics: a dashed home run from its first device toward the board (800 mm at most, never past the middle) ending in an arrow, and a tag with its number; both are picked with the circuit. **On the desktop** a selected circuit has an Electrical group in Properties (`circuit_parameters`: number, name, rating, poles, voltage, classification and demand factor, through `circuit`; its panel, load, current and devices shown), and Electrical > Create Circuit (`create_circuit`) makes one from the selected board and devices.

**Electrical settings** (`electrical.rs`) are the tables circuits, panels and connectors name, as Revit's Electrical Settings: `Voltage` (`nominal`, and the band `min` to `max` a device may read within), `DistributionSystem` (`phases` 1 to 3, `wires` the line conductors and the neutral and not the protective one, and the voltages it supplies by name: `l_l` between lines, `l_n` to the neutral, `l_g` to earth, and `ground`, how it is earthed as IEC 60364-1 names it) and `WireType` (`material`, `temperature`, `insulation`, a `note` saying where the ampacities come from, and `sizes`, a row for each section with its `ampacity`). Every section is in mm² whatever the project's units; a size a standard gives in AWG or kcmil is converted and keeps its name as `label`. Each is a map of `Model` (`voltages`, `distribution_systems`, `wire_types`), a library entry (`Kind::Voltage`, `Kind::DistributionSystem`, `Kind::WireType`, so each has an id and a GUID, comes in with Insert from File and is renamed with `rename_entry`), and has a command that makes, edits, renames and deletes it and, without a `name`, lists it (`voltage`, `distribution_system`, `wire_type`); `inspect` lists all three. A system names voltages, so `Model::lib_refs_mut` hands them out (a file holds their GUIDs, a rename follows) and `Model::check` refuses a system that names one the project lacks, saying which and listing the ones it has, and `voltage` refuses to delete one a system supplies; with three phases `l_l` must be about root three times `l_n`. `Model::voltage_range`, `system_voltages` and `wire_ampacity` are what circuits will ask: the band of a voltage, the voltages a system supplies (the ones a circuit on it may have) and the ampacity of a section in a wire type's table. A new project starts with `data/electrical.json`, data and not code: 230/400 V, 127/220 V, 220/380 V and 277/480 V, and copper wires of NBR 5410:2004 Tabelas 36 and 37 (PVC 70 °C and XLPE or EPR 90 °C, seeded from `nbr5410/tables.rs`: method B1, two loaded conductors, a test keeps them equal) and of NFPA 70 Table 310.16 (THHN 90 °C, THW 75 °C); their ampacities are the contributor's and unchecked, and one number for a section cannot say how a wire is laid, so a wire type's `note` says. `start_system` and `start_wire` name the ones a new panel or circuit starts with for the project's units (Europe's in metric, North America's in feet and inches). None of it goes to IFC: the tables are project data, and what they produce (voltage, current, section) will go in the device's property set once circuits exist. **A distribution board** (a component of the equipment category) holds `Component::panel`, `electrical::Panel`: its `distribution` (a distribution system of the project, a library reference by name that a file holds by GUID, renamed with the system and refused for deletion while a board is on it), its `mains` (amperes), its `slots` (`Max #1 Pole Breakers`) and `supply_from` (another board, by GUID, so `visit_refs!` lists it and Insert from File brings the board that supplies it). It is not a family parameter, which holds only a length, a number, an angle, a yes or no or a material, and a board's `12 circuits`, `24 circuits` and `36 circuits` stay its size, not its slots. `panel` sets it by the parameters' names, `""` takes one away, and with no `id` lists the boards with the ones each feeds; `element_json` says it flat (`distribution_system`, `mains`, `slots`, `supply_from`), which the component schedule's fields `Distribution System`, `Mains`, `Max #1 Pole Breakers` and `Supply From` (the id of the board) read. `Model::check` (`electrical::check_panel`, with each element, so `check_changed` sees a board, what it names and what names it) holds that only equipment has it, that the system is one of the project's, that mains and slots are in range (`MAX_MAINS`, `MAX_SLOTS`), and that the board that supplies it is equipment, never the board itself and never one it feeds however far down (`electrical::supply_loop`, which names the chain); `delete` leaves the boards a deleted board fed without a supply. Boards make a tree, and the totals (installed and demand load, current of each phase) are sums over a board's circuits, so they are worked out where circuits are and never saved. IFC writes each system a board is on as an electrical `IfcDistributionSystem` (once, whatever the number of boards; `IfcRelAssignsToGroup` to its boards, `IfcRelServicesBuildings` to the building) and the rest in the property set `BIMCraft_Panel` (`Mains`, `Slots`, `DistributionSystem`, `SupplyFrom`, a GlobalId, which is the board's GUID); it reads them back, a board another tool put in a system taking its name from the group, and leaves out with why a system the project has no table for, a supply the file has no board for, and a supply that comes back to its board (broken at the first board of the loop, the rest of the tree kept). On the desktop, an Electrical group of the Properties palette (`properties.rs`, `panel_parameters`) edits it through `panel`.

**A panel schedule** (`Element::PanelSchedule`, `panel_schedule.rs`; file format 18) is a distribution board's load schedule, the quadro de cargas, **a view of its own** and not a `schedule` (a table of one row for each element of a category): it has sections (a header, the slots, a summary, notes) and is edited through the circuits it shows. It holds the board's GUID (`visit_refs!`; it goes with the board, as its viewports go with it) and its options: `template` (`branch`, Revit's two columns with the odd slots left and the even right and the phases beside them, or `list`, a row for each slot with classification, voltage and demand factor), `show_spare`, `show_space`, `phase_columns`, `scale` (how large it comes out, 0.25 to 4) and `notes`. `panel_schedule::layout` works out everything it says as data, which the sheet and the desktop draw: the header (board, system, voltage, phases, mains, level); a row for each slot (`Kind::Circuit`, `Continues` for the later slots of a multi-pole circuit, `Spare`, a breaker with no device, and `Space`, a slot no circuit takes); and the load summary by classification (the project's `demand_factors` applied over each circuit's own `demand_factor`, so a class counts the circuits' demand VA times its factor) and by phase (each pole's load falls on the phase of its slot: the phases add up to the board's load, and a phase's current is its demand over the phase voltage, line to neutral; the board's estimated current is `board_totals`'). `demand_factors` (`Model::demand_factors`, class to factor above 0 up to 1, empty to start with: no standard's numbers are bundled) is the table; `board_totals` and the `panel` listing apply it too. The `panel_schedule` command makes and changes one, edits a circuit in it (`circuit`, with `description`, `breaker`, `phase` (the first free slots that start on it), `demand_factor`, `slot`, through `circuit`'s own rules, so `Model::check` and the rollback are `circuit`'s) and marks a slot a spare (an empty circuit) or a space (an empty circuit taken away; one with devices is refused); `place` puts it on a sheet like any view and it prints with it (`sheet/panel.rs` draws the tables from the layout: a space is struck through and a spare lettered grey; `inspect` lists it as `panel_schedules`). On the desktop (`panelsched.rs`) Electrical > Panel Schedule makes and opens the selected board's, the Project Browser lists it under Schedules/Quantities > Panel Schedules and Place View offers it; its slots are one list (the branch template's two sides are for the sheet), a circuit's description, breaker and demand factor are edited in the cell, a slot picked is marked or moved to a phase from the bar above, and the template and what it shows are options in that bar.

On the desktop, Manage > Electrical Settings (`ui-egui/src/electrical.rs`) is one dialog with a tab for each table: rows to add and delete, a voltage chosen from the project's own for each line of a system, and a wire type's table of sections below its fields. It is a draft, applied by OK or Apply as one undo step (`Session::transaction`) through the three commands, in an order the core accepts (`renames::plan`, which the other Manage dialogs use, orders each table's renames and deletes; the voltages are made before the systems that name them and taken away after the systems that no longer do). A system's voltages are held by their rows' keys, so one renamed in the dialog is still the one named. A blank or repeated name, a voltage a system supplies being deleted, and what the core refuses are said inside the dialog, which stays open. Not yet: Revit's tiered demand factors, and a panel schedule's own templates beyond the two.

The discipline tabs after Structure add data to this, no engine code of their own. Plumbing: `pipe` (PI), types in PVC and copper
(`Types::runs["pipe"]`, outside diameters, so a pipe's `width` is its diameter and its schedule says Diameter). Electrical: `conduit`
(CN), `duct` (DU: a rectangular or round duct, written as `IfcDuctSegment`; only a Revit import makes them in practice) `cabletray` (CT, `cable_tray`: the one rectangular run) and `wire` (EW), by their outside size. Lightning protection (NBR 5419):
`downconductor` (DC, may stand: a riser down a wall) and `groundconductor` (GC), two runs of one IFC class that `BIMCraft_Run`'s `Category`
tells apart; an air terminal, an earthing electrode and an inspection box are components of their own family categories, written as
`IfcBuildingElementProxy` of the object type `Category::ifc` names, so the product's name (not its object type) holds `Family:Type` and
import reads it from there. Data (NBR 14565): `cable` (CB, `cabling`), a run like a wire, so a file's `IfcCableSegment` is a wire unless its
`Category` says cable; data and voice outlets (`IfcOutlet` `DATAOUTLET`, `TELEPHONEOUTLET`; one of no type reads as the power outlet,
`Category::from_ifc` taking the first of equals), a patch panel (`IfcCommunicationsAppliance`) and a rack (a proxy of object type `Rack`);
the Cable Tray tool is the Electrical tab's, on the Data tab too. A fixture or a
device is a component of a family whose `family::Category` says what it is (a sanitary fixture, a floor drain, a pipe fitting, a pipe
accessory…); `Category::ifc` is its IFC class, predefined type and, where IFC has no class (lightning protection), the object type a
proxy carries, and `Category::from_ifc` reads the same table back, so a category is one row, in `family.rs`. A family's `kind`
(`Category::kinds`: `wc`, `lav`, `sink`… for a sanitary fixture, `floor_drain`… for a drain, `bend`, `tee`… for a fitting, the names the
sizing commands count fixtures by) says which one it is, and `Family::ifc` writes that kind's predefined type in place of the
category's `.NOTDEFINED.`; the import reads it back with `Category::kind_of`, the first kind of a predefined type two share. The starter families are
`fixtures.rs` (original forms, named in the project's language; a fitting or a valve is centred on its pipe, along x, so placing it at the
pipe's height lines them up; a wall device, a board and a luminaire carry their `Mounting Height` in their type, so the Fixture tool
places them at the level and they stand where they hang). The desktop's `mep.rs` has one tool for every fixture and device, `Tool::Fixture(key)`: the families of the
kinds its `FixtureTool` names, listed in its Type Selector and placed as the Component tool places them (`placing` takes the tool's
kinds), and the tabs' ribbon panels; a fixture's two-letter shortcut is in `mep::FIXTURES`, not `SHORTCUTS`, since no command makes
one. Edit Type of a run type also sets whether it is round (a diameter, no depth) and its material.

Reinforcement (`rebar.rs`, `model::Rebar`, `Element::Rebar`, category `rebar`, the `rebar` command; tests in `rebar_tests.rs`). A bar is one
element: a round bar of `diameter` mm (3 to 100) bent along `path` (points in mm, `z` over its level), of steel `grade` (CA-50 unless
told), with the `host` it reinforces (a beam, a column, a footing or a floor, held as a `Guid`, and it goes when its host does). A hoop
is a bar whose last point is its first; a layer of bars, or hoops along a beam, is one command with `count` and `step`, which makes one
element for each (IFC has one `IfcReinforcingBar` for each, and a bar schedule counts them). It has no type: the diameter and the grade
are its own, and the bends are sharp, with no hooks, so its length is the sum of its pieces (`rebar::length`) and its weight that
length of steel at 7850 kg/m³ (`rebar::weight_kg`). Nothing checks that it lies inside its host: `reinforce` places bars inside by construction. The schedule of category `rebar` lists Id, Level, Grade, Diameter,
Length, Count, Weight and Host, and grouped by Grade, Diameter and Length with Count and Weight totalled it is a bar schedule. IFC
writes each as an `IfcReinforcingBar` (`SteelGrade`, `NominalDiameter`, `CrossSectionArea`, `BarLength`) with a body of an
`IfcSweptDiskSolid` along an `IfcPolyline`, aggregated (`IfcRelAggregates`) to its host, or contained in its storey when it has none;
import reads the same back, and also a disk behind an `IfcMappedItem` and a circle extruded straight, with the host found by the
aggregation, while a body of disks of several radii, of disks that do not meet end to end, or of several separate bars is left out. The 3D view draws a tube along each straight piece
(`rebar::tubes`, built with a round run's tube), which the concrete round a bar hides: look at bars with their host hidden. A section draws each bar it cuts as a
dot over the host's poché (`render::section_in` keeps them out of the host's pieces, which would swallow them); a plan draws them too (`render::bar_in_plan`).

Importing a structural program's IFC (Eberick, Cype and the like; tests in `eberick_tests.rs`, written by hand, never from anyone's project). Each
product is named by its mark, so a name that is not its type's (BIMCraft names a product by its type, and writes it as its object type too) is its
`Mark` parameter, for the categories in `import::MARKED` (beams, columns, structural columns, footings, strip footings and floors; not stairs,
which BIMCraft calls "Stair"). The single values of every property set that is not BIMCraft's (`BIMCraft_*`, `RoomSeparation`; `Project Parameters`
is read by its names as written) are project parameters named for the property, a character a formula can't say turned into a space
(`parameter_name`: "Cargas de parede (G2)" is "Cargas de parede G2"), lengths in the file's unit made millimetres; `LoadBearing` is left out because
it already makes a column or a floor structural. The cover Eberick sets (`Cobrimento`, `ConcreteCover`) is therefore on the host as a length, for the
rules that will arm it. A run is found in its body as the way it spreads furthest, except where the body is one extruded profile and the extrusion is
not up: then it runs along the extrusion (`Reader::extrusion`), because a beam between two columns can be shallower than it is deep, and its body then
spreads furthest up. A strip footing, a plan outline stood up, is still read by its shape. A landing slab that belongs to no stair is a floor.

Bars by rules (`reinforce.rs`, the `reinforce` command; tests in `reinforce_tests.rs`). The engineer says how many bars, how thick and how far
apart; the module lays them out in the host's own frame (`Frame`: along, across, up) inside the cover and checks that what is asked fits, with the
message saying what to change. It holds geometry and no standard's table: a spacing or a count is never decided here (that is design), and the
clearance between bars (`clearance`, their thickness and at least 20 mm unless told) is the only rule it brings. Parameters are read with serde types
that deny unknown fields (`Row`, `Pitch`, `Stirrup`, `ColumnBars`, `Mesh`, `Grid`), by key, so a key that is not the host's is refused with the host named.
The cover is `{top, bottom, sides}` (Revit's faces: the top and bottom faces of a beam, a footing or a floor, the ends of a column, and all round
the rest) or one number, and where none is given the host's `Cobrimento` or `ConcreteCover` parameter (a length in mm, as the import of an Eberick IFC
leaves it) is the cover. A spacing is the most between bars: `spread` and `stations` place them evenly from end to end, so the count follows the length
(Revit's Maximum Spacing) and `ends` gives closer stirrups in the first and last `length` mm. A stirrup is a closed loop; with `hook` it runs on past
its first corner to a tail turned into the section at each end (`stirrup`). A beam has `bottom` and `top` rows, `stirrup`, `skin` and `anchor`; a
column `bars` (`x` and `y` along each face, corners counted; a round one, `count` round its circle), `stirrup` (a round one's a polygon of 24 sides), `ties`
(across a rectangular section; with `hook` a tail at each end turned toward the other axis) and `anchor`; a footing a `mesh` of two layers with `hook`
turning their ends up (a round one's bars stop at its circle, `half` in `footing`); a floor `bottom` and `top` meshes (`hook` turning their ends up from the
bottom one and down from the top one, square to the slab's top) whose bars stop a cover short of its outline and of its
openings (`chords`, a scan line through the outline and the openings together; a bar parallel to an opening's edge and nearer it than the cover keeps
its cover from the ends only). A sloped floor's layers are measured from its top face, square to it (`off`, and its secant for the height plumb), so the
cover and the thickness are the type's and its bars follow the slope; a mesh on a slope is one plane's bars. Round beams and strip footings are refused. Each bar is a `rebar` element of the host with a `layer`
(`bottom`, `top`, `skin`, `stirrup`, `bars`, `tie`, `mesh x`, `mesh y`, `bottom x`, `top y`, ...); reinforcing a host again replaces its bars that have a layer
and leaves the ones with none, which were drawn by hand, and one command makes up to `rebar::MAX_BARS`. A bar's layer is in the project file and the
schedule (a field a bar schedule may show), not in IFC. What it does not do: bends other than a hook, laps and splices, bars of a round beam, and any check
against a standard. A section shows a bar in its plane as a strip along it and one it crosses as a dot (the cut of the bar's tube, `section_in`); a bar
beyond the cut stays hidden by the concrete it is in.

More of `reinforce`. `hosts` arms a list (`rebar::MAX_HOSTS`, 1000) alike in one command: every host's bars are made before any is replaced, so one that refuses (its refusal names its category and id) arms none, and `rebar::MAX_BARS_ALL` (20000) caps all of them
together. A `hook` on a beam's `bottom` and `top` rows is the straight leg in mm turned up from the bottom bars and down from the top ones at each end
(a number for both, or `{start, end}`), and on a column's `bars` a leg turned into the section along the way across the face the bar is on
(`{below, above}`); `legs` reads it, with its own words when it is not a length, and a leg that does not stay inside the stirrup (a column's: stop
short of the middle) is refused. The bends are sharp, as every bar's are, and each leg is a piece of the path, so the length and the weight count it.
A `reinforcement_preset` is `reinforce::Preset`, a kind of host (`KINDS`) and the parameters that kind takes, in `Model::reinforcement` and as a library
entry (`Kind::ReinforcementPreset`, so it has an id and a GUID, comes in with Insert from File and is renamed with `rename_entry`); it is checked
for its keys and its size when saved (`Preset::check`, `MAX_PRESETS`) and against a host only when it arms one. `reinforce` with `preset` takes its parameters
and lets any given with it replace them, key by key; a preset of another kind than the host is refused, saying both. Plans: a bar of the plan's level
is drawn from above, solid when the cut plane is over its lowest point and dashed when it is over the plane and under the view range's top, a bar
that stands (a column's) as a dot, and bars that lie on one another (a column's stirrups, one over the next) once.

Bends beyond a hook. `bottom.bent` `{count, angle?, top?}` (`Bent`, `bent_up`) takes `count` of a beam's inner bottom bars (the two at the corners hold the
stirrups and stay straight; the others are picked evenly) and bends them up at each end: along the top face for `top` mm from the end (a fifth of the
length unless told), down a slope of `angle` degrees (15 to 75, 45 unless told) to the bottom row's level and along it. They are on the layer `bent`,
have no hook, count as bottom steel in the check, and are refused where the slopes meet in the middle, where the section is too shallow to rise
in, and where one comes within the clearance of a top bar at the top. `bars.jog` `{by, from_top, slope?}` (`Jog`, `step`, `stepped`) steps a column's
bars `by` mm toward the axis (a corner bar along both faces, a face bar across its own, a round column's radially), the step starting `from_top` mm below
the top and running `slope` mm up for each mm across (6 unless told); the points of the bar above the step move with it, including a hook's leg, and
the bars at their new places are checked against one another for the clearance.

The check (`reinforce/norm.rs`). `check` `"NBR 6118"`, with `aggressiveness` (1 to 4) and `fck` (MPa), makes `reinforce` return `notes`: where the bars
made fall short of the usual limits for the kind of host (Table 7.2's cover by class; for a beam the 4 % most, the 0.15 % least up to C30, skin bars over 600 mm
deep, stirrups of 5 mm to a tenth of the width at most min(0.6 d, 300) apart; for a column bars of 10 mm to an eighth of the side, 4 bars or 6 round,
centres at most min(2 sides, 400), 0.4 to 8 % of steel, stirrups of 5 mm or a quarter of the bars' at most min(200, the side, 12 bars) apart; for a floor
bars of at most an eighth of the thickness at most min(2 h, 200) apart). A note never refuses: an engineer may know better, and the bars are made.
It reads only the bars made, so a limit that needs shear, the load or the aggregate is not in it. The numbers are typed from handbooks and course material
that print them, not from the standard's text, which is paid for, and the item numbers are the 2014 edition's at the level of the chapter's item: check
one against a copy before relying on it. The desktop's Check row (Standard, Aggressiveness class, fck) shows the notes inside the dialog, which stays
open, with the bars made, so a field can change and OK remake them.

The Reinforce dialog (`crates/ui-egui/src/reinforce.rs`, Structure > Reinforcement, RF; tests in `tests/reinforce.rs`). It opens on the one beam, column,
structural column, footing or floor selected (anything else says what to select), with the form of its kind (`BEAM`, `COLUMN`, `FOOTING`, `FLOOR`: tables of
groups of fields, each field the path of the parameter it sets, such as `bottom.count`). Every field is text: a count is a whole number, a bar size is mm,
a length is read in the project's units (`Units::parse`); a field left empty is not sent, a group with none filled is left out, and the command is the one
that says what is missing. OK builds the parameters and runs `reinforce` as one undo step; a refusal is the command's own words, shown inside the dialog
(a status box in the view's corner could lie over its buttons) with the dialog left open and the project unchanged, and a field that is not a number is
named before the command is asked. It takes any number of elements selected, of one form (a mixed selection says so), and arms them with one `hosts` command; Remove Bars removes the bars of all. What was typed is kept for the next element of the same form (not in the project, not on disk). The Preset row lists the project's presets of the form: choosing one fills the fields (`ReinforceDialog::load`, the reverse of `params`, lengths in the project's units), Save Preset keeps the fields as they are under the name typed (`reinforcement_preset`, the parameters without the hosts), and Delete Preset takes the chosen one away; each keeps the dialog open. Remove Bars, shown
when the element has bars the command made (those with a layer), deletes them and leaves the ones drawn by hand. The fields' accessible names are
"Group: Field" (`Bottom bars: Count`), and the cover's is `Cover`, its label having a colon so that no two nodes carry the name.

**Connectors.** A family's `connectors` (`family::Connector`, a list for any discipline; today the electrical device's) say what
a placed device draws: `kind` `power` or `switch`, `system` `power-balanced`, `power-unbalanced` or `switch`, `poles` (1 to 3),
`voltage`, `load_va`, `power_factor` and a `classification`. The numbers are numbers or formulas over the family's parameters, so a
type sets its load (the starter luminaires' `Load` parameter) and an instance parameter lets one component set its own; a
`power-unbalanced` connector's `load_va` may be a list, one load for each pole, and a single number is shared out evenly.
`Family::check` refuses a family of two `power` connectors, a kind and system that don't go together, poles, voltage, power factor
or load out of range for any type, and a load on a switch. A connector is a part (`HasPart`: an id and a GUID, kept by place
while the family is edited), and a family without any reads and places as it always did. A component's `element_json` carries
`connectors` worked out for its type (`Family::connector_values`: `load_va` in all, `load_va_by_phase`), which a circuit will sum;
there are no fitting-to-pipe joins yet. `Model::warnings` (shown by
`inspect`; `Model::check` stays an error only) says which lighting, outlet or switch family in use holds no connector, so its devices
count as zero load. IFC writes each device with a connector `Pset_ElectricalDeviceCommon` (`NumberOfPoles`, and for a power
connector `PowerFactor` and `RatedVoltage`) and `BIMCraft_Connector` (`System`, `Classification`, `LoadVA`, `LoadVAPhase1`…); IFC
import reads them back into a connector on the family (a `Load` parameter for a balanced load, `Load L1`… for an unbalanced one, one
value for each type) so a circuit's devices keep their load through a round trip.

**Connectors with a place** are where a pipe, a data cable or an earthing conductor meets a family, the first piece of the MEP
network (#169). Besides `power` and `switch`, a connector's `kind` is `pipe`, `data` or `earthing`; `system` is only a power or a
switch connector's. A `pipe` connector has a `domain` (`cold`, `hot`, `waste`, `vent`, `storm`, `gas`), a `flow` (`in`, `out`,
`both` the default), `at` (mm, in the family's axes: x along it, y to its left, z up from its base), `direction` (the way its
pipe leaves it, any length but none; worked out as a unit vector) and `size` (its DN, mm, up to `MAX_CONNECTOR_SIZE`); a `data` or
`earthing` connector has `at` and may have the other two; an electrical one may have `at` and `direction`, never a `domain` or a
`size`, and only a `power` one carries a load. Each of `at`, `direction` and `size` is a number or a formula over the family's
parameters, and `name` says which it is for people. `Model::component_connectors` places them where a component stands
(mirrored, turned and raised with it), and `element_json` gives `at` and `direction` in the model. The starter WC, washbasin,
bathtub, kitchen sink, shower and floor drain carry their waste (out, DN 100 for the WC, 50 for the sink, 40 for the rest) and
their cold and hot water (in, DN 20); the starter elbow, tee and gate valve have a connector at each end, of their diameter and
with no domain, which a pipe fitting's or accessory's alone may leave out: it takes the domain of the pipe it joins, as Revit's
fitting takes its pipe's system. IFC writes the electrical device's set from an electrical connector only, so a fixture with
pipe connectors is no electrical device; IFC ports (`IfcDistributionPort`) come with the network. A file
from before reads as it did; file format 18 is the one that has these connectors, so a build that reads up to 17 refuses a
project saved by this one, saying it is from a newer BIMCraft, instead of failing on a `pipe` connector it doesn't know.

**The pipe network** (`network.rs`, the `network` command; piece 3 of #169) is worked out from the model each time it is asked
for and never stored, as wall joins are, so it can't go stale and changes no file. Pipes' centre-line ends and the components'
pipe connectors within `tolerance` (1 mm by default) are one node; a node on another pipe's length, away from its ends, splits
that pipe into two edges without splitting the element. A pipe fitting's or accessory's connectors are joined through it (as a
fitting's linked connectors carry a system through it in Revit); a fixture's are not, since its waste and its water are networks of
their own. The reply has the nodes (what meets at each), the edges, the groups of joined elements with their pipes' systems and
their connectors' domains, the open ends (an end or a connector meeting nothing, as Revit's Show Disconnects marks) and warnings
that never refuse the model, since a drawing in progress has open ends: two pipe systems in one network, two connector domains in
one, a pipe met along its length with no fitting, and, in a network with a waste connector, a pipe leaving a node downhill narrower
than one coming down to it. A query works on 200,000 pipe ends and connectors at most (`MAX_POINTS`), and looks for nodes along
pipes through the plan's metre squares each lies across, 4,000,000 of them for all the pipes together (`MAX_CELLS`): a small file
of pipes kilometres long on the diagonal would fill billions, so past it the query is refused, naming the pipe across the most and
saying to ask for one `level`. It has no shortcut, since Revit has no tool for it. IFC's ports say the same with
`IfcRelConnectsPorts` between two `IfcDistributionPort`s; writing them is piece 8. A pipe's `width` is its outside diameter and a
connector's `size` its DN, so they aren't compared. Conduits and the other electrical runs are not in it yet: their routes are
piece 6.

**The fixtures library** (`plumbing.rs`, `data/plumbing.json`) holds what the sizing commands know of each kind of fixture, the way `data/electrical.json` holds the electrical settings: its design flows in L/s (cold, and hot where a source gives one; `hot_flow` is `null` until one does, never guessed), its weight (the method of weights of `water_size`), its units of contribution and the smallest DN of its drain (`drain_size`), each with the source it comes from (`water_source`, `drain_source`). `plumbing::validate` refuses a row with no source, an empty or repeated kind, or a number that is not positive and finite, and a test runs it over the shipped file; the library reads as empty if the file fails it, so a bad row shows as "no fixture" and never as a wrong number. `water::fixture` and `drain_size`'s `drainage::fixture` are views of it, and the commands list its kinds in their errors. A project's own values are a later step.

### Wood framing

`framing.rs` frames walls in wood, as a light-frame house is built, and `framing/floor.rs` floors ([below](#floors)): a wall holds `framing`, the name of a framing type (a
library reference, held by its GUID in a file, renamed and removed as other type references are), and its plates, studs,
kings, jacks, headers, sills, cripples and blocking are worked out from the wall whenever they are asked for (`framing::wall`)
and never stored, as a railing's balusters are. Moving a window, changing a height, attaching the top to a roof or editing the
framing type changes them with nothing to regenerate, and a file holds no member to go stale.

A framing type is `Types::framing[name]`, a `FramingType` (all mm; every field has its default, so `type {category: framing,
name}` alone is 2x4 framing 16 inches on centre for a bearing wall): `spacing` 406.4 (the grid's stud centres apart);
`layout` `start`, `end` or `centre` (where the grid is measured from); `thickness` 38 (a stud's or a plate's); `material`, a
material of the library, none for the structure layer's; `bottom_plates` 1 and `top_plates` 2; `kings` 1 and `jacks` 1 each side
of an opening; `window_rough` [25, 13] and `door_rough` [50, 41], what a rough opening adds to the opening's width (half each
side) and to its height (on top); `headers`, `{up_to?, depth, plies, flat?}` in order, the first whose `up_to` holds the rough
opening's width (or the last) its header, by default 2 plies 184 deep up to 1250 and 2 plies 235 deep above; `sill_plates` 1;
`corner` `three_stud`, `two_stud` or `none`; `tee` `studs` or `none`; `blocking`, heights over the wall's base of rows of flat
blocking, none by default; and `stock`, the stock lengths a cut list takes members from, shortest first (2440, 3050, 3660,
4270, 4880, 6100); and the floors' fields (`rim`, `lap`, `bridging`, `trimmers`, `under_partitions`, [below](#floors)), which a
wall doesn't read. `Model::check` holds each to its range (`framing::check_type`): thickness 10 to 300 and spacing wider than
it (up to 3000), 0 to 4 plates of each kind, 0 to 3 kings and jacks, rough allowances 0 to 500, 1 to 8 headers of 1 to 3 plies
and 1 to 1000 deep, 8 rows of blocking and 16 stock lengths at most, each longer than the last, a lap of 0 to 2000, bridging
of 0 or 300 to 100000, 1 to 3 trimmers, and a material the library has.
A new project has no framing type: `frame` makes `2x4 @ 406` when there is none, so a project that frames nothing doesn't change.

The members stand in the wall type's structure layer, the thickest where it has several (the first of those): as deep across the
wall as that layer is thick, so a 2x4 wall is one with an 89 mm structure layer and a 2x6 one with a 140, never a second number
that could disagree. `framing::check`, which `Model::check` asks of each framed wall, refuses a framing type that doesn't exist,
a wall that isn't straight, a wall type with no structure layer, a wall longer than 1024 spacings (`MAX_STATIONS`) and a header
whose plies are wider together than the layer; each error says what to change, and `frame`, `modify` or `type` rolls back.
Nothing an opening or a join does is refused: moving a window must never be, so what doesn't fit is left unframed.

The rules, in the wall's own frame (`s` along its location line from its start, `across` to its left, `z` up):

- The framed length is the structure layer's along the wall. A free end is square. At a square L corner of two walls each with a
  structure layer, the wall of the lower GUID (the one whose compound core runs through, `walls/compound.rs`) runs to the
  other's far structure face and the other stops at its near one, so their members neither overlap nor leave a gap. A wall
  ending on another's side (a tee) stops at that one's near structure face, not its finish (where its own structure layer's two
  edges both reach it, should it meet at an angle), so a partition against a clad wall is nailed to its studs. At any other join
  (not square, three walls) the layer's shorter edge ends it.
- A stud stands flush with each end; `three_stud` adds one to the wall that runs through an L corner (the butting wall's end
  stud is the third), its face on the inside corner, where the other wall's structure begins, so the lining has backing there
  however deep that wall is (never nearer the end than beside the end stud); `two_stud` and `none` add none. Grid stations are stud centres `k` spacings from
  the framed length's start, end or middle; a grid stud is left out where it would stand in a rough opening or nearer than its
  thickness to another upright. An upright shorter than it is thick is left out.
- Each door and window hosted in the wall has a rough opening as wide as it plus the allowance, centred on it, from a window's sill
  (a door's: the wall's base) up to its head plus the allowance. Jacks stand beside it from the bottom plates to its top, kings
  outside them full height, a header on the jacks from the outer face of one to the other's (its plies side by side, centred in
  the layer; a `flat` one as wide as the layer, its plies stacked), a window's sill plates under it between the jacks, and
  cripples at the grid stations inside it, over the header up to the top plates and, under a window, from the bottom plates
  up to the sill. No cripple is shorter than three times its thickness (114 mm for a 2x, 4 1/2 in): one shorter splits as
  it is toe-nailed at both ends. Where the whole gap over a header, or under a sill, is shorter than that, it has no cripples:
  as many flats as fit (blocking, `thickness` thick and as wide as the layer, keyed `<GUID>/blocking/top/<i>` and
  `<GUID>/blocking/bottom/<i>`) lie stacked on the header, as long as it, or on the bottom plates between the jacks, what is
  left (less than a thickness) shimmed on site; none where not even one fits. The cabin's 900 x 2100 door in a 2440 wall
  has its header's top 39 mm under the top plates: one flat, not 39 mm cripples. Under a sloped top (a wall attached to a
  roof) the gap is flat-filled only where it is short all along, the flats as many as fit at its lowest; else the cripples
  stand where they are long enough and a short one at the low end is left out. The bottom plates are cut across a door's
  rough opening. An opening whose kings would run past the framed
  length or into an end or corner stud, overlap an opening framed before it along the wall, or leave no room for its header
  under the top plates or its sill over the bottom ones, is left unframed, and still no stud stands in its rough opening.
- Plates follow the wall's base and top (`walls::profile`) along both faces of the structure layer, the higher base and the
  lower top of the two, so under a roof sloping across the wall (an eave wall) nothing rises out of the wall at its low face: a
  top attached to a roof sloping along it makes sloped top plates, a piece for each slope, each as thick square to its slope as
  `thickness`, with plumb ends; the studs are cut each to its own length with a bevelled top. A top plate that would come down
  onto the bottom plates (a wall too low) is left out.
- A plate longer than the longest `stock` length is spliced, butt-jointed, over full-height studs (studs, kings, corner and
  tee studs, so never over an opening), into the fewest pieces that length holds and, of those splits, the one whose longest
  piece is shortest, ties going to the splices nearest the wall's start (`framing::spliced`, which a roof's rafters use too).
  The lower top plate is spliced first, and each top plate's splices stand at least 610 mm (24 in, `TOP_SPLICES`) from the
  others', as IRC R602.3.2 has a double top plate's end joints offset. Each piece is keyed by the plate's key and its place,
  `/0` first (`top_plate/1/0/0`, `top_plate/1/0/1`). The cabin's 6101 mm wall has each plate in two pieces, every one from
  6100 stock. Where no splice over a stud keeps every piece within the stock (studs farther apart than the longest stock), the
  split whose longest piece is shortest is made and each piece longer than every stock length is warned of.
- The upper top plate laps where two framed walls meet, as IRC R602.3.2 has a double top plate do, when both have two top
  plates or more of one thickness, neither is attached to a roof or floor and their tops are at one height (else they butt as
  the other members do). At a square L corner the butting wall's upper plate runs on to the far structure face of the wall
  running through, whose own upper plate stops at the near one, over the lower plate of the other. At a square tee the butting
  wall's upper plate runs on over the side wall's structure to its far face, and the side wall's upper plate is cut round it
  (the piece past it keyed `top_plate/0/<piece>/<wall GUID>`), so the two walls are tied over the lower plate and nothing
  overlaps.
- Where another wall ends on this one's side, `studs` puts a stud each side of its structure layer (else its whole thickness),
  backing for its end stud, unless it would stand in an opening or against another upright; no grid stud stands between the two.
- Blocking is laid flat, as thick as a stud and as wide as the layer, centred at each height between neighbouring full-height
  members (studs, kings, corner and tee studs), never across a rough opening, through the plates or in a bay narrower than a
  stud is thick.

Each member has a `key` that lasts while what it belongs to does: grid studs by their station from the layout's origin
(`stud/<station>`), end and corner studs `stud/start`, `stud/end`, `corner/start`, backing `tee/<wall GUID>/0`, an opening's
members by its GUID (`<GUID>/king/left/0`, `<GUID>/header`, `<GUID>/cripple/top/<station>`), plates by ply and piece (and a
spliced one's pieces by their place), blocking by
row and the stud it starts at; moving a window keeps the far studs' keys. Its `section` is one ply as sawn (38 x 89; each ply of a
header 38 x 184), its `length` the cut length (up an upright's longest edge, along a lying one's slope).

`frame {ids | id, type?}` frames walls (Structure > Framing) with `type` or the project's default framing type; `unframe {ids |
id}` takes it off, leaving the type in the project; `modify {id, framing: name | null}` does either for one wall; `framing {id}`
reads a framed wall's members and its cut list (role, section, plies, length to the mm, stock length, count) and builds nothing;
`type {category: framing, name, ...}` makes or edits a framing type from the fields given over its own.

An opening is never left unframed without a word: `framing` and `frame` return `warnings`, and Frame says them in the status bar.
A header deeper than the room between the rough opening and the top plates is cut to that room (a flat one loses plies first); with
less room than a member's thickness there is no header, and the jacks run up to the top plates. An opening whose kings would run
past the wall's framed end, stand on a corner's or another opening's members, whose sill would be under the bottom plates or whose
rough opening starts in the top plates (a clerestory over the wall's top) isn't framed, and the warning says which and why. A
header cut to fit is said at the height it is cut to (a flat one's plies times its depth).

A framed wall's members are solids of the wall in the framing's material (`view3d.rs`), so a pick takes the wall and a section
cuts them as it cuts any solid; its structure layer is split off its body and drawn only where a view hides Walls > Framing
(`framing`, a subcategory of walls in Visibility/Graphics, `graphics::SUBCATEGORIES`), which brings the layer back as it was. A
plan at medium or fine detail clears the structure layer's band and draws the members its cut plane crosses, filled in their
material's cut pattern and outlined (`render::members_cut`); at coarse detail the wall draws as before. A schedule of
`framingmembers` (Framing Members, `schedule::framing_members`) has a row for each member of each framed wall: Host, Host Type,
Framing Type, Role, Section (`38 x 89`, a header of two plies `2 x 38 x 184`, to the mm as the cut list has it, so a header cut
to fit groups with its like), Length (cut, to the mm), Stock Length (the shortest stock it is cut from, chosen by its length
unrounded, as the reply and the cut list choose it, so a 2440.3 stud comes from 3050; 0 where it is longer than every stock
length, which is warned of), Level, Key and Count; grouped by
Role, Section and Length it is the cut list. IFC writes each member as an `IfcMember`, `STUD` for studs, kings, jacks and
cripples, `PLATE` for plates and sills, `MEMBER` for headers and blocking, its role its Name and ObjectType and its key its Tag,
its GlobalId made from the wall's GUID and its key (so it lasts while the member does), in the framing's material, and aggregated
to its wall (`IfcRelAggregates`, with any bars that reinforce it), in no storey of its own; the wall keeps its own body. IFC2x3
writes them without a predefined type. An IFC import brings the wall back unframed: the members, being its parts, are left out.
A file with a framed wall or a framing type is FORMAT 21 (an older build would drop `framing` without a word).

In the window, the Structure tab's Framing panel has Frame and Remove Framing for the walls of the selection, each one undo step
(the status box says when no wall is selected; Frame drops the project's framing types down when it has several); Find (Ctrl+K)
has both. A wall's Properties have Framing under Structural, None or one of the project's types (`modify`), with a button beside
it to its Type Properties, where every field but the headers is typed or picked (spacing, thickness, plates, blocking heights and
stock lengths as lengths parted by commas, layout, corner and tee, rough allowances, kings, jacks, sill plates and material) and
Duplicate and Rename work as for any type; the headers are shown and set with `type`. The Project Browser lists framing types
under Families, a click opens one's Type Properties and its right-click menu deletes one nothing uses.

#### Floors

A floor holds `framing` too, a framing type's name as a wall's is (renamed, refused deletion while in use, brought in by
Insert from File, held by its GUID in a file), and `framing_angle`, the way its joists run in degrees from +x (east); none, the
default, runs them square to its outline's longest edge (the first of the longest), so they span the short way. Both are left
out of a file while none. Its joists, rims, trimmers, headers and blocking are worked out from the floor whenever they're asked
for (`framing::floor::floor`), from its outline, its openings and the shafts through it, the beams and walls under it, the walls
standing on it and its framing type, and never stored: moving a beam, an opening, a wall on it, or editing either type, changes
them. FORMAT stays 21, unreleased, which holds them with the walls'.

The framing type is the walls' (one category, `framing`, serves both): its `spacing` (406.4) is the joists' centres apart, its
`thickness` (38) theirs, its `layout` (`start`, `end`, `centre`) and `stock` as for studs, and five fields only a floor reads:
`rim` (true: a rim joist along each edge the joists end on), `lap` (0: joist pieces butt over a support; more, each runs that
far past its centre line, 0 to 2000), `bridging` (0: none; else the longest a joist runs between rows of solid blocking, 300 to
100000), `trimmers` (2, 1 to 3: plies of trimmer each side of an opening and of header across each end of it) and
`under_partitions` (true: a pair of joists under a wall standing on the floor along them). The joists are as deep as the floor
type's structure layer (the thickest, the first of those) is thick, from its underside to its top, so a 2x8 floor is one with a
184 mm structure layer; the layers over it (a subfloor, a finish) stay drawn as layers.

`framing::floor::check`, which `Model::check` asks of each framed floor, refuses a framing type that doesn't exist, a
`framing_angle` that isn't a number from -3600 to 3600, a sloped floor and one whose shape is edited (points or split lines on
its top: only a flat floor is framed; sloped members come with roofs), a floor type with no structure layer, a floor wider
across its joists than 1024 spacings (`MAX_STATIONS`) and rows of blocking of more than 65536 blocks; each error says what to
change, and the command rolls back. Nothing an opening, a support or a wall does is refused: what the rules can't frame is
framed more simply or left out with a warning that names it and says why.

The rules, in the floor's frame (`s` the way the joists run, `across` to its left, both from the outline's first corner, `z` up):

- A rim joist lies along each edge the joists end on (each not parallel to them), one thickness thick inside it and the whole
  edge long, less any hole it crosses (an opening or a shaft at the edge). Where two rims meet at a corner, the one of the lower
  edge index runs through and the other stops where its nearer corner meets the first's inner face; at a sharp corner (under
  90°) each stops where its inner side meets the outline, so none pokes past it; at an inward corner the one that runs through
  runs on to where their inner faces meet and the other starts clear of it, which fills a square inward corner; one that isn't
  square leaves a wedge between them, with a warning giving its width. A rim that would overlap another (a floor shorter there
  than two rims) or doesn't fit on the floor (thinner there than a rim) is left out with a warning. An edge parallel to the
  joists has an edge joist flush with it instead (where the edge lies farthest in, an edge within 1 mm of parallel), running
  between the rims; edges in line (a T's top) share one; one that would overlap another (a floor narrower there than two
  joists) is left out with a warning, as is one where the floor is narrower than a joist. Every joist ends square where it
  first meets a rim or a hole (where its nearer corner meets a skewed rim's inner face, so its far corner falls short of the
  rim; at an inward corner, where the rims' inner faces meet). With `rim` off the joists run to the edges themselves.
- Grid joists are centred at stations `spacing` apart across the outline from the layout's origin: `start` the outline's least
  `across`, `end` its most, `centre` its middle. The lines are laid in order, edge joists, trimmers, pairs under walls, then the
  grid, and a grid joist that would overlap one laid before it is left out. Each line runs in pieces where the outline, less
  the holes, lets it (a concave outline gives several), each piece keyed by its order along the line.
- A beam (not upright) or a straight wall under the floor whose top lies within 10 mm (`BEARING`) of the joists' underside
  where it crosses them bears them (a wall's top read along it by `walls::top_at`, so a sloped top bears where it reaches).
  Where one crosses a joist away from its ends (more than the support's half width, or a thickness, from each), the joist is in
  pieces that butt on its centre line; with `lap`, each grid joist's pieces run `lap` past the centre line (no more than halfway
  to the next support, so pieces in line never meet; shortened, with a warning) and every other piece lies a thickness over
  (to its left, else its right, wherever it clears the other members and stays on the floor, its end at a rim or a header
  where its own side of the joist meets it), beside the one before; with room on neither side its pieces butt, with a
  warning. Edge joists, trimmers and pairs under walls always butt. A curved wall under the floor at that height isn't taken to bear it, with a warning. A floor that nothing bears
  warns that its joists span the whole floor, and how far. A piece shorter than it is thick is left out, with a warning.
- Each hole (an opening, by its index, or a shaft through the floor, by its GUID, found as the floor's solid finds the shafts that
  cut it, `Model::less_shafts`, so framing and solid agree) is framed by the box round it in the joists' frame: `trimmers` plies of trimmer along each side,
  running the joists' whole length (as its line lets them, round other holes), `trimmers` plies of header across each end
  between the inner trimmers, and the joists it cuts become tails from what they start on to a header. A hole that isn't a
  rectangle square to the joists is framed by that box, with a warning giving its size. A hole whose box, grown by its trimmers
  and headers, doesn't lie inside the outline, clear of the rims and the edge joists (one touching or near the floor's edge, or
  across a notch in it), runs into another hole (or that one's grown box), or whose trimmers would run into another's, isn't
  trimmed: its joists end at its own outline, with a warning naming it.
- A straight wall standing on the floor (its level's, its base within 10 mm of the floor's top) and running along the joists
  (within 1°, its middle inside the outline) gets two joists side by side under its centre line, the floor's whole length
  (`under/<wall GUID>/0` and `/1`), and the grid joists they overlap are left out. A wall square to the joists needs nothing;
  one whose centre line is over an edge joist stands on that; one over an opening, or whose pair would overlap an edge joist
  (its centre line beside it), a trimmer or another pair, gets none, with a warning.
- With `bridging`, rows of solid blocking are laid between each two neighbouring lines of joists, as thick as a joist and as
  deep, dividing each span between what bears them evenly, no bay longer than `bridging`; none across a hole or its trimmed box,
  or past the outline or a rim; none between joists less than a millimetre apart (a type spaced a hair over its thickness),
  with a warning.

A floor's keys: grid joists `joist/<station>/<piece>` (the station counted from the layout's origin), edge joists
`edge/<edge index>/<piece>`, rims `rim/<edge index>` (edges numbered as the outline has them; `rim/<edge index>/<piece>` where a
hole cuts one in pieces), an opening's members `opening/<index or shaft GUID>/trimmer/<left or right>/<ply>/<piece>` and
`opening/<…>/header/<start or end>/<ply>`, a pair under a wall `under/<wall GUID>/<0 or 1>/<piece>`, blocking
`blocking/<key of the line on its right>/<row>`. Roles are `joist`, `rim` and `trimmer`, with `header` and `blocking` as a
wall's; each member says its `grain`, the way its length runs (`along` for joists and trimmers, `across` for a rim square to
them, headers and blocking).

`frame {ids | id, type?, angle?}` frames floors with walls, `angle` setting each floor's `framing_angle` where given; `unframe`
takes it off; `modify {id, framing, framing_angle}` sets either for one floor; `framing {id}` reads a floor's members, cut list
and `warnings` as a wall's, and also `supports` (the ids of the beams and walls that bear its joists), `span` (the longest clear
span of a joist between what bears it, face to face, mm) and `angle` (the way they run, degrees). `frame` and `framing` say
every warning, each naming its host (`floor 100005: …`).

Output follows the walls'. In the 3D view and sections a framed floor's members are solids of the floor (picking it) in the
framing's material, and its structure layer is drawn only where the view hides Floors > Framing (`floorframing`), which brings
the layer back whole; a section cuts each member filled in its material's pattern, the layers over it drawn as layers. A plan
draws no member by default: Floors > Framing Lines (`floorframingline`, off unless a view shows it) draws each member's outline
dashed, as hidden lines under the floor's top, the framing plan a framer builds the floor from. It is off by default because on
an ordinary plan the joists 406 apart hatch the whole floor; a second subcategory is needed because one key can't be shown in
3D and sections and hidden in plans. The Framing Members schedule lists a floor's members with the floor as Host, its type as
Host Type and its level as Level; the cabin's cut list is 38 joists 38 x 184 x 3012 from 3050 stock and 2 rims 7320 long (0
stock: longer than every length, to be spliced). IFC writes each joist, rim, trimmer and header as an `IfcBeam` of predefined
type `JOIST` and each block as an `IfcMember` `MEMBER`, aggregated to the slab (which keeps its whole body) in no storey of its
own, with GlobalIds from the floor's GUID and the key; IFC2x3 writes the beams without a predefined type; an IFC import of the
file brings the floor back unframed, its beams, being the slab's parts, left out. `ci/ifc-coverage.txt` has a framed floor.

In the window, Frame and Remove Framing act on the selection's walls and floors together, as one undo step, the status bar
saying how many of each and the first warning (Warnings (n): …). A floor's Properties have Framing under Structural, as a
wall's, and, once it is framed, Joist Direction (°), its `framing_angle`: blank is automatic (square to the longest edge), and
clearing it returns to that. A framing type's Type Properties group its fields: Construction (Spacing, Thickness, Layout, which
walls and floors share), Walls, Wall Openings, Wall Joins, Floors (Lap at Supports, Bridging Spacing, Trimmer and Header Plies,
Rim Joists, Joists Under Partitions), Materials and Finishes, and Other (Stock Lengths).

Not yet, for floors: sloped and shaped floors (refused); blocking over supports and at the rims; joist hangers and other
connectors; cantilevers past a support (the joists end at the outline, wherever the supports are); splitting a member longer
than every stock length (a long rim is scheduled with 0 stock); blocking where a concave outline leaves a bay between lines
that don't face each other along their whole length; a curved wall as a support; span checks against a species or a table;
trimmers and headers sized by the opening; at an inward corner of the outline that isn't square, the wedge between the two rims
(warned of, with its width); and the tip of a sharp corner past where its rims stop.

Not yet, for walls: roofs (rafters), which will reuse `Member`, the schedule, the graphics and IFC; framing elevations;
picking or tagging a single member; sheathing; span checks; connectors; a plate's splices placed by hand; and at non-square corners and joins of three walls the framed length falls back to the
layer's shorter edge, which may leave a gap or an overlap between two framed walls' members.

### Timber wall framing

`timber_framing` is the older read-only query, kept as it was; `frame` (above) is the feature.

`timber_framing` reads one isolated straight wall, its chosen structural layer and its only hosted rectangular window.
Supply their ids or GUIDs as `wall`, `layer` and `window`. Supply `spacing`, stud/plate `thickness`, layer `depth` and
`header_height` in mm. The window cut provides the rough opening. The layout uses flush end studs, a grid measured
from the wall start, one bottom and two top plates, one king and jack per jamb, a sill, and a header over the jacks.
Only whole grid pieces that fit between the adjacent framing faces are included. Attached walls, wall junctions and
layouts above 4096 grid stations are outside this command.

Each member and clear bay has a `key` scoped to the returned wall and layer GUIDs. Its `xz` is
`[left, right, bottom, top]` in mm, measured along the wall and up from its base. Every rectangle spans the returned
`depth`. For model coordinates, start at `origin` on the interior face of the framing layer, move along `x_axis`,
into the layer along `depth_axis`, and up. Repeating the query creates no project elements or Undo steps.
A bay key names its vertical region and adjacent members. A placement whose bay disappears on a new query
keeps its last intended position for conflict review. Outlet and niche placement are exercised as test consumers.

## Steel sections

`profiles.rs` gives beam and structural-column types an optional `profile`. The public `type` command accepts a catalogue
name (`profile: "IPE 300"`; IPE 80–600, HEA/HEB 100–600), or `shape` I/H, U, L, T, RHS/SHS, CHS and plate dimensions in mm.
I/U/T use `width`, `depth`, `web`, `flange`, and optional root `radius` (a T's flange on top); `turned: true` turns any
section half round in its plane, as IFC turns a profile by its own axes, so an inverted T (a foundation beam) is a T turned,
and IFC import reads an `IfcTShapeProfileDef` whose axes point down as one; L uses `width`, `depth`, `thickness`, optional root
`radius`; RHS uses `width`, `depth`, `thickness`; CHS uses `width` (diameter) and `thickness`. Plates must fit, or the
command rolls back. Creating a `beam` or `structural_column` with a missing catalogue-named `type` adds it first; existing
project types take precedence. A catalogue beam defaults to Metal. Editing dimensions/plates clears a catalogue designation
when it no longer describes the section. `shape: rectangle` removes a profile.

Plans cut columns as their actual sections, including hollow interiors. Sections and 3D sweep their plates and root fillets.
Where a steel beam joins another run, each plate and fillet runs on and is cut by the planes its mitred outline is cut by
(`runs::joined_cap`, `Geom::swept`), so its flanges meet the other beam's on the outline the plan fills, as a rectangular
beam's prism does; an end that joins nothing, and a sloped member, which never joins, stays upright. Fillets are circular
arcs approximated by four chords per quarter in native drawings/meshes; circular hollow sections use 24 sides. RHS sections
have sharp corners. This is geometric section support, not a steel connection, fabrication, structural sizing or
code-compliance model.

Schedules expose `Profile`, `Section Area` (cm²), `Weight per Metre` (kg/m) and `Weight` (kg). Mass assumes steel density
7850 kg/m³ for a profiled column and for a beam of steel or of no material; a beam of another material (a concrete T from IFC)
shows its section area and no weight, since the library's materials carry no density. Areas and quantities use analytical
plate/fillet/ring areas. IFC writes the corresponding I/U/L/rectangle-hollow/circle-hollow profile shared by the body and the
existing type material profile set. Existing cardinal points and quantity conventions remain: length, cross-section area,
lateral surface area (including hollow interiors), gross volume, without inventing net deductions or adding end faces.
Native files use format 18 or later: a reader of 17 or before rejects them before lenient field removal, preventing a steel section
from becoming its solid bounding rectangle/circle. Older files (format 17 and before) migrate without data conversion: absent
profiles keep their original plain sections. GUID references and prior migrations stay unchanged. Existing desktop Edit Type dimensions and Duplicate retain the
section; the CLI/MCP `type` command is the entry point for profile authoring. IFC section recognition currently reads direct
parameterized extruded Body representations; mapped or otherwise unsupported representations retain the existing import
fallback. Columns retain steel metadata when IFC marks them load-bearing.
