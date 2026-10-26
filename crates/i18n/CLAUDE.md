# bimcraft-i18n

Shared translation lookup and the build-time PO compiler. The default library has no dependencies: production front ends
use generated static catalogs. The `catalog` feature enables the compiler for core/UI build scripts; `tooling` adds Rust
source extraction and the `cargo i18n` commands. Syntax tooling is not a production runtime dependency.

The file layout, fallback rules, supported PO syntax and contribution workflow are in
[translations](../../docs/areas/translations.md). Keep interface and project catalogs separate, preserve saved locale codes,
and leave CLI/MCP diagnostics in English. The root rules about validation, panics and tests apply here too.

Run `cargo test -p bimcraft-i18n`, `cargo i18n check` and the root checks after a change. The parser, generator, merge and
extraction tests are enabled under `cfg(test)` even without optional features. Test malformed input through errors, and
ensure updating a catalog preserves translator work and remains repeatable.
