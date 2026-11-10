# IFC import: bodies, colours, classes

What `import::ifc` reads of an element's body and look, beside what [families](families.md) says of the generic components
it makes and [looking at the result](looking-at-results.md) of checking an import. Each element is measured from its body, so
how a file builds a body is what decides whether it comes in.

## Bodies

`Reader::faces` reads a representation item as faces in its own coordinates: an extruded profile (tapered as its first
profile), a disk swept along a curve (polygonal or not), a faceted B-rep, a shell or face set, a triangulated or polygonal
face set, a mapped item, a box, a boolean result's first operand (and its second, a union's). `import/shapes.rs` reads the
rest that BIMCraft builds:

- **Turned** (`IfcRevolvedAreaSolid`, tapered as its first profile): the profile turned about its axis in 32 steps a turn,
  its holes cut in convex pieces as an extrusion's. A positive angle turns it as IfcOpenShell does, against the right hand
  about the axis (about y, from x toward z): the IFC page couldn't be read from here, and IfcOpenShell is what exporters
  are checked against (it agreed on volume and bounds, `import::shapes` and `ifc_geometry_tests.rs`).
- **Swept along a curve** (`IfcFixedReferenceSweptAreaSolid`, `IfcSurfaceCurveSweptAreaSolid` on a plane): the profile square
  to the curve, its x toward the fixed direction or the plane's normal, as ISO 10303-42 lays a swept area. Along a polyline
  each segment's ring is mitred at the bends, and a bend that turns back on itself ends one piece; along a curve drawn as
  chords (a circle, an ellipse, an arc of one, a B-spline) each ring is square to the curve where it is. Start and end
  parameters are read on a polyline or an indexed polycurve of lines (a segment is a parameter of 1), a circle or ellipse
  (its angle), or a trimmed curve where they are its trims; any other is left out, saying so. A sweep on a surface that
  isn't a plane is left out, saying so.
- **Advanced B-reps** (`IfcAdvancedBrep`, with voids, and an `IfcAdvancedFace` in a surface model): each face on its
  surface, a plane, a cylinder, a sphere, a surface of revolution or of linear extrusion; its edges as lines, circles and
  ellipses between their vertices, or the curve they follow (polylines, trimmed curves, B-splines). An edge a loop goes
  along both ways is a seam and parts the loop. A flat face of one loop is that loop; any other is a constrained Delaunay
  triangulation (`triangulation.rs`) in a plane its parameters lie flat in: a turned surface's angle and length along its
  meridian as polar coordinates (so a band round the axis is a ring), with points a fifteenth of a turn apart inside a
  curved face, and each triangle turned the way the surface's normal is, times the face's sense. A face on a B-spline
  surface, or on one not listed (a torus, an offset surface), a face round the end of its axis (a cone's tip, a sphere's
  pole) and one that won't triangulate leave the whole B-rep out, saying which.
- **B-spline curves** (`IfcBSplineCurveWithKnots`, rational or not) as 12 points a knot span, the control points of a curve
  of degree 1, wherever a curve is read: profiles, directrices, edges. One whose knots don't fit its control points is none.
- **Reinforcing bars** (`IfcReinforcingBar`): a bar of one radius whose body is a disk swept along a polyline, a disk behind
  an `IfcMappedItem`, or a circle extruded straight, each piece starting where the last ended, is one line. Disks of several
  radii, disks that do not meet end to end, and a body of several separate bars make no one line: the bar is left out, and
  the import's `skipped` messages say so.

An item builds at most `shapes::MAX_FACES` (a family mesh's `MAX_TRIANGLES`) faces; past them it builds none. A curved face
takes at most `MAX_ROUND` points round it and `MAX_INSIDE` inside. Why an item built nothing, or less than it holds, is
`Reader::give_up`'s, kept by item (`Reader::unread`): a product left out as having no body says it after `it has no body
BIMCraft can read: `, and one that came in without part of its body has its own note, `part of its body was left out, …`.

## Colours

A material takes the colour a file shades it in: a surface style of its name, else its `IfcMaterialDefinitionRepresentation`'s
(`material_colours`; how see-through, from a shading's or rendering's second argument). What a component's forms are made of
comes from how its body is styled (`looks`, `Reader::looked_items`), each look a form of its own:

- An `IfcStyledItem` names an item and its styles, a surface style straight (IFC4) or through an `IfcPresentationStyleAssignment`
  (IFC2x3). A style with a name makes the form of that material, coloured as above; one with none makes it of a material named
  for its colour (`Colour FF8000`, with `25%` after it where it is see-through), created in that colour.
- A style on an item inside a mapped representation (as Revit writes a type's geometry once and maps it) is that item's, and an
  item takes the style of the mapped item it is in where it has none of its own.
- An `IfcIndexedColourMap` on a triangulated or polygonal face set colours each face by its index into the map's
  `IfcColourRgbList` (from 1), a material for each colour; its `Opacity` read as its name says (1 opaque, none opaque, so 0.5
  is 50% see-through). A face it gives no colour (past its list, or an index past the colours) is the item's look; a map with a
  colour that isn't three numbers is not read at all.
- What nothing styles is the element's material, as it was.

Walls, floors, roofs and the other elements made of layers or of a type's material take their colour from those materials by
name alone: a styled item on their bodies doesn't colour them.

## Classes

A product comes in as BIMCraft's element where BIMCraft has one with its meaning, and as a generic component that remembers its
class where it hasn't ([families](families.md)); a class is never forced into an element of another meaning.

- **`IfcMember` as a beam.** A member that frames, its predefined type (else its type object's) a brace, strut, chord, collar,
  purlin, rafter, member or stiffening member, or none, comes in as a beam (`FRAMING`) where it runs along a line as a beam may
  (`run_in`, then `runs::check`: not standing, sloping at most 75°), measured as an `IfcBeam` is. One that is part of anything
  but the building's spaces or an `IfcElementAssembly` (a truss) stays out of it: a curtain wall's mullions and transoms, a
  roof's or a stair's parts. A mullion, a plate, a post, a stud, a stringer, a user's own type (its object type says what it
  is), one that stands or slopes past a beam's limit: generic, as before. A member that comes in as a beam goes back out as an
  `IfcBeam`.
- **`IfcAirTerminal` stays generic.** BIMCraft's `Category::AirTerminal` is a lightning air terminal (a captor), written as a
  proxy of object type `LightningTerminal`; IFC's is a supply or extract grille or diffuser. Reading one as the other would put
  diffusers in the SPDA tab's category, so it stays a generic component of its class, as Revit's Air Terminals do
  (`crates/rvt` `generic.rs`).
- **No new categories for mechanical equipment, duct fittings or fire protection terminals.** A category of BIMCraft's goes
  out as one IFC class (`Category::ifc`), and these are many (`IfcPump`, `IfcFan`, `IfcBoiler`, `IfcTank`, `IfcChiller`…;
  `IfcDuctFitting` of its kinds; `IfcFireSuppressionTerminal`'s sprinklers and hydrants): a category would send each back out
  as one class, where the generic family sends it out as its own class, predefined type and object type. A category of its
  own would also need its place in Visibility/Graphics, Object Styles, the schedules' `OWN` table, the Family Editor's list, the
  discipline tabs, `lenient` and the translations; worth doing when BIMCraft has tools for those systems, not as an import's
  side effect.

## Georeference

Where a project lies on a map is `Model::georeference` (`model::Georeference`): a projected coordinate reference system by its
name (an EPSG code) and what else IFC says of it (a description, geodetic and vertical datums, map projection and zone), the map
point of the project's origin in mm (eastings, northings, height), the angle from the map's east to the project's x axis in
degrees, counterclockwise, and the scale, a distance on the map over the same in the project (1, or a grid's scale factor). The
model's own coordinates and drawings don't move with it.

- **Import** (`import::georeference`): the file's first `IfcMapConversion` (or IFC4X3's `IfcMapConversionScaled`, by its x
  factor) to an `IfcProjectedCRS` that has a name. Eastings, northings and height are in the CRS's map unit, else the project's
  length unit; the file's `Scale` takes the project's length unit to the map's, as IFC4 and IfcOpenShell's `geolocation` read it
  (a file in millimetres on a map in metres writes 0.001), so a millimetre project in metres keeps a scale of 1. Where the import
  brings a model at map coordinates near the origin (`Imported::moved`), the map point of the new origin is moved by the same,
  turned and scaled, so the georeference still puts every element where the file did. One that breaks `Georeference::check` is
  left out with a note, and the project opens without one. IFC2x3's `ePSet_MapConversion` is not read.
- **Export** (`ifc::export`): an `IfcProjectedCRS` in metres (`IfcSIUnit` METRE as its map unit) and an `IfcMapConversion` from
  the model context, eastings, northings and height in metres, the x axis as its cosine and sine, the scale times 0.001.
- **The command** `georeference` says it, sets it (a project with none takes one with `crs`; what isn't given keeps its value) and
  clears it; `Georeference::check` bounds it (`MAX_MAP`, 1e13 mm; a scale of 1e-6 to 1e6; one-line texts of `MAX_TEXT`), as
  `Model::check` does for a file.
- **The file**: an older BIMCraft leaves `georeference` out of a project and opens the rest (`lenient`), so it took no `FORMAT`
  step. The desktop doesn't show it yet.
