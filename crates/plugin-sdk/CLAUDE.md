# bimcraft-plugin-sdk: the guest side

The SDK a plugin author builds against (`src/lib.rs`): `execute`, `note`, `host_info`, `export!`, and the linear-memory
entry points the host looks for. It is compiled into a plugin module (`wasm32-unknown-unknown`), not into BIMCraft.

- **The ABI is fixed in `crates/plugin/src/runtime.rs`.** `execute`, `log` and `info` are the only host imports; the
  module exports `memory`, `bimcraft_alloc`, `bimcraft_free` and `bimcraft_run`, and a reply packs the pointer in the high
  32 bits and the length in the low 32. A change on one side is a change on both, with `manifest::ABI` bumped, a WAT test
  in `crates/plugin/tests/fixtures` and a paragraph in `docs/areas/plugins.md`.
- **One unsafe block, at the boundary.** A WebAssembly guest receives raw pointers into its linear memory; no safe Rust
  API reads them. `read`, `write` and the `#[unsafe(no_mangle)]` exports are the whole of it, each saying why it is sound;
  the crate does not opt into the workspace lints because `unsafe_code` is forbidden there. Nothing else in the workspace
  may take `unsafe`.
- **A handler's `Err` is a failed call.** `export!` writes `{"bimcraftError": "..."}`, which the host turns back into an
  error, so the plugin's transaction rolls every change back. A handler that returns a value returns it as the call's JSON.
- **A build for the host is a stub.** Outside `wasm32` the calls answer with an error, so `cargo check` and the sample's
  `cargo fmt`/tests work without a WebAssembly toolchain.
- **The example is the documentation.** `plugins/area-summary` uses only this crate and `serde_json`; its `README.md`
  says how to build and install it, and `crates/plugin/tests/sample.rs` runs the built module.