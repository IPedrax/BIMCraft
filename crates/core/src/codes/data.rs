//! The building codes BIMCraft ships: `data/codes/<discipline>/<code>.json`, one file a code, and the region presets of
//! `data/codes/presets.json`. A new country is a new file here and a line below; a test fails on a file of the folder
//! that isn't listed.

use super::{BuildingCode, Preset};

/// Each file, by its path under `data/codes/`.
macro_rules! files {
    ($($path:literal),* $(,)?) => {
        pub(crate) const FILES: &[(&str, &str)] = &[$(($path, include_str!(concat!("../../data/codes/", $path)))),*];
    };
}

files! {
    "architectural/generic.json",
    "architectural/nbr-9050.json",
    "architectural/nbr-9077.json",
    "architectural/ibc.json",
    "architectural/din.json",
    "architectural/jis.json",
    "structural/generic.json",
    "structural/nbr-6118.json",
    "electrical/generic.json",
    "mechanical/generic.json",
    "plumbing/generic.json",
    "plumbing/nbr.json",
    "plumbing/cbpmesp-it22.json",
}

/// Every code the files hold. One that doesn't read is left out, which a test keeps from happening.
pub(crate) fn read() -> Vec<BuildingCode> {
    FILES.iter().filter_map(|(_, text)| serde_json::from_str::<BuildingCode>(text).ok()).collect()
}

/// The region presets.
pub(crate) fn presets() -> Vec<Preset> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct File {
        presets: Vec<Preset>,
    }
    serde_json::from_str::<File>(include_str!("../../data/codes/presets.json")).map(|f| f.presets).unwrap_or_default()
}
