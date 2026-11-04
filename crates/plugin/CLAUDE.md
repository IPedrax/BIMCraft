# bimcraft-plugin: the sandbox

The plugin folder (`lib.rs`), the manifest (`manifest.rs`) and the WASM runtime (`runtime.rs`). The core knows nothing of
WASM: this crate fills `Session`'s external command registry and calls `Session::execute` for everything a plugin does.

- **Everything a plugin gets is a command.** A new host function is a change to the ABI: bump `manifest::ABI`, add it to
  `runtime::HOST` and `install`, its limit and its log line, cover it with a test in `tests/plugins.rs` (a WAT module in
  `tests/fixtures`), and write it in `docs/areas/plugins.md`. The guest side goes in `crates/plugin-sdk`.
- **No global state.** Plugins belong to a `Plugins` value and its commands to one `Session`; tests make a folder in a
  temporary directory, so two tests and two sessions never share a plugin.
- **The host trusts nothing.** A manifest, a module and every byte a guest returns are hostile: sizes capped, imports
  limited to `bimcraft`, replies capped, fuel, memory, wall-clock and command budgets, permissions checked here before
  `execute`, and every refusal logged. A trap, a limit or bad JSON is a `Result::Err`, never a panic.
- **A call is a transaction.** `Plugins::register` wraps the runtime in `Session::transaction`, so a successful change is
  one undo step and a failed one leaves model, history, phase and path as they were. A read creates no step.
- **The module is compiled once and instantiated per call.** A guest gets no state between calls; the compiled `Engine`
  and `Module` are shared through an `Arc`. Keep it that way unless persistent state is designed and documented.
- **Tests first.** `tests/plugins.rs` holds the sandbox (forwarded command, hang, trap, memory, bad reply, command storm,
  permissions, folder, disabled state) and `tests/sample.rs` runs the example plugin in `plugins/area-summary`.