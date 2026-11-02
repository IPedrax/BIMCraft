# Structure and the discipline tabs (Plumbing, Electrical, SPDA, Data)

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

Structure (`runs.rs`, `structure.rs`; the design for the five discipline tabs is in [docs/disciplines.md](../disciplines.md)). A run
(`model::Run`) is a straight element from `start` to `end` in plan; its `offset` mm over its level is the height of its `justify` line
(top, centre or bottom) at the start, `end_offset` at the end (level when none), `system` the name of the system it is in: a beam, a
strip footing, a pipe (Plumbing), and what the other tabs draw (cable carriers, wires, down conductors, data cabling) as further
`Element` variants over the same struct, as `Door` and `Window` share `Opening`; `model::run_arm!` is the pattern for any of them, so a
place that treats runs alike names no category, and a new category is a line there. Its type is `Types::runs[category][name]`, a `RunType`
(`width` across by `depth` deep, or round, in a `material` of the library). The section is square to the axis, so a sloped run is
`depth / cos θ` deep vertically (`Geom::half`). `runs::Geom` is the one place that knows the geometry (centre line, `footprint`,
`prism` for a rectangle, `tube` for a round one, `z_span`); plans, sections (a prism is cut like any solid; a tube by the plane
through the section's line, `Solid::cuts`: the hull of where its edges cross it, a ring, an ellipse when oblique), 3D, snaps and IFC call
it and know no category. A run's heights are those of its `justify` line, which a beam or a footing has at the top by default and
anything else at the middle (`runs::default_justify`); the desktop keeps each run tool's own justification and offset
(`keep_run_options`). A beam or strip footing can't stand or lean past 75°
(`runs::check`, `may_stand`: what stands is a column; a pipe, a cable carrier or a conductor may stand, a riser). In plan (`render::stands`) a footing or a run
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
are assigned to by an `IfcRelAssignsToGroup`, its `Justification` and `Category` in `BIMCraft_Run`, and its material as an `IfcMaterial`.
Import (`import::run_in`) finds a run's axis as the direction its body's points spread furthest in (power iteration on their
covariance), so a beam rolled about its axis comes in as the box round it; an axis that stands (a riser) has its width along east and
runs up; the justification BIMCraft wrote puts the heights back on the line they were typed at (`Geom::half`), none meaning the middle;
a system comes from the IfcSystem the product is assigned to, and its category from the class and type, or BIMCraft's `Category`
where a class has several; `LoadBearing` makes a column or a floor structural.

The discipline tabs after Structure add data to this, no engine code of their own. Plumbing: `pipe` (PI), types in PVC and copper
(`Types::runs["pipe"]`, outside diameters, so a pipe's `width` is its diameter and its schedule says Diameter). Electrical:
`cablecarrier` (`cable_carrier`, CC; Cable Carrier / КНС) and `wire` (EW), by their outside size. One carrier category holds round
conduits and rectangular trays and channels; `runs::ifc_shape` writes them as `IfcCableCarrierSegment` of predefined type
`CONDUITSEGMENT` or `CABLETRAYSEGMENT` according to their section. Lightning protection (NBR 5419):
`downconductor` (DC, may stand: a riser down a wall) and `groundconductor` (GC), two runs of one IFC class that `BIMCraft_Run`'s `Category`
tells apart; an air terminal, an earthing electrode and an inspection box are components of their own family categories, written as
`IfcBuildingElementProxy` of the object type `Category::ifc` names, so the product's name (not its object type) holds `Family:Type` and
import reads it from there. Data (NBR 14565): `cable` (CB, `cabling`), a run like a wire, so a file's `IfcCableSegment` is a wire unless its
`Category` says cable; data and voice outlets (`IfcOutlet` `DATAOUTLET`, `TELEPHONEOUTLET`; one of no type reads as the power outlet,
`Category::from_ifc` taking the first of equals), a patch panel (`IfcCommunicationsAppliance`) and a rack (a proxy of object type `Rack`);
the Cable Carrier tool is the Electrical tab's, on the Data tab too. Fire and security (SP 484, SP 3.13130): a fire alarm panel
(`IfcUnitaryControlElement` `ALARMPANEL`), smoke, heat and motion detectors (`IfcSensor` `SMOKESENSOR`, `HEATSENSOR`, `MOVEMENTSENSOR`;
one of no type reads as the smoke detector), a manual call point, a sounder and an exit sign (`IfcAlarm` `BREAKGLASSBUTTON`, `SIREN`,
`LIGHT`), a ceiling speaker and a camera (`IfcAudioVisualAppliance` `SPEAKER`, `CAMERA`), `fixtures::fire_and_security`; the Low Voltage
tab places them, and the Component tool too (FA, DS, HD, CP, SR, ES, AU, MO, VC). A fixture or a
device is a component of a family whose `family::Category` says what it is (a sanitary fixture, a floor drain, a pipe fitting, a pipe
accessory…); `Category::ifc` is its IFC class, predefined type and, where IFC has no class (lightning protection), the object type a
proxy carries, and `Category::from_ifc` reads the same table back, so a category is one row, in `family.rs`. The starter families are
`fixtures.rs` (original forms, named in the project's language; a fitting or a valve is centred on its pipe, along x, so placing it at the
pipe's height lines them up; a wall device, a board and a luminaire carry their `Mounting Height` in their type, so the Fixture tool
places them at the level and they stand where they hang). The desktop's `mep.rs` has one tool for every fixture and device, `Tool::Fixture(key)`: the families of the
kinds its `FixtureTool` names, listed in its Type Selector and placed as the Component tool places them (`placing` takes the tool's
kinds), and the tabs' ribbon panels; a fixture's two-letter shortcut is in `mep::FIXTURES`, not `SHORTCUTS`, since no command makes
one. Edit Type of a run type also sets whether it is round (a diameter, no depth) and its material.

`Element::Wire` (`wire`, EW) is both the physical cable and its optional logical connection: a `source` component and ordered
`devices`, by GUID, with `system` as its loop or panel label, `cable` as its make and `reserve` as a percentage.
`along_walls` (default false, `modify` reads it too) takes each leg along the level's walls instead of square to the axes:
`Model::wall_leg` runs the cable along the walls' location lines, going from one wall to another where their ends meet (within
2×`TOL`), by the cheapest way (`MAX_WALL_ROUTE` walls at most, square legs outside that); the hook from a device to the wall
it leans on stays straight. `Model::circuit_route` gives these logical legs; `Model::circuit_length` sums them and the height
changes between `Model::device_point`s, with reserve. A physical-only wire reports its axis length. The `wire` schedule offers
Source, Devices and Cable beside the ordinary run fields; Name is the system label. Device addresses (`Model::address`) use
that label and the device's position, e.g. "L1.3". Plans and IFC draw the physical axis, as `IfcCableSegment`.
`carrier` optionally names a Cable Carrier by GUID; `local_offset` is stored on the wire. Deleting a carrier detaches its wires;
deleting a source or the last device clears the logical connection while retaining the physical wire.

The desktop's Wire button on Electrical, Data and Low Voltage uses `Tool::Run("wire", ...)`, the same point-to-point tool as
other runs. Logical references are set through the `wire` and `modify` commands, by id or GUID. The old separate Circuit tool,
CI shortcut and Divide Panel command have been removed. Format 8 migrates old carrier categories and logical loops to the new
elements, preserving their ids and GUIDs; an old loop's physical axis starts at its source toward its first distinct device.

`detectors` lays point fire detectors over a room (SP 484.1311500.2020): `Region::cover` puts a square grid over the room's
bounds, its cells' diagonal twice the detector's reach (a smoke detector's 6400 mm, a heat detector's 3500, closed in
proportion when the room's ceiling, to the level above, stands higher than the family's mounting height, or the `radius`
given), and a detector in each cell the room has, at the cell's centre or the point of the cell nearest it that is inside and
`margin` (500) off the walls. A rectangle is covered exactly; a cell an edge cuts is covered from its part inside; a `radius`
whose grid would ask for over 2000 detectors is refused. It leaves to the designer what SP 484 asks besides: two detectors
where one alone would trip the alarm, and beams and ducts that split a ceiling.

Panel-limit automation is a future Wire workflow; panel families still describe their capacity in their type names.

Stage 3 links a Wire to its CableCarrier by GUID. Translation, rotation and mirror
move the cable once, including when both are selected. Direct axis/height edits
map cable centres through the carrier's 3D frame. Type depth changes, including
closed-project ODS synchronization, retain cable position relative to the physical
section for top/bottom justification. Undo/Redo preserves both instances and GUIDs.

`open` after ODS synchronization and `inspect` report peak simultaneous gross cable
section along carrier intervals. `cable_carrier_check` takes explicit `fill_percent`
and `mode: warn|auto`; `configure` saves the policy and optional project default
carrier type. Auto chooses the smallest sufficient shape-compatible type from the
same known manufacturer for a straight connected run, including across perpendicular
branches. It checks both peak area and the cable's offset/oriented envelope, keeps
one size along the run, and never reduces an existing section's area. Unknown
manufacturer or insufficient catalogue leaves warnings. Persisted `size_locked`
marks a manual size boundary excluded from automatic sizing; locked overfill is
reported. Advisory checks do not add Undo steps.

No regulatory percentage is assumed. Gross type dimensions are not verified internal
dimensions. Instance `compartments` record measured usable width/depth/lateral offset
and explicitly permitted systems; assigned cables receive per-compartment geometry,
separation and simultaneous-fill warnings. These measurements must fit any candidate
type. They describe constraints, not independently identified divider products.
Manage > Equipment Catalogue exposes the policy, project type, manual locks and
selected element's compartment settings. Checks highlight affected carriers;
opening and ODS polling retain the feedback. The bounded Russian maximum-fill
profile is documented in [carrier limits](carrier-norms.md).
