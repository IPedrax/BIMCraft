use std::path::Path;

fn main() -> Result<(), String> {
    let root = std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?;
    let out = std::env::var("OUT_DIR").map_err(|e| e.to_string())?;
    bimcraft_i18n::catalog::compile(&Path::new(&root).join("../../translations/project"), &Path::new(&out).join("catalogs.rs"))?;
    families(&Path::new(&root).join("data/shape-families"), &Path::new(&out).join("shape_families.rs"))
}

/// The built-in families: every `<name>.json` in the directory but `shapes.json`, sorted by name, each compiled in.
fn families(dir: &Path, out: &Path) -> Result<(), String> {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut found: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        let file = entry.file_name().into_string().map_err(|n| format!("{n:?} is not UTF-8"))?;
        match file.strip_suffix(".json") {
            Some(name) if name != "shapes" => {
                println!("cargo:rerun-if-changed={}", entry.path().display());
                found.push((name.to_string(), entry.path()));
            }
            _ => {}
        }
    }
    found.sort();
    let mut text = String::from(
        "/// The built-in families, by name: the files of `data/shape-families`, made by `build.rs`.\npub static BUILTIN: &[(&str, &str)] = &[\n",
    );
    for (name, path) in &found {
        text.push_str(&format!("    ({name:?}, include_str!({:?})),\n", path.display().to_string()));
    }
    text.push_str("];\n");
    std::fs::write(out, text).map_err(|e| format!("{}: {e}", out.display()))
}
