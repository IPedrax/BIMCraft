# Fetching OpenStreetMap and elevation

`fetch_osm` and `fetch_terrain` get a box's buildings and ground from the services and make them as `import_osm` and `toposurface` do.
The pure part is `crates/core/src/geo.rs` (boxes, tiles, the Overpass query, elevation grids, the `GeoSource` hook); the commands are in
`commands.rs`; the fetching is `crates/geofetch`, which only the desktop app attaches (`Session.geo`). A build without it (the CLI,
the MCP server, tests) refuses both commands: an agent fetches the Overpass JSON itself and gives it to [`import_osm`](osm-import.md) as
`data`, or brings terrain in with [`toposurface`](terrain.md). The desktop's `main` does not attach it yet, so nothing user-reachable
fetches until the Site tab does (its background job and the map picker for the box).

## The box

`Bbox` is [south, west, north, east] degrees. A command takes `bbox`, or `radius` metres each way round the project's origin (1000 when
neither; the project must be placed, as what is fetched is placed in it), and at most **25 km²**: `Bbox::check_area`. Every parameter is
checked, with an `import_osm` that imports nothing, before anyone is asked, so a refusal costs no fetch.

## Buildings

`fetch_osm` writes the query (`geo::overpass_query`: ways and multipolygon relations tagged `building`, and `building:part` when `parts`),
asks `GeoSource::overpass`, and runs `import_osm` on the text. `geofetch::overpass::Overpass` posts it as a form, asks a busy server
(429, 502, 503, 504) again twice after the pause it gives (1 to 30 s), refuses a reply that is not JSON with an `elements` list or that
says the query ran out of time or memory (an error that says to use a smaller box, never kept), and keeps a good one a day. The default
server is `overpass-api.de`; the user may name another (https, a host name, no port), added to the allow-list by name.

## Ground

`fetch_terrain` asks `GeoSource::elevation` for a grid, samples it at nodes every `spacing` metres (by default as fine as keeps it near
36,000 points, no finer than 10 m; a spacing that makes more than 50,000 is refused before the fetch), projects them, and makes a
[toposurface](terrain.md) clipped to the box. Heights are made **project heights**: with no height above the sea in the project and
`set_elevation` true (the default) the ground at the origin becomes `Location.elevation`, so project z = 0 is there; otherwise the
project's own is taken off (`z_mm = metres x 1000 - Location.elevation`). One undo step brings back the terrain and the height.

- **Terrarium** (default): `s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png`, 256 x 256 PNGs, metres = R x 256 + G + B / 256
  - 32768; the zoom is the coarsest at or below the spacing in metres a pixel, a coarser one when the box needs more than 100 tiles; the
  tiles are laid together and sampled onto longitude and latitude, so a seam is no seam. Kept a month.
- **Copernicus GLO-30** (`source: copernicus`): one-degree cloud-optimised GeoTIFFs in `copernicus-dem-30m.s3.amazonaws.com`, read
  by range requests in 128 KiB blocks (kept on disk), decoded with the `tiff` crate (float32, deflate, floating-point predictor), only the
  chunks a box needs. Placement comes from the file's own tags. A tile the bucket lacks (403 or 404, as over open water) leaves a hole.

## Credits and terms

OpenStreetMap data is (c) OpenStreetMap contributors under the ODbL; Terrarium is made from USGS 3DEP and SRTM, GMTED2010, ETOPO1 and
other sources (see the Joerd `attribution.md`); Copernicus DEM GLO-30 is (c) DLR e.V. 2010-2014 and (c) Airbus Defence and Space GmbH
2014-2018 provided under COPERNICUS by the EU and ESA. `GeoSource::attribution` has each wording and the commands' replies carry it; the
desktop must show it with the data. **Read each source's own terms before shipping**: the sandbox that built this could not open them.
Overpass's public servers ask for fair use (one request at a time, small boxes, a User-Agent that says who is asking: BIMCraft's does).

## The desktop does not phone home unasked, or in a Flatpak

`Geofetch::desktop` gives none when `BIMCRAFT_NO_NETWORK` is set or in a Flatpak (which is not given the network). The cache folder is
`cache` beside the settings file (`geofetch::cache_dir(favorites::default_path())`); with none, nothing is kept.

## Not yet

The desktop attach, a background job so the window never waits, and the Site tab's map picker (a rectangle on a zoomable map); an
agent's `fetch_*` through `mcp --connect` would run on the window's thread until then. Copernicus as the default (once checked against
a real tile), or both with a fallback. Roads, water and land use.
