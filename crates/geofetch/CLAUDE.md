# bimcraft-geofetch

Fetches OpenStreetMap buildings (Overpass) and the map tiles of the area picker for the desktop app, behind the core's `GeoSource`
hook (`bimcraft_core::geo`). With `crates/update` it is the only code that phones home: the desktop's `main` attaches it, and the
core, the CLI and the MCP server do not depend on it (a test in `lib.rs` reads their manifests). It depends on the core (for `Bbox`,
`GeoSource`), never the other way. What it takes from the network is hostile: a request goes only to a host and path `net::Allow`
names (HTTPS, no port or credentials, no `..` or escapes, **no query string**; a server of the user's own is added by name and only
if it is https with a host name), a redirect is followed by hand under the same rule, a reply is read to an exact cap, a PNG is
decoded under limits after its header is checked, and what is kept on disk is checked again when it is read. The write-up is
[geofetch](../../docs/areas/geofetch.md); the modules are in the root `CLAUDE.md`.

- The network stays behind `net::Net` (`testing::Fake` for a table of answers; `net::Ureq::loopback` against a server on this
  machine). **No test touches a real service**, and the sandbox cannot reach them: `examples/fetch.rs` (buildings) is how a person tries them.
- Credits are part of the data: `GeoSource::attribution`, and `fetch_osm` puts it in its reply. Read OpenStreetMap's own terms (the
  ODbL, the tile usage policy) before shipping.
- What a server says is hostile, and is still the best clue: an error status keeps its number and carries a short reason made of the
  first bytes of its page (`net::reason`, one line of plain text), so a 400 names the parameter it did not like.
- Time is injected (`Cache::with_clock`, `Overpass::with_pause`), so no test waits or depends on the date.
- `cargo test -p bimcraft-geofetch` runs in a few seconds.

## Modules

`net.rs` HTTPS to named hosts, `cache.rs`, `overpass.rs`, `tiles.rs` (the picker's map), `testing.rs` (a fake network).
