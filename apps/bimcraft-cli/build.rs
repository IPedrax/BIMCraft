//! Puts the commit the CLI is built from into the build: `BIMCRAFT_GENERATOR_VERSION` is `bimcraft-<release>+<commit>`, with
//! `.dirty` after the commit when tracked files have uncommitted changes. `BIMCRAFT_COMMIT` stands in for a build with no `.git`.
//! `BIMCRAFT_GENERATOR_VERSION` set for the build replaces the whole version (a pipeline naming its own releases); `deliver`
//! still refuses one without a `+`. It is empty when nothing is known, and `deliver` then refuses. The version is fixed here,
//! at build time, never read at run time.

use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn watch(path: &str) {
    if Path::new(path).exists() {
        println!("cargo:rerun-if-changed={path}");
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=BIMCRAFT_GENERATOR_VERSION");
    if let Ok(v) = std::env::var("BIMCRAFT_GENERATOR_VERSION")
        && !v.trim().is_empty()
    {
        println!("cargo:rustc-env=BIMCRAFT_GENERATOR_VERSION={}", v.trim());
        println!("cargo:rustc-env=BIMCRAFT_GENERATOR_VERSION_FROM=env");
        return;
    }
    println!("cargo:rerun-if-env-changed=BIMCRAFT_COMMIT");
    let release = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let commit = match std::env::var("BIMCRAFT_COMMIT") {
        Ok(c) if !c.trim().is_empty() => Some(c.trim().to_string()),
        _ => git(&["rev-parse", "--short=7", "HEAD"]).filter(|c| !c.is_empty()).map(|c| {
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_none_or(|s| !s.is_empty());
            if dirty { format!("{c}.dirty") } else { c }
        }),
    };
    // Re-run when the commit moves or tracked files change: HEAD, the branch it names, the index, and the sources.
    for what in ["HEAD", "index", "packed-refs"] {
        if let Some(p) = git(&["rev-parse", "--git-path", what]) {
            watch(&p);
        }
    }
    if let Some(r) = git(&["rev-parse", "--symbolic-full-name", "HEAD"]).filter(|r| r.starts_with("refs/"))
        && let Some(p) = git(&["rev-parse", "--git-path", &r])
    {
        watch(&p);
    }
    for p in ["../../crates", "../../apps", "../../Cargo.toml", "../../Cargo.lock"] {
        watch(p);
    }
    let version = match commit {
        Some(c) if !release.is_empty() => format!("bimcraft-{release}+{c}"),
        _ => String::new(),
    };
    println!("cargo:rustc-env=BIMCRAFT_GENERATOR_VERSION={version}");
    println!("cargo:rustc-env=BIMCRAFT_GENERATOR_VERSION_FROM=git");
}
