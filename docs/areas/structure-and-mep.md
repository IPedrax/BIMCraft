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

### Physical equipment connections (stage 4)

`cable_carrier {from_wire: id|GUID, type?, placement?: centre|top|bottom, local_offset?}`
creates a carrier around the selected wire's actual axis, including its slope or
vertical span, and links the wire by GUID in the same undo step. Placement uses
the section's local frame, keeping the wire fixed; its whole section must fit,
including rectangular corners inside a round carrier. Already linked wires and
owned equipment feeders are refused. On Low Voltage, Route Carriers on Level and Route Carriers in Project process uncarried main
wires in one undo step. Compatible wires can share an overlapping, uncompartmented carrier when its envelope and simultaneous fill
fit the selected profile. A new carrier uses the project's default type and must pass the same checks.
Create Carrier from Wire opens an explicit
type, placement and lateral-offset editor in plans and the 3D view.

The run Options Bar exposes End Offset (blank means the start height) and Vertical
for wires and carriers. Vertical fixes the second point's plan coordinates at the
first point. A chained segment starts at the preceding segment's end height. In
3D, Work Level and Offset define the click plane; equipment has its own Offset.
Switch to 2D/3D keeps the active placement tool and remembers the work level.
Drawing a carrier over suitable wires offers their explicit selection and
confirmation; neither a unique nor an ambiguous overlap attaches silently.

`wire_connect {wire, device, point: [x,y,z], in_carrier?}` records an explicit point
relative to the equipment's insertion point, rotated and mirrored with that
instance. It projects onto the wire in plan (onto its height span for a vertical
wire), creates the vertical leg followed by the horizontal leg, and omits zero
length legs. A sloped wire supplies the connection height at that plan position.
`in_carrier` requires the main wire to have a carrier and creates matching carrier
legs with its type. Stage 5 logical terminals belong to the electrical line topology.
Keep their identity separate from the physical wire end and its attachment point,
so a terminal edit does not move a wire. Existing wires with `source` and `devices`
have an estimated circuit length. For a line made of drawn segments, the cable
journal must use their measured lengths and count each generated lead and reserve
once.

Create logical terminals with `electrical_terminal` and group the source, devices,
terminals and main wires with `electrical_line`. Edit the topology with
`electrical_line_edit`. On a physical connection, `wire_connect` accepts the GUID
of a terminal owned by the connected device. Removing the terminal clears that
reference and retains the lead.
The `branches` list names each tap's starting device and
its ordered devices. Numbering visits taps before the main continuation, shortest
first by plan distance, with clockwise order from east for equal lengths.
`electrical_mark_template` sets the default prefix for a system.
`electrical_line_renumber` can set a line-specific prefix, while
`electrical_line_mark` locks or unlocks an individual device mark.
`electrical_line_inspect` returns the traversal, marks and terminal current checks.
`cable_journal` reports measured lengths by wire or for the entire line.
In the desktop's standard Low Voltage ribbon, open Electrical Lines to create
a line from the current selection. The first selected placed component is the
source, the remaining components follow in selection order, and selected main
wires become its segments. Open a line in that menu to see the ordered marks,
lock or unlock one, or run renumbering.

The main `Run.connections` holds equipment GUIDs and local points; generated
`Run.feeder` names the main wire, equipment and leg role. All are visited GUID
references. Session regeneration updates surviving roles in place, preserving
their ids and GUIDs, rather than appending another set. Each physical wire leg's
reported length includes the main wire's reserve once; generated carrier lengths
have no cable reserve. Deleting the main carrier detaches the connection and
removes its owned carrier legs while retaining the physical wires and their GUIDs.
Format 14 protects this ownership contract from older
readers. Stage 4 remains in progress; these commands do not establish completion
of the full floor workflow or its visual acceptance.

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

`wire_connect` accepts `end: 0|1` (default `0`). A cable returning to the same
equipment retains two independent connections and sets of owned feeder legs.
Updating one end preserves the other and keeps the surviving legs' ids/GUIDs.
The desktop connection dialog selects the first or second end and loads its
existing draft. This identifies physical ends, not logical terminals. FORMAT15
defaults FORMAT14 connections and owners to end zero; older readers refuse the
new format rather than conflating the ends. Allowed device sides and wall-surface
tangency remain separate work; explicit local points are still the current solver.

`wire_connect` also accepts an optional `carrier` id/GUID for that end's route.
Omitted/null reconnects to the main wire route; an explicit carrier can be chosen
independently for each end, including four ends of two returning cables. The
dialog selects this route separately from whether to create carrier branches.
Refresh stores the last world-space landing and follows the selected carrier.
Deleting it removes that link and the generated carrier branches, preserving the
physical cable's landing rather than jumping it onto another route. Reconnecting
chooses a new route. Copy/transform remap route GUIDs and transform a detached
landing; surviving feeder roles keep their identity. FORMAT16 defaults the new
fields to absent for old files and prevents older readers discarding a route.
The landing is a route endpoint, not a wall attachment or bend-radius proof.

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
opening and ODS polling retain the feedback. See `docs/stage-3-progress.md` for
current acceptance evidence and the pending Russian regulatory profile.
# Surface query for mounting drafts

In 3D, Free 3D on Wire and Cable Carrier takes points from visible faces, or from the chosen work plane in open space.
Ortho keeps the dominant world axis. A segment whose endpoints lie on the same supported face is created with a mount
in the same undo step. The preview draws the physical axis, including the offset from the face.

Place on Face and Shift-drag in 3D store an equipment mount by host GUID and face. The contact follows edits to the host,
and cable feeders follow the equipment in the same undo step. Removing the host detaches the equipment at its current pose.

`run_mount {id, host, face, revision?}` persists round-run contact or a rectangular cable carrier's mounting back on a
straight wall (`face: exterior|interior`), floor or ceiling (`face: top|bottom`).
The host key is required; explicit
`host: null` detaches while preserving the current physical axis and section orientation. `revision`
rejects stale proposals. FORMAT 18 stores optional `Run.mount` with the host
GUID. Mounted endpoints and heights describe the contact line; shared run
geometry derives the centre line by adding the radius along the surface's outward
normal. Changing diameter therefore preserves surface contact. A rectangular
carrier instead shifts its axis by half its depth, with its depth axis along
the outward normal and its width along the surface. Its entire mounting back,
including openings away from the centre line, must be supported. `section_roll`
retains the section orientation after detaching or deleting the host; absent
means zero. `Geom::frame` is shared by candidate, envelope and compartment
checks, the rolled box mesh and the IFC sweep's reference direction. Legacy runs
without this field retain their existing axis/justification semantics.

FORMAT 19 also stores `Run.square_ends` (default false, omitted when false).
Mounted rectangles use end planes normal to the run axis even when their
section roll is zero. The same mode selects the physical box in Scene, plan
footprint and vertical bounds; detaching or deleting the host persists it.
A legacy unmounted rectangle without this flag keeps its plumb-cut prism.
The version fence prevents older readers silently changing a detached,
sloped carrier's shape. Round-run geometry remains unchanged by this flag.

Rotation and reflection also transform an unmounted rectangular run's physical
section. Upright sections use a global reference axis, so their roll must change
when the run rotates. Reflected physical boxes retain their end-plane mode even
if their new roll is zero. Unrolled legacy sloped prisms keep their existing
plumb cuts; mounted sections continue to derive orientation from their host.
The shared command transformation serves both edits and copies.

The whole contact segment must lie on the named face: triangle-interval union
rejects a segment crossing an opening even if both endpoints are supported.
Host motion follows the old host frame, including level elevation exactly once.
Copy remaps a copied host; deleting the host detaches at the previous physical
axis. Creating a carrier from the mounted wire transfers ownership to that
carrier and preserves physical wire geometry. These cascades share the command's
Undo step. Save/Open and deletion Undo/Redo have command-level regression tests.

Unchanged linked cables follow a mounted carrier's physical frame when its host
moves or its section depth changes. Explicitly edited cables take precedence.
Their physical endpoints and section orientation are mapped together in the
same command, before the owned connection legs are refreshed.

This implementation accepts independent round MEP runs and rectangular cable
carriers. Floor mounting uses the actual sloped plane and vertical slab depth;
ceiling mounting uses its underside and type thickness. The host frame is
anchored at the first boundary vertex with a lifted boundary tangent. Both
tangential coordinates follow translation, rotation and slope changes; the
finite face, holes and shafts still determine support. Slab top/bottom names
follow the sign of the face normal, including allowed slopes above 60 degrees.
Curved walls and roofs remain outside the run mounting solver.

`surface_query {host, origin:[x,y,z], direction:[x,y,z], reach, revision?, footprint?}` is a
read-only core command for a selected wall, floor, roof or ceiling. Coordinates,
reach and returned distance are in mm. Direction is normalized; parameters are
strict and bounded. The response has a session `revision` and `hit`, either null
or `{host: GUID, id, face, point, normal, distance}`. Normals face outwards and
semantic face names (`exterior`, `interior`, `top`, `bottom`) are independent of
triangle indices. An optional outdated revision is refused.

This host-specific query uses the scene's actual triangles and openings; other
objects do not occlude the selected host. Curved faces give tessellated normals.
An optional footprint is four ordered world-space corners of a rectangle in the
hit plane. The response then adds `supported`: whether its whole area is on that
named planar face (false on ray miss). `Scene::supports_patch` subtracts the union
of coplanar triangles, so holes inside a rectangle and overlapping triangles are
handled. Degenerate/nonplanar rectangles are refused; a fragmentation/work limit
returns an error rather than guessing support. Numerical tolerances are 1e-6 mm
for coplanarity and 1e-8 square mm for fragment area. This is planar support on
tessellated geometry, not exact support on a curved surface.
Rectangle shape and coordinate validity are checked even on a ray miss.

The query does not persist an attachment. Drafts must query again after model edits. `Session::revision` is an
ephemeral snapshot token backed by a weak model reference, independent of file
format and Undo history.

`component` and `modify` accept `connection_sides` with any combination of
`bottom`, `top`, `right` and `left` in equipment coordinates. An empty list
allows all four. To use a free contact, call `wire_connect` with
`surface: {host, face}` on a straight wall (`interior` or `exterior`). The
existing `point` input still selects a fixed local contact.

The solver chooses a contact along the allowed sides of the equipment envelope
and a landing on the finite route. It then offsets the cable axis by the outside
radius along the wall normal. Each connection end can name a separate carrier.
The lead exits the selected side and uses a vertical transition when the route
height differs. Each leg stays tangent to the wall. The cable must fit the
selected carrier section at the landing. Openings that interrupt either leg
cause the command to roll back. A specified minimum bend radius checks the
available tangent lengths at the turn and at the carrier entry.
