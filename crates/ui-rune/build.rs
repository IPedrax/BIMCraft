use std::path::Path;

/// The interface's catalogs, the ones the egui front end reads: `translations/ui/*.po`, both UIs speaking with one voice.
fn main() -> Result<(), String> {
    let root = std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?;
    let out = std::env::var("OUT_DIR").map_err(|e| e.to_string())?;
    bimcraft_i18n::catalog::compile(&Path::new(&root).join("../../translations/ui"), &Path::new(&out).join("catalogs.rs"))
}
