# bimcraft-geofetch

Fetches OpenStreetMap buildings (Overpass) and elevation (Terrarium tiles, Copernicus GLO-30) for the desktop app, behind the
core's `GeoSource` hook (`bimcraft_core::geo`). With `crates/update` it is the only code that phones home: the desktop's `main`
attaches it, and the core, the CLI and the MCP server do not depend on it (a test in `lib.rs` reads their manifests). It depends on
the core (for `Bbox`, `ElevationGrid`, `GeoSource`), never the other way. What it takes from the network is hostile: a request goes
only to a host and path `net::Allow` names (HTTPS, no port or credentials, no `..` or escapes; a server of the user's own is added
by name and only if it is https with a host name), a redirect is followed by hand under the same rule, a reply is read to an exact
cap, a PNG is decoded under limits after its header is checked, a GeoTIFF is read by range requests with its size claims checked,
and what is kept on disk is checked again when it is read. The write-up is [geofetch](../../docs/areas/geofetch.md); the modules
are in the root `CLAUDE.md`.

- The network stays behind `net::Net` (`testing::Fake` for a table of answers; `net::Ureq::loopback` against a server on this
  machine). **No test touches a real service**, and the sandbox cannot reach them: `examples/fetch.rs` (buildings, heights) and `examples/imagery.rs` (aerial imagery; its pure helpers are `probe.rs`) are how a person tries them.
- The Copernicus reader is tested against GeoTIFFs the tests write themselves (`copernicus.rs`), not a real tile; that is why
  Terrarium is the default source. Check it against a real tile before making it the default.
- Credits are part of the data: `GeoSource::attribution` for each source, and `fetch_osm` and `fetch_terrain` put them in their
  replies. Read the sources' own attribution documents before shipping (the Terrarium one is the Joerd `attribution.md`).
- Aerial imagery: the sources are the core's table (`bimcraft_core::imagery::SOURCES`), and `net::Allow::standard` takes each one's
  addresses and no others (a query is pinned whole, pair by pair); a tile is a PNG or a JPEG of 256 by 256 or it is refused, and a
  tile with no imagery (404, or one flat colour) is left clear. Try the services with `examples/imagery.rs`; its notes are in
  [geofetch](../../docs/areas/geofetch.md).
- Time is injected (`Cache::with_clock`, `Overpass::with_pause`), so no test waits or depends on the date.
- `cargo test -p bimcraft-geofetch` runs in a few seconds.
