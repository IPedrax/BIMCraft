# Structure and the discipline tabs (Plumbing, Electrical, SPDA, Data)

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

Structure (`runs.rs`, `structure.rs`; the design for the five discipline tabs is in [docs/disciplines.md](../disciplines.md)). A run
(`model::Run`) is a straight element from `start` to `end` in plan; its `offset` mm over its level is the height of its `justify` line
(top, centre or bottom) at the start, `end_offset` at the end (level when none), `system` the name of the system it is in: a beam, a
strip footing, a pipe (Plumbing), and what the other tabs draw (conduits, cable trays, down conductors, data cabling) as further
`Element` variants over the same struct, as `Door` and `Window` share `Opening`; `model::run_arm!` is the pattern for any of them, so a
place that treats runs alike names no category, and a new category is a line there. Its type is `Types::runs[category][name]`, a `RunType`
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
are assigned to by an `IfcRelAssignsToGroup`, its `Justification` and `Category` in `BIMCraft_Run`, and its material as an `IfcMaterial`.
Import (`import::run_in`) finds a run's axis as the direction its body's points spread furthest in (power iteration on their
covariance), so a beam rolled about its axis comes in as the box round it; an axis that stands (a riser) has its width along east and
runs up; the justification BIMCraft wrote puts the heights back on the line they were typed at (`Geom::half`), none meaning the middle;
a system comes from the IfcSystem the product is assigned to, and its category from the class and type, or BIMCraft's `Category`
where a class has several; `LoadBearing` makes a column or a floor structural.

The discipline tabs after Structure add data to this, no engine code of their own. Plumbing: `pipe` (PI), types in PVC and copper
(`Types::runs["pipe"]`, outside diameters, so a pipe's `width` is its diameter and its schedule says Diameter). Electrical: `conduit`
(CN), `cabletray` (CT, `cable_tray`: the one rectangular run) and `wire` (EW), by their outside size. Lightning protection (NBR 5419):
`downconductor` (DC, may stand: a riser down a wall) and `groundconductor` (GC), two runs of one IFC class that `BIMCraft_Run`'s `Category`
tells apart; an air terminal, an earthing electrode and an inspection box are components of their own family categories, written as
`IfcBuildingElementProxy` of the object type `Category::ifc` names, so the product's name (not its object type) holds `Family:Type` and
import reads it from there. Data (NBR 14565): `cable` (CB, `cabling`), a run like a wire, so a file's `IfcCableSegment` is a wire unless its
`Category` says cable; data and voice outlets (`IfcOutlet` `DATAOUTLET`, `TELEPHONEOUTLET`; one of no type reads as the power outlet,
`Category::from_ifc` taking the first of equals), a patch panel (`IfcCommunicationsAppliance`) and a rack (a proxy of object type `Rack`);
the Cable Tray tool is the Electrical tab's, on the Data tab too. A fixture or a
device is a component of a family whose `family::Category` says what it is (a sanitary fixture, a floor drain, a pipe fitting, a pipe
accessory…); `Category::ifc` is its IFC class, predefined type and, where IFC has no class (lightning protection), the object type a
proxy carries, and `Category::from_ifc` reads the same table back, so a category is one row, in `family.rs`. The starter families are
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
import reads the same back, with the host found by the aggregation. The 3D view draws a tube along each straight piece
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

The Reinforce dialog (`crates/ui-egui/src/reinforce.rs`, Structure > Reinforcement, RF; tests in `tests/reinforce.rs`). It opens on the one beam, column,
structural column, footing or floor selected (anything else says what to select), with the form of its kind (`BEAM`, `COLUMN`, `FOOTING`, `FLOOR`: tables of
groups of fields, each field the path of the parameter it sets, such as `bottom.count`). Every field is text: a count is a whole number, a bar size is mm,
a length is read in the project's units (`Units::parse`); a field left empty is not sent, a group with none filled is left out, and the command is the one
that says what is missing. OK builds the parameters and runs `reinforce` as one undo step; a refusal is the command's own words, shown inside the dialog
(a status box in the view's corner could lie over its buttons) with the dialog left open and the project unchanged, and a field that is not a number is
named before the command is asked. It takes any number of elements selected, of one form (a mixed selection says so), and arms them with one `hosts` command; Remove Bars removes the bars of all. What was typed is kept for the next element of the same form (not in the project, not on disk). The Preset row lists the project's presets of the form: choosing one fills the fields (`ReinforceDialog::load`, the reverse of `params`, lengths in the project's units), Save Preset keeps the fields as they are under the name typed (`reinforcement_preset`, the parameters without the hosts), and Delete Preset takes the chosen one away; each keeps the dialog open. Remove Bars, shown
when the element has bars the command made (those with a layer), deletes them and leaves the ones drawn by hand. The fields' accessible names are
"Group: Field" (`Bottom bars: Count`), and the cover's is `Cover`, its label having a colon so that no two nodes carry the name.

