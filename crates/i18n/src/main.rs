use bimcraft_i18n::catalog::{self, Document};
use std::fs;
use std::path::{Path, PathBuf};

fn template(root: &Path, domain: &str) -> PathBuf {
    root.join("translations").join(domain).join(format!("bimcraft-{domain}.pot"))
}

fn run(root: &Path, action: &str) -> Result<(), String> {
    for domain in ["project", "ui"] {
        let path = template(root, domain);
        let source: Document = catalog::extract(root, domain)?;
        let rendered = catalog::render(&source);
        match action {
            "extract" | "update" => fs::write(&path, &rendered).map_err(|e| e.to_string())?,
            "check" => {
                if fs::read_to_string(&path).map_err(|e| e.to_string())? != rendered {
                    return Err(format!("{} is out of date; run cargo i18n update", path.display()));
                }
            }
            "stats" => {}
            _ => return Err("usage: cargo i18n <extract|update|check|stats> [repository root]".into()),
        }
        let mut paths = fs::read_dir(path.parent().ok_or("template has no directory")?)
            .map_err(|e| e.to_string())?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        paths.sort();
        for path in paths.iter().filter(|p| p.extension().is_some_and(|e| e == "po")) {
            let mut document = catalog::read(path)?;
            if action == "update" {
                document = catalog::merge(document, &source);
                fs::write(path, catalog::render(&document)).map_err(|e| e.to_string())?;
            }
            if action == "check" {
                for key in document.entries.keys() {
                    if !source.entries.contains_key(key) {
                        return Err(format!("{}: obsolete active message {:?}; run cargo i18n update", path.display(), key));
                    }
                }
            }
            let translated = document.entries.values().filter(|e| !e.fuzzy && !e.translation.trim().is_empty()).count();
            println!(
                "{domain}/{}: {translated}/{} translated",
                path.file_name().unwrap_or_default().to_string_lossy(),
                source.entries.len()
            );
        }
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let action = args.next().unwrap_or_else(|| "check".into());
    let root = args.next().map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    run(&root, &action)
}
