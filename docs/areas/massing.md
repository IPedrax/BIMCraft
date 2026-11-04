# Masses

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

A mass (`Mass`, category `mass`) is a footprint (`Mass::footprint`: counter-clockwise whichever way `boundary` runs) extruded from
`base_offset` over `level` up to `top_level` or `height`, as a shaft is, with `params`. Given a `top` outline (as many points as
`boundary`, twin for twin, and running the same way round; `Mass::top_ring` has it counter-clockwise like the footprint) it is lofted
straight between the two (`Mass::lofted`: a `top` equal to the footprint is no loft, and the `mass` command drops it; its `lean`
parameter makes `top` by moving the footprint sideways, a sloped mass). `Model::mass_outline(ms, z)` is the cross section at a height,
the footprint, or each corner that far along its way to its twin (`massing::ring_at`), and everything that measures a mass asks it:
floors, the `massfloor` and `mass` schedules, gross floor area, the plan, snaps. The area is a quadratic of the height, so
`Model::mass_volume` is Simpson's rule on three cross sections, exact. `massing::loft_problem` (which `Model::check` and the IFC import
both ask) refuses a top outline that doesn't pair up, runs the other way round, or collapses the cross section at some sixteenth of
the way up. A plan dashes the outline at its cut height where the plane passes through it (`Model::mass_cut_by`); a section outlines
where its line cuts it and never fills it in poché (`section` leaves masses out of `cuts`; a straight one's stretches come from
`view3d::crossings`, a lofted one's lines from `massing::section_lines`, the cut of its skin of triangles, joined end to end); 3D draws
a straight one as a slab and a lofted one as `massing::tris`, a closed skin turned out (two triangles on a side whose twin
edges are parallel, which makes it flat; a side that twists is cut into `massing::STRIPS` strips up its height, so a twisted loft's volume
in 3D and IFC is a percent or two under the exact one, while the areas and volumes the model reports are exact),
see-through by `graphics::MASS_TRANSPARENCY` unless the view's graphics say otherwise (`Look::with` starts a mass's override there, so a
project with masses never takes its plain shortcut). Show Mass (`show_mass`, MH) is `graphics {hide|show: [mass]}` on one view.
`Model::mass_slices` is the levels it crosses, from its base up to, not including, its top, each with how far the mass goes up from
there: `mass_floors` and the rows of the `massfloor` schedule (`schedule::sources`; a `Row` carries the schedule's category, not its
element's) come from it.
What a mass makes remembers it, under reserved names in `params` as phases and paint are held (`massing.rs`; no project or family
parameter may take them, `Model::check` holds each to a real mass and a level or a face of the right kind, `delete` takes off the marks
naming a mass that goes, and a copy or a split piece loses its own). `mass_floors` leaves `Mass Floor`, `{mass, level}`, on its floors,
so a second run finds them by it, gives them the mass's outline at their level (and the type, if one is given), makes none again, lists
as `stale` the ones on levels the mass no longer crosses and deletes those only with `prune`. `wall_by_face`, `floor_by_face` and
`roof_by_face` build their element by running `wall`, `floor` and `roof` with the face's parameters, so those commands' checks stay the
one place, and leave `Mass Face`, `{mass, face}`: an edge number (numbered from the first point, counter-clockwise) for a wall, `"top"`
for a floor or a roof. A wall stands plumb, so only on a face whose top edge is right over its bottom edge (`massing::plumb`); a floor
and a roof lie on the top outline. `update_by_face` (UF; Revit's Update to Face has no default shortcut) rebuilds the geometry of the
ones asked for, a mass's, or all of them (`massing::follow`: a wall's ends, level, offset and height or top level; a floor's or roof's
boundary, level and offset; the type, a roof's slope and the rest stay) and refuses the lot, naming each, when a face is gone. A mass
mirrored in place runs its outline round the other way, so `relocate` renumbers the faces its walls hold (`massing::mirrored`). `fetch_terrain` runs the same step for the buildings it stands on new ground, mass by mass (a shell whose face is gone is left whole and named). Nothing
else ties them to the mass, and IFC carries no mark: what comes back from a file is ordinary. `commands::category` reads `mass` and
`masses` as `mass` (it trims every trailing `s`, which would make `ma`). IFC writes a mass as an `IFCBUILDINGELEMENTPROXY` of object type
`Mass`, its footprint swept up; a lofted one is a closed `IFCTRIANGULATEDFACESET` with a `BIMCraft_Mass` property set (`ifc::LOFT`) whose
`Loft` says as JSON how the footprint and the top outline pair up, since a face set doesn't. Import reads those before the proxies that
become components of families, and takes a lofted one only when that set's outlines are the body's bottom and its top; a mass whose
outline changes from base to top with no such set is left out and listed. The desktop's Massing tab has the Mass tool (`Tool::Mass`:
corners clicked, Enter or the first corner again finishes, `finish_mass`), Show Mass (the button is pressed while the view shows
masses) and, for each selected mass, the Model by Face buttons, which run the face commands, and Update to Face; a mass's Properties
hold its levels and offsets and `Top Outline`, its corners as `x, y` parted by `;`, blank for none.

Masses also come from OpenStreetMap: `import_osm` makes one for each building of a file, on the terrain, and keeps its `OSM Id` as a
parameter so importing again updates it in place ([OSM import](osm-import.md)).

## Building shell

`building_shell {ids, storeys?, storey_height?, walls?, floors?, roof?, shape?, angle?, height?, orientation?}` (`commands.rs`, the pure
roof planning in `shell.rs`) makes, for each mass chosen (up to 200 a command, one check, one undo step): a wall on each plumb edge
(`wall_by_face`); **a floor at the base and over each storey** on the mass's own level at `base_offset + k x rise` (the storeys: `storeys`,
else the mass's `OSM Levels`, else its height over `storey_height`, 3000; `rise` is `storey_height`, else the facade's height over their
number), each marked `Mass Face` = `{mass, face: {storey: k, rise}}` so `update_by_face` re-places it (`massing::Face::Storey`); and a
**roof** on top of the walls: `shape` (else `OSM Roof Shape`) flat, hipped, pyramidal (a hip on a square) or gabled; any other value is a
flat roof and is said in the reply's `notes`. The slope is `angle` (else `OSM Roof Angle`), else what makes the roof `height` mm tall
(`OSM Roof Height`: the roof is laid out once at 45 degrees and its peak, proportional to the slope's tangent, gives the slope; cut to 75
degrees, said), else 30. A gabled roof stands the footprint's two edges across its ridge upright (the ridge runs along the smallest
enclosing rectangle's longer side, or across it with `orientation: "across"`); a footprint that is not near a rectangle (under 80% of
its rectangle) or has no edge across the ridge at each end gets a hipped roof, said. A footprint of more than 256 corners gets a flat
roof. **A gabled roof's upright ends** reach into its gable: the walls on the edges it stands upright get `attach_top` to that roof, so
their tops follow its underside to the ridge. **Doors** (`doors`, true, with walls): each entry of the mass's `OSM Entrances`
([OSM import](osm-import.md#entrances-doors-and-exits)) is cut into the wall nearest it (within 300 mm) with `door {wall, offset}`, of
the project's door type nearest the entrance's `width` (else 900, main 1000; a note when it is over 100 mm off, and a sliding type for
`door=sliding` or `revolving` when the project has one), its `Mark` the kind (`main`, `exit`, `emergency`, `service`), and the project
parameters `OSM Entrance` and `OSM Door`. None where the facade is lower than the door, where no wall is near (a cut edge, a part with no
walls) or where a door stands already: each is a note. The reply's `doors` counts them. A mass that has walls, floors or a roof made by face already is skipped (`skipped` in the reply), so a second run never doubles.

