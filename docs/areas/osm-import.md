# OpenStreetMap import

`import_osm` brings the buildings of an OpenStreetMap file into the project as masses standing on the ground. The code is
`crates/core/src/osm.rs` (pure: reads files, finds buildings; no model, no I/O), `import_osm` in `commands.rs` (projects, drapes,
makes the masses) and `Location::projector` in `georef.rs`; the tests are `osm.rs` and `osm_import_tests.rs`. Terrain it stands on
is a [terrain](terrain.md) `Toposurface`; the project must be placed first ([georeferencing](georeferencing.md)).

Data (c) OpenStreetMap contributors, under the Open Database Licence (ODbL). The command's reply says so; keep it with anything
published from the data. No OSM data is kept in the repository: tests build their files from strings.

## What it reads

`osm::parse` takes the text and looks at its first character: `{` is Overpass JSON, `<` OSM XML.

- **Overpass JSON** (`[out:json]`): the `out geom;` form, whose ways and relation members carry their points, and the plain
  `out body; >; out skel qt;` form, whose ways name nodes (the nodes may come after the ways).
- **OSM XML**: the same two ways (nodes first and `nd ref`, or `nd` with `lat` and `lon` from `out geom`). Read with `roxmltree`
  (pure Rust, no unsafe), with DTDs refused and a limit on nodes.

Everything in a file is hostile: sizes are capped (`MAX_ELEMENTS`, `MAX_WAY_POINTS`, `MAX_BUILDINGS`, `MAX_RING_CHECK`), coordinates
off the Earth become "missing", tag text is cut to 256 characters, and a feature that cannot be a building is counted in the
reply, never a failure.

## Which buildings

A closed way tagged `building` (not `no`), or a `type=multipolygon` relation tagged `building`. A relation's outer ways are joined
end to end (either direction) into rings; its inner ways are courtyards; two outlines are two buildings (`relation/5`,
`relation/5#2`). `building:part` is left out unless `parts` is true. Outlines must close, have three corners, enclose area and not
cross themselves (`Model::check` does not look for crossing in a mass; `osm.rs` does, up to 2,000 corners).

**Height**, mm: `height` (`12`, `12.5`, `12 m`, `12,5`, `40 ft`, `40'`, `40' 6"`), else `building:levels` times `level_height`
(3000), else `default_height` (6000; the reply counts how many took it). `min_height` or `building:min_level` times
`level_height` lifts a part off the ground.

## What it makes

A `Mass` for each building: its outline counter-clockwise in project mm (through `Projector`, built once for the whole import),
`height` = height - min_height, `base_offset` = the lowest ground under the outline's corners and middle (`Tin::height_at` of the
toposurface; the only one, or `terrain`) minus the level's elevation, plus `min_height`. With no terrain, or outside it
(`off_terrain` in the reply), it stands at the level. A Mass cannot have a courtyard, so one is filled (`courtyards_filled`).

Each keeps the project parameters `OSM Id` (`way/123`), `OSM Building`, `OSM Name` and `OSM Levels` (text, declared for masses by
the command). **Importing again** updates a mass with the same `OSM Id` in place (same id and GUID, so walls made from it keep their
link; what you added to its parameters stays); `replace: false` makes new ones. Masses with other ids are never touched.

**Walls on request**: `detail: ["way/123", …]` (up to 50) also runs `wall_by_face`, `floor_by_face` and `roof_by_face` on those
masses. Everything else stays a mass: a district of thousands is one command, one model check, one undo step.

## Checking it

`bimcraft-cli run --cmd 'geolocate {...}' --cmd 'import_osm {"path": "block.json"}' --save /tmp/block.ifc` and `python
ci/validate_ifc.py /tmp/block.ifc`; masses export as `IfcBuildingElementProxy`. Look at the plan PNG (`--save x.png`).

## Not yet

Roads, water and land use (a CAD-style drawing with layers is the likely route: the CAD import holds thousands of polylines as one
element with layer visibility); GeoJSON; fetching from Overpass (the `geofetch` crate, with the map picker); roof shapes
(`roof:shape`), and walls for a whole district.
