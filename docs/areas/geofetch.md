# Fetching OpenStreetMap and elevation

`fetch_osm` and `fetch_terrain` get a box's buildings and ground from the services and make them as `import_osm` and `toposurface` do.
The pure part is `crates/core/src/geo.rs` (boxes, tiles, the Overpass query, elevation grids, the `GeoSource` hook); the commands are in
`commands.rs`; the fetching is `crates/geofetch`, which only the desktop app attaches (`Session.geo`). A build without it (the CLI,
the MCP server, tests) refuses both commands: an agent fetches the Overpass JSON itself and gives it to [`import_osm`](osm-import.md) as
`data`, or brings terrain in with [`toposurface`](terrain.md). The desktop's `main` attaches it, and the [Site tab](#the-site-tab) is what asks it.

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

## The Site tab

The window never waits on the network. `crates/ui-egui/src/site.rs` takes the box's area and options on the window's thread, asks the
fetcher the session holds on a thread of its own (`Job`, one at a time, the busy box with Cancel, as reading a Revit file does), and
when the answer comes hands it to the **unchanged** `fetch_osm` or `fetch_terrain` through `geo::Prefetched`: a `GeoSource` that holds
that one reply (or grid) for that one box, query, spacing and source and answers only it, taking its credits from the real fetcher.
`session.geo` is swapped for it for the length of the command and put back whatever the command says. So there is one validation path,
one undo step, one report and one attribution line, and a refusal (no location, a bad height) is said in the box. Cancel drops what the
thread brings. An agent's `fetch_*` through `mcp --connect` still runs on the window's thread (agents wait while a fetch is on).
`geo::default_spacing` is the spacing both the box and the command default to.

`apps/bimcraft/src/main.rs` is the only place that builds the fetcher (`session.geo = Geofetch::desktop(..)`): `ui-egui` never names
`bimcraft-geofetch` or an HTTP crate (`the_window_library_fetches_only_through_what_the_app_attaches` reads the manifests). Where
`desktop` gives none (`BIMCRAFT_NO_NETWORK`, a Flatpak) the Site tab's boxes say there is no network and their Fetch is off.

## The map the picker is drawn on

`GeoSource::tile(zoom, x, y)` (empty by default: "no map tiles") is the picker's map, and `geofetch::tiles::OsmTiles` answers it from
OpenStreetMap's standard tiles, `https://tile.openstreetmap.org/{z}/{x}/{y}.png` (one host, in `Allow::standard`), one tile at a time,
only when the picker is on screen and shows it: decoded to RGBA (a palette, grey or 16-bit PNG is brought to 8-bit colour; 256 x 256
only, a header that claims more is refused before anything is made), kept two weeks (the tile policy asks for at least a week), a
reply that is not a tile is kept nowhere. The usage policy (operations.osmfoundation.org/policies/tiles) is kept to: BIMCraft's own
`User-Agent`, caching, no bulk fetching or fetching ahead, two connections at most (the window's loader has two threads and drops
what is no longer in view), and the credit, "© OpenStreetMap contributors", in the corner of the map. **Read the policy before
shipping**: the sandbox that built this could not open it, and heavy use (many users) may need a tile server of the project's own.
The pure part is `bimcraft_core::map` (a view of the map, its pixels, pan, zoom about a point, the tiles that cover a window, a drag as
a box, a graticule); `Prefetched` hands `tile` on to the real fetcher.

## Not yet

A bundled world outline for the picker with no network (it has its lines of latitude and longitude instead: the outline would have
to come from a public-domain dataset such as Natural Earth, checked in); tiles from a server of the user's own. Copernicus as the default (once checked against
a real tile), or both with a fallback. Roads, water and land use.
