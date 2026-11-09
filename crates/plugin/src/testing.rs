//! What a test — this crate's or the window's — needs to run a real plugin module without one in the repository.
//!
//! The module comes from `crates/plugin/tests/sample-plugin`, a fixture built with the guest SDK for
//! `wasm32-unknown-unknown` before the tests run. Built, [`sample_wasm`] is its bytes; not built, it says what to run.

use std::path::{Path, PathBuf};

/// Where a build of the fixture puts its module, under `target`: the folder, then the target the build names.
fn candidates() -> Vec<PathBuf> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sample-plugin");
    let module = Path::new("wasm32-unknown-unknown/release/sample_plugin.wasm");
    // A nested workspace keeps a `target` of its own, unless the build's `--target-dir` says otherwise; and the
    // workspace's `target` is where the CI step that builds the fixture leaves it.
    let mut all = vec![fixture.join("target").join(module), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target").join(module)];
    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        all.push(Path::new(&target).join(module));
    }
    all
}

/// The command that builds the fixture.
pub const BUILD: &str = "rustup target add wasm32-unknown-unknown && \
                         cargo build --manifest-path crates/plugin/tests/sample-plugin/Cargo.toml \
                         --target wasm32-unknown-unknown --release";

/// The fixture's module, when it was built.
pub fn sample_wasm() -> Option<Vec<u8>> {
    candidates().into_iter().find_map(|path| std::fs::read(path).ok())
}

/// The fixture's module, or a skip with what to run; inside CI, a failure — the build must have happened there.
pub fn sample_wasm_or_skip() -> Option<Vec<u8>> {
    let module = sample_wasm();
    if module.is_none() {
        assert!(
            std::env::var_os("CI").is_none(),
            "the sample plugin isn't built, and CI runs every test: build it with `{BUILD}` before the tests"
        );
        eprintln!("SKIPPED: the sample plugin isn't built; run `{BUILD}`");
    }
    module
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_module_that_was_built_is_a_web_assembly_module() {
        let Some(module) = super::sample_wasm() else { return };
        assert_eq!(&module[..4], b"\0asm", "the build is WebAssembly");
    }
}
