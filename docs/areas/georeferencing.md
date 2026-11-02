# Georeferencing: where the project is on Earth

Revit's Manage > Location, and IFC's `IfcSite` position with `IfcMapConversion` (SITE-04 in `IDEAS.md`). The code is
`crates/core/src/georef.rs`; IFC is written in `ifc.rs` and read in `ifc_georef.rs` (called from `import.rs`); the command is
`geolocate` in `commands.rs`; the tests are `georef.rs`, `georef_tests.rs` and `ifc_georef_tests.rs`.

## The model

`Model::location: Option<Location>`: `latitude` and `longitude` (degrees, WGS 84) of the project's origin (0, 0), its `elevation`
(mm above sea level), `true_north` (degrees, **counter-clockwise from the project's +y** to true north: 0, +y is north; 90, north is
-x), and optionally `crs`, an EPSG code. A project that isn't placed has none. The field is optional and skipped when empty, so
`FORMAT` did not change: an older version reads a placed project and drops the key as `left_out`.

`crs` is a projected CRS in metres. Left out, it is WGS 84's UTM zone of the place (`utm_epsg`: 326zz north, 327zz south, with
Norway's and Svalbard's zone exceptions). Any metre-based EPSG projected CRS in `proj-core`'s registry works (31983 is
SIRGAS 2000 / UTM zone 23S, 2154 Lambert-93). A geographic CRS, a CRS in feet, or an unknown code is refused by
`Location::problem`, which `Model::check` runs, with what to fix.

## The conversion

Projections come from `proj-core` (pure Rust, no unsafe, MIT OR Apache-2.0; default features off), checked in the tests against
PROJ's own numbers (eastings and northings to a millimetre, meridian convergence, point scale). `Location::mapping` takes
the origin's easting and northing, the **meridian convergence** and the **point scale** from the transform itself (two small
steps), so any CRS works the same way. The project's plane is then one rotation and one scale on the grid:

- the project's +y lies `true_north` degrees clockwise of true north, so the project's x axis is at
  `rotation = -(true_north + grid azimuth of true north)` from grid east;
- `E = E0 + k (x cos r - y sin r)`, `N = N0 + k (x sin r + y cos r)`, x and y in metres.

That holds over a site of some kilometres: the scale and the convergence are those at the origin. `to_lonlat` and `from_lonlat` go
between the project's mm and degrees; WGS 84 is the datum of the degrees (where the registry has no step from WGS 84 to a CRS, its own
datum's geographic CRS is used; SIRGAS 2000 differs from WGS 84 by under a metre).

## The command

`geolocate {latitude?, longitude?, elevation?, true_north?, crs?, clear?, point?, lonlat?}`. Both `latitude` and `longitude` the
first time, then any one. `crs` is an EPSG code, `"EPSG:31983"`, or `"utm"`. `point` [x, y] mm answers with `lonlat`, `lonlat`
[longitude, latitude] with `point`; neither changes the project. `inspect` shows `location`.

## IFC

Export, when the project is placed:

- `IfcGeometricRepresentationContext.TrueNorth` is the direction `(-sin a, cos a)`;
- `IfcSite` gets `RefLatitude` and `RefLongitude` as compound plane angles (degrees, minutes, seconds, millionths of a second; every
  part of the angle's sign) and `RefElevation` in the project's mm;
- `IfcProjectedCRS('EPSG:n', name, datum, $, projection, zone, metre)` and `IfcMapConversion(context, crs, E0, N0, height,
  cos r, sin r, scale)`, eastings, northings and height in **metres** (the CRS's map unit), `Scale = k * 0.001` (the project's
  millimetres to the grid's metres, the grid scale included).

Import (`ifc_georef::read`) takes the site's latitude, longitude and elevation; else inverts the map conversion through its
`EPSG:n` CRS; reads the true north from the map conversion's rotation, else from the context's direction. A CRS other than the
place's UTM zone is kept as `crs`. A place the project can't hold is noted in `skipped` (`the project's location: …`) and the rest of
the file opens. A file's own far-from-origin coordinates are still brought near the origin (`Imported::moved`), independently of
the location.

## Checking it

`bimcraft-cli run --sample --cmd 'geolocate {...}' --save /tmp/placed.ifc`, then `python ci/validate_ifc.py /tmp/placed.ifc`
(IfcOpenShell, `express_rules=True`). A project point mapped through `IfcMapConversion` by hand (`E = E0 + s (a x - b y)`,
`N = N0 + s (b x + a y)`, with `a` and `b` the x axis abscissa and ordinate) agrees with `geolocate {"point": …}` run through
pyproj to under a millimetre over a kilometre.

## Not yet

A survey point and project base point as distinct from the origin; vertical datums and geoid heights (`elevation` is as given); a
CRS in feet; grid-based datum shifts; the Location dialog in the desktop. Terrain ([terrain](terrain.md)) and OpenStreetMap import ([OSM import](osm-import.md)) build on this.
