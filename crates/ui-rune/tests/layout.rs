//! The layout is kept by a test, as the root `CLAUDE.md` is kept to its word budget: egui's `lib.rs` grew to 8,000 lines because
//! nothing said stop. No source file of this crate passes `MOST` lines, apart from a module's tests (`tests.rs`, `tests/`), so a new
//! area is a new module and a slice of `App`, never more of one file.

use std::fs;
use std::path::{Path, PathBuf};

const MOST: usize = 1200;

fn sources(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_none_or(|n| n != "tests") {
                sources(&path, found)?;
            }
        } else if path.extension().is_some_and(|e| e == "rs") && path.file_name().is_some_and(|n| n != "tests.rs") {
            found.push(path);
        }
    }
    Ok(())
}

#[test]
fn no_module_grows_into_a_monolith() -> Result<(), String> {
    let mut found = Vec::new();
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut found)?;
    assert!(found.len() >= 10, "the sources were found: {}", found.len());
    let long: Vec<String> = found
        .iter()
        .filter_map(|p| {
            let lines = fs::read_to_string(p).ok()?.lines().count();
            (lines > MOST).then(|| format!("{} has {lines} lines", p.display()))
        })
        .collect();
    if long.is_empty() { Ok(()) } else { Err(format!("split these into modules (at most {MOST} lines each):\n{}", long.join("\n"))) }
}
