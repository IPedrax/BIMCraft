# Fetching OpenStreetMap buildings

`fetch_osm` gets a box's buildings from Overpass and makes them as `import_osm` does. The pure part is `crates/core/src/geo.rs`
(boxes, map tiles, the Overpass query, the `GeoSource` hook); the command is in `commands.rs`; the fetching is `crates/geofetch`, which
only the desktop app attaches (`Session.geo`). A build without it (the CLI, the MCP server, tests) refuses the command: an agent fetches
the Overpass JSON itself and gives it to [`import_osm`](osm-import.md) as `data`. The desktop's `main` attaches it, and the
[Massing tab's site tools](#the-massing-tabs-site-tools) are what ask it.

Ground, heights and aerial imagery are not fetched: buildings stand at their level (see [OSM import](osm-import.md)). Terrain from an
elevation service and imagery over it may come in a follow-up.

## The box

`Bbox` is [south, west, north, east] degrees. A command takes `bbox`, or `radius` metres each way round the project's internal origin (1000
when neither), and at most **25 km²**: `Bbox::check_area`. Either way the active site needs a map system and a place
(`location {name, crs, latitude, longitude}`, Manage > Location: [coordinates](coordinates.md)), as what is fetched is placed through
it: `geo::Projector` reads the site's `Georeference` and its CRS once (`crs::Projector`) for all the points of a fetch, and puts project
north on grid north, as the site does unless it is turned. Every parameter is checked, with an `import_osm` that imports nothing,
before anyone is asked, so a refusal costs no fetch.

## Buildings

`fetch_osm` writes the query (`geo::overpass_query`: ways and multipolygon relations tagged `building`, `building:part`, `man_made=column`,
`type=building` relations, and entrance and door nodes), asks `GeoSource::overpass`, and runs `import_osm` on the text.
`geofetch::overpass::Overpass` posts it as a form, asks a busy server (429, 502, 503, 504) again twice after the pause it gives (1 to
30 s), refuses a reply that is not JSON with an `elements` list or that says the query ran out of time or memory (an error that says to
use a smaller box, never kept), and keeps a good one a day. The default server is `overpass-api.de`; the user may name another (https,
a host name, no port), added to the allow-list by name.

## Credits and terms

OpenStreetMap data is (c) OpenStreetMap contributors under the ODbL. `GeoSource::attribution` has the wording and the command's reply
carries it; the desktop must show it with the data. Overpass's public servers ask for fair use (one request at a time, small boxes, a
User-Agent that says who is asking: BIMCraft's does). **Read the terms before shipping**: the sandbox that built this could not open them.

## The desktop does not phone home unasked, or in a Flatpak

`Geofetch::desktop` gives none when `BIMCRAFT_NO_NETWORK` is set or in a Flatpak (which is not given the network). The cache folder is
`cache` beside the settings file (`geofetch::cache_dir(favorites::default_path())`); with none, nothing is kept.

## The Massing tab's site tools

The window never waits on the network. `crates/ui-egui/src/site/` takes the box's area and options on the window's thread, asks the
fetcher the session holds on a thread of its own (`Job`, one at a time, the busy box with Cancel, as reading a Revit file does), and
when the answer comes hands it to the **unchanged** `fetch_osm` through `geo::Prefetched`: a `GeoSource` that holds that one reply for
that one box and query and answers only it, taking its credits from the real fetcher. `session.geo` is swapped for it for the length of
the command and put back whatever the command says. So there is one validation path, one undo step, one report and one attribution
line, and a refusal (no place on Earth, a bad height) is said in the box. Cancel drops what the thread brings. An agent's `fetch_osm`
through `mcp --connect` still runs on the window's thread (agents wait while a fetch is on).

`apps/bimcraft/src/main.rs` is the only place that builds the fetcher (`session.geo = Geofetch::desktop(..)`): `ui-egui` never names
`bimcraft-geofetch` or an HTTP crate (`the_window_library_fetches_only_through_what_the_app_attaches` reads the manifests). Where
`desktop` gives none (`BIMCRAFT_NO_NETWORK`, a Flatpak) the box says there is no network and its Fetch is off.

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

Elevation and terrain from it, and aerial imagery (both left out to keep this to buildings); a bundled world outline for the picker with
no network (it has its lines of latitude and longitude instead: the outline would have to come from a public-domain dataset such as
Natural Earth, checked in); tiles from a server of the user's own. Roads, water and land use.
