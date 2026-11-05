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

An item builds at most `shapes::MAX_FACES` (a family mesh's `MAX_TRIANGLES`) faces; past them it builds none. A curved face
takes at most `MAX_ROUND` points round it and `MAX_INSIDE` inside. Why an item built nothing, or less than it holds, is
`Reader::give_up`'s, kept by item (`Reader::unread`): a product left out as having no body says it after `it has no body
BIMCraft can read: `, and one that came in without part of its body has its own note, `part of its body was left out, …`.
