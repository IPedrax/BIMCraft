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
project's own is taken off (`z_mm = metres x 1000 - Location.elevation`). **The buildings `import_osm` made** (the masses with an `OSM Id`) are stood on the new ground, whichever was fetched first (`stand_buildings`, true by default): each at the lowest ground under its footprint, as `place_on_terrain` does, and what was made from them by face follows (`update_by_face`'s step, `follow_one`); one not over the ground stays and is counted, one whose shell cannot follow (a face is gone) stands while its shell is left, named (`stood` in the reply: `moved`, `off_terrain`, `shells_updated`, `shells_left`). One undo step brings back the terrain, the height and the buildings' bases.

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

## Aerial imagery (being built)

Aerial imagery for the terrain is built in stages, each tried on a real machine, because the sandbox that writes the code cannot reach
the services. Stage 0 is `examples/imagery.rs` over `probe.rs`, whose pure, tested helpers make a tile address from a template
(`{z}`, `{x}`, `{y}`, `{-y}`, and WMTS's `{TileMatrix}`, `{TileCol}`, `{TileRow}`), tell a reply from its first bytes, give the ground a
pixel stands for, and read a WMTS capabilities document (layers with their descriptions, styles, formats and sets; each set's levels, and
whether it is the ordinary Web Mercator pyramid; the tile addresses; fees and constraints). `imagery probe` fetches tiles of the sources
meant to be used (Flanders' and the Netherlands' orthophotos, USGS imagery, EOX's Sentinel-2 cloudless) at a place in each and prints a
table, flagging a tile that is one flat colour; `imagery caps URL|FILE [DIR] [LAYER]` prints what a service's document says, and the
address to try for a service that is asked by query; `imagery tile` tries any address. An address with a query string is fetched with a
plain client and the row says so, because the app's client refuses it (see below).

What the first runs on a real machine found (2026-11-03; they are the only checks of these services, and none is repeated by a test):

- **PDOK** (`service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0/{layer}/EPSG:3857/{z}/{x}/{y}.jpeg`, layers `Actueel_orthoHR`, `Actueel_ortho25`
  and one for each year): 256 px JPEG, down to zoom 19 and probably 20 (`z22` answers 400). A tile outside the coverage is an ordinary
  200 with a small JPEG, so "no imagery" is not an error status. Fees and constraints in the document: none; the licence (CC-BY 4.0) is
  on PDOK's dataset page.
- **USGS** (`basemap.nationalmap.gov/.../USGSImageryOnly/MapServer/tile/{z}/{y}/{x}`): 256 px JPEG, but 404 beyond zoom 16 at Washington
  DC (about 1.9 m a pixel), though its document lists 24 levels: a document does not say how fine the tiles really go, only a probe does.
- **EOX** (`tiles.maps.eox.at/wmts/1.0.0/{layer}/default/GoogleMapsCompatible/{z}/{y}/{x}.jpg`, https): layers are `s2cloudless_3857`
  (no year) and one for each year from 2018. Attribution is mandatory and carries links, and "additional restrictions may apply for
  individual layers as indicated in the respective abstract", so a layer's abstract is read before it is used.
- **Flanders** (`geo.api.vlaanderen.be/OMWRGBMRVL/wmts`, layer `omwrgbmrvl`, set `GoogleMapsVL`, 23 levels, 256 px): free of charge,
  with the terms on a Digitaal Vlaanderen page, and **no tile addresses of its own: it is asked by query string** (WMTS GetTile).

Two limits of this crate shaped what could be used, and both are lifted for imagery: `net::Allow` refuses any query string except the
whole, pinned one of a source that can only be asked by query (Flanders; never a general opening), and `imagery::ImageryTiles` reads
JPEG as well as PNG (`tiles::decode`, the picker's map, still reads only PNG). Bing, Esri, Maxar, Mapbox and Google are left out: as far
as the research found, their terms do not allow fetching them into a project.

**Found on a real machine (2026-11-04), Flanders works** once asked with an empty style: zoom 18 over Ghent, 0.375 m a pixel, 4 x 3 tiles,
0 % without imagery. The service takes **PNG only** (`image/jpeg is not a supported format`), about 92 KB a tile, and its matrices run 0 to 21
(22 and 23 are unknown, so the table's `max_zoom: 21` is exactly its finest; a tile at 21 is 26 KB, mostly upsampled); zooms 17 to 21 are proven. **A tile off a service's grid is an HTTP 400 naming
`TileOutOfRange`, not a 404** (Flanders: `Column 54613 is out of range, min: 66453`; PDOK: `outside the bounding box of the tile map`), so
`ImageryTiles` takes that 400, and only that one, as a tile with no imagery (`no_tile_here`): a box at the edge of a service's grid gives a
picture clear where there is no tile, not a failed fetch that falls through to a coarser source.

**Flanders and its 400** (the first run, the same day): the first address asked with `style=default` and every tile came back
HTTP 400, so Automatic over Belgium fell through to Sentinel-2 at 6 m a pixel. Every published request for this service family has an
**empty `style=`** and its capabilities list no style, so the table now asks with an empty one (`flanders_is_asked_with_an_empty_style...`;
the pinned-query rule takes an empty literal, a test says so). That this is the cause is a guess until a real run says so: **an error
status now carries the service's own words** (`net::reason`: up to 512 bytes of the error page, tags taken out, an OGC exception's code
and `locator`, which names the parameter it did not like, kept, one line of at most 200 characters, nothing for a body that is not text),
so the next 400 says which parameter. `imagery probe` asks Flanders at zoom 18 in four more ways as rows of their own (style left out,
`style=default`, the matrix as `GoogleMapsVL:{z}`, JPEG) so that one run shows which the service takes. A source that failed is never
silent: `fetch_imagery`'s reply has `skipped` (each source passed over before the one used, with what it said, cut at 300 characters),
the Site tab's result box lists them, and a refusal gives every source's reason, not only the last.

**The parts that are built:** `bimcraft_core::imagery` is the table of
sources that are allowed (those whose terms permit fetching their tiles into a project, each with its credit and where its terms are,
the finest and most local first), the plan (the finest zoom whose pixels are no smaller than the picture's, within the source's zooms
and a hundred tiles), the mosaic (checked before it is used), and the warp, which lays the mosaic into the project's own axes, north
up, clear where there is no imagery, by a lattice of mapped points (`Projector::lonlat` is the way back from degrees), tested against a
ground whose colour is a plain function of longitude and latitude with the project turned. `GeoSource::imagery` is the hook, and
`Prefetched` hands imagery back for the source and plan it was fetched for and no other. `imagery::ImageryTiles` asks at most four
tiles at once, takes a tile only if its bytes are a PNG or a JPEG of 256 by 256 pixels (told by the bytes), keeps what decodes for
thirty days, leaves a tile clear when the service has none (a 404, or a tile of one flat colour: PDOK sends white and EOX the ocean's
blue for places it has none of) and fails the fetch, naming the tile, for any other trouble. `net::Allow` takes each source's
addresses and no others: a REST address by its text and its numbers (one to ten digits) with nothing before, between or after, and an
address that can only be asked by query (Flanders) by its exact path and its whole query, pair by pair, in order, each value as
written or a number. `imagery warp` in the example does the whole of it for a place and writes the picture, the picture with a 100 m
grid, a world file (`world_file` gives its six numbers; it has no CRS, so a note says which) and a note of the source and its terms.
The `fetch_imagery` command, the planar texture and its drawing in the 3D view (processor and card) are built too ([terrain](terrain.md)). The Site tab's Aerial Imagery button runs it, says what it will ask for before it does (a line for each source: metres a pixel, tiles, about how many MB; `Plan::tiles`, `estimated_bytes`, `is_coarse`) and counts tiles while it runs (`ImageryTiles::progress`, two atomics, read through `GeoSource::imagery_progress`, which `Prefetched` hands on to the real fetcher). The plans' underlay is built too ([terrain](terrain.md#the-photograph-in-plans)). Not yet: the credit drawn in the views.

## Not yet

A bundled world outline for the picker with no network (it has its lines of latitude and longitude instead: the outline would have
to come from a public-domain dataset such as Natural Earth, checked in); tiles from a server of the user's own. Copernicus as the default (once checked against
a real tile), or both with a fallback. Roads, water and land use.
