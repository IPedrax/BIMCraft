# IFC exchange: grids

Native grid lines go to IFC with their names and endpoints in the project's millimetres. Two or more lines form one
`IfcGrid`, placed in and contained by the building, since the datums span storeys. Each line is one `IfcGridAxis` with a
two-dimensional `IfcPolyline` and `SameSense = true`. Lines parallel to the first, including those drawn in reverse,
form U; the rest form V. The grid's `Axis` representation also draws their curves for readers.

IFC4 requires nonempty U and V lists and exclusive membership of an axis in those lists. If all lines are parallel,
the last real axis forms V and the others form U; `PredefinedType = IRREGULAR` promises no rectangular arrangement.
A lone line cannot form a valid `IfcGrid` without another axis. It is instead an `IfcAnnotation` with its native name
and GlobalId, an `Axis` curve representation and `ObjectType = BIMCraft_GridAxis`. BIMCraft restores that annotation as
a native grid. Other IFC readers can display its curve, but will identify it as an annotation, not an `IfcGrid`.
No extra axes are invented. A project without grids writes neither grid products nor this fallback annotation.

Import reads external `IfcGrid` U, V and W axes, transforms their curves through the grid's placement and respects
`SameSense`. A shared axis reference within one grid is read once; the same reference placed by different grids is
read separately. Distinct axis entities are kept even when their geometry coincides. Ordinary annotations are not
treated as native grids.

The command-level save/open regressions are `crates/core/tests/ifc_grids.rs`. CI validates the sample, the representative
axes in `ci/ifc-coverage.txt`, a lone-axis annotation and an all-parallel grid with IfcOpenShell 0.8.5 and EXPRESS rules,
then builds every represented product's shape (`ci/validate_ifc.py`). This adds no native file fields or migration.
