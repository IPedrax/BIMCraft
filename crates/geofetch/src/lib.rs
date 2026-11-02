//! Fetching for the desktop app: OpenStreetMap buildings from Overpass and elevation from tiles, behind the core's `GeoSource`. The
//! only code besides the updater that phones home, and only the desktop's `main` attaches it: the core, the CLI and the MCP server
//! do not depend on it (a test reads their manifests).
//!
//! What comes from the network is hostile. A request goes only to a host and path [`net::Allow`] names, over HTTPS, with no port or
//! credentials; a redirect is followed by hand under the same rule; a reply is read up to a cap; a picture is decoded under size
//! limits; what is kept on disk is checked again when it is read. The network sits behind [`net::Net`], so the tests use a table of
//! answers or a server on this machine, never the real services.

pub mod cache;
pub mod copernicus;
pub mod layout;
pub mod net;
pub mod overpass;
pub mod terrarium;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bimcraft_core::geo::{Bbox, Elevation, Fetcher, GeoSource, Reply};

use crate::cache::Cache;
use crate::copernicus::Copernicus;
use crate::net::{Allow, Net, OVERPASS, Ureq};
use crate::overpass::Overpass;
use crate::terrarium::Terrarium;

/// The elevation sources there are, as `source` names them, the first being the one used when none is named.
pub const SOURCES: [&str; 2] = ["terrarium", "copernicus"];

/// What OpenStreetMap's licence asks to be said where its data is used.
pub const OSM_ATTRIBUTION: &str =
    "Map data (c) OpenStreetMap contributors, available under the Open Database Licence (ODbL): https://www.openstreetmap.org/copyright";

/// The fetcher: Overpass and an elevation source, over one network and one cache.
pub struct Geofetch {
    overpass: Overpass,
    terrarium: Terrarium,
    copernicus: Copernicus,
}

impl Geofetch {
    pub fn new(net: Arc<dyn Net>, cache: Arc<Cache>, endpoint: &str) -> Geofetch {
        Geofetch {
            overpass: Overpass::new(net.clone(), cache.clone(), endpoint),
            terrarium: Terrarium::new(net.clone(), cache.clone()),
            copernicus: Copernicus::new(net, cache),
        }
    }

    /// The fetcher the desktop app attaches, or none where it is not wanted: `BIMCRAFT_NO_NETWORK` set, or a Flatpak, which is not
    /// given the network. `cache_dir` is where replies and tiles are kept (none: not kept); `endpoint` is an Overpass server of the
    /// user's own, which must be https and named (an address of this machine or network is refused).
    pub fn desktop(cache_dir: Option<PathBuf>, endpoint: Option<&str>) -> Result<Option<Fetcher>, String> {
        let env = |name: &str| std::env::var(name).ok();
        Self::desktop_in(&env, Path::new("/.flatpak-info").exists(), cache_dir, endpoint)
    }

    /// [`Geofetch::desktop`], told the environment and whether it is in a Flatpak.
    pub fn desktop_in(
        env: &dyn Fn(&str) -> Option<String>,
        flatpak: bool,
        cache_dir: Option<PathBuf>,
        endpoint: Option<&str>,
    ) -> Result<Option<Fetcher>, String> {
        let allow = match endpoint {
            Some(e) => Allow::standard().with_endpoint(e)?,
            None => Allow::standard(),
        };
        if flatpak || env("FLATPAK_ID").is_some_and(|v| !v.is_empty()) || env("BIMCRAFT_NO_NETWORK").is_some_and(|v| !v.is_empty()) {
            return Ok(None);
        }
        let fetcher = Geofetch::new(Arc::new(Ureq::new(allow)), Arc::new(Cache::new(cache_dir)), endpoint.unwrap_or(OVERPASS));
        Ok(Some(Arc::new(fetcher)))
    }
}

/// The folder replies and tiles are kept in, beside the file the app's settings are kept in (`favorites::default_path()`).
pub fn cache_dir(settings_file: &Path) -> PathBuf {
    settings_file.with_file_name("cache")
}

impl GeoSource for Geofetch {
    fn overpass(&self, bbox: &Bbox, query: &str) -> Result<Reply, String> {
        self.overpass.fetch(bbox, query)
    }

    fn elevation(&self, bbox: &Bbox, spacing: f64, source: Option<&str>) -> Result<Elevation, String> {
        match source.unwrap_or(SOURCES[0]) {
            "terrarium" => {
                let (grid, cached) = self.terrarium.grid(bbox, spacing)?;
                Ok(Elevation { grid, source: "terrarium".into(), cached })
            }
            "copernicus" => {
                let (grid, cached) = self.copernicus.grid(bbox, spacing)?;
                Ok(Elevation { grid, source: "copernicus".into(), cached })
            }
            other => Err(format!("there is no elevation source `{other}`: the sources are {}", SOURCES.join(", "))),
        }
    }

    fn attribution(&self, what: &str) -> String {
        match what {
            "osm" => OSM_ATTRIBUTION.into(),
            "terrarium" => terrarium::ATTRIBUTION.into(),
            "copernicus" => copernicus::ATTRIBUTION.into(),
            other => format!("Data from {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::OVERPASS;
    use crate::testing::{Fake, Scratch};

    const GOOD: &str = r#"{"version":0.6,"elements":[]}"#;

    fn fetcher(net: Arc<Fake>) -> Geofetch {
        Geofetch::new(net, Arc::new(Cache::new(None)), OVERPASS)
    }

    fn block() -> Bbox {
        Bbox::new(-23.56, -46.64, -23.54, -46.62).unwrap()
    }

    #[test]
    fn the_core_asks_through_the_trait_and_overpass_answers() {
        let net = Arc::new(Fake::new().post(OVERPASS, GOOD.as_bytes()));
        let f: Fetcher = Arc::new(fetcher(net.clone()));
        let r = f.overpass(&block(), "[out:json];out;").unwrap();
        assert_eq!((r.text.as_str(), r.cached), (GOOD, false));
        assert_eq!(net.asked().len(), 1);
    }

    #[test]
    fn the_source_is_terrarium_unless_another_is_named_and_one_that_is_not_there_is_refused() {
        let net = Arc::new(Fake::new());
        let f = fetcher(net.clone());
        // Nothing to fetch from: the error is the tile's, which shows that terrarium is what was asked, named or not.
        for named in [None, Some("terrarium")] {
            let e = f.elevation(&block(), 20.0, named).unwrap_err();
            assert!(e.contains("tile 13/"), "{named:?}: {e}");
        }
        // Copernicus is a source too: with nothing to fetch, it is the lack of its tile that is the answer.
        assert!(f.elevation(&block(), 20.0, Some("copernicus")).unwrap_err().contains("no Copernicus tile"));
        for bad in ["", "srtm", "terrarium/../x", "TERRARIUM"] {
            let e = f.elevation(&block(), 20.0, Some(bad)).unwrap_err();
            assert!(e.contains("no elevation source") && e.contains("terrarium"), "{bad}: {e}");
        }
    }

    #[test]
    fn each_kind_of_data_has_its_credit() {
        let f = fetcher(Arc::new(Fake::new()));
        assert!(f.attribution("osm").contains("OpenStreetMap") && f.attribution("osm").contains("ODbL"));
        assert!(f.attribution("terrarium").contains("Terrain Tiles") && f.attribution("terrarium").contains("joerd"));
        assert!(f.attribution("copernicus").contains("Copernicus DEM GLO-30") && f.attribution("copernicus").contains("Airbus"));
        assert!(f.attribution("something").contains("something"));
    }

    #[test]
    fn the_desktop_has_it_unless_a_flatpak_or_the_user_says_no() {
        let none = |_: &str| None;
        let set = |name: &'static str, value: &'static str| move |n: &str| (n == name).then(|| value.to_string());
        let dir = Scratch::new("desktop");
        assert!(Geofetch::desktop_in(&none, false, Some(dir.0.clone()), None).unwrap().is_some());
        assert!(Geofetch::desktop_in(&none, true, None, None).unwrap().is_none(), "a Flatpak has no network");
        assert!(Geofetch::desktop_in(&set("FLATPAK_ID", "org.example"), false, None, None).unwrap().is_none());
        assert!(Geofetch::desktop_in(&set("BIMCRAFT_NO_NETWORK", "1"), false, None, None).unwrap().is_none());
        assert!(Geofetch::desktop_in(&set("BIMCRAFT_NO_NETWORK", ""), false, None, None).unwrap().is_some(), "empty is not set");
        // A server of the user's own, if it is one that may be used.
        assert!(Geofetch::desktop_in(&none, false, None, Some("https://overpass.example.org/api/interpreter")).unwrap().is_some());
        for bad in ["http://overpass.example.org/api/interpreter", "https://127.0.0.1/api/interpreter", "overpass"] {
            assert!(Geofetch::desktop_in(&none, false, None, Some(bad)).is_err(), "{bad}");
        }
        assert_eq!(cache_dir(Path::new("/home/a/.config/bimcraft/favorites.json")), Path::new("/home/a/.config/bimcraft/cache"));
    }
}
