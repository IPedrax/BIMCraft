//! Fetching for the desktop app: OpenStreetMap buildings from Overpass and elevation from tiles, behind the core's `GeoSource`. The
//! only code besides the updater that phones home, and only the desktop's `main` attaches it: the core, the CLI and the MCP server
//! do not depend on it (a test reads their manifests).
//!
//! What comes from the network is hostile. A request goes only to a host and path [`net::Allow`] names, over HTTPS, with no port or
//! credentials; a redirect is followed by hand under the same rule; a reply is read up to a cap; a picture is decoded under size
//! limits; what is kept on disk is checked again when it is read. The network sits behind [`net::Net`], so the tests use a table of
//! answers or a server on this machine, never the real services.

pub mod cache;
pub mod net;
pub mod overpass;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
