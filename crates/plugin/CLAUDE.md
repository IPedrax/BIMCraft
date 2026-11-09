# bimcraft-plugin: the sandbox

The plugin folder (`lib.rs`), the manifest (`manifest.rs`) and the WASM runtime (`runtime.rs`). The core knows nothing of
WASM: this crate fills `Session`'s external command registry and calls `Session::execute` for everything a plugin does.

- **Everything a plugin gets is a command.** A new host function is a change to the ABI: bump `manifest::ABI`, add it to
  `runtime::HOST` and `install`, its limit and its log line, cover it with a test in `tests/plugins.rs` (a WAT module in
  `tests/fixtures`), and write it in `docs/areas/plugins.md`. The guest side goes in `crates/plugin-sdk`.
- **No global state.** Plugins belong to a `Plugins` value and its commands to one `Session`; tests make a folder in a
  temporary directory, so two tests and two sessions never share a plugin.
- **The disk is the core's word, not a list here.** A command that reads or writes files is marked on `Command.files`
  (`crates/core/src/commands.rs`), and `engine::touches_files` decides by its parameters for a `Conditional` one. The
  mark is what the `files` permission guards; a core test fails when a command takes a `path`, `save` or a texture's
  picture and isn't marked. The permission itself is the user's: a manifest only asks (`Manifest::asks_for`), and
  `Plugins::set_permission` (Manage > Plugins) grants it, kept in the folder's `state.json`.
- **The host trusts nothing.** A manifest, a module and every byte a guest returns are hostile: sizes capped, imports
  limited to `bimcraft`, replies capped, fuel, memory, wall-clock and command budgets, permissions checked here before
  `execute`, and every refusal logged. A trap, a limit or bad JSON is a `Result::Err`, never a panic.
- **A call is a transaction.** `Plugins::register` wraps the runtime in `Session::transaction`, so a successful change is
  one undo step and a failed one leaves model, history, phase and path as they were. A read creates no step.
- **The module is compiled once and instantiated per call.** A guest gets no state between calls; the compiled `Engine`
  and `Module` are shared through an `Arc`. Keep it that way unless persistent state is designed and documented.
- **Tests first.** `tests/plugins.rs` holds the sandbox (forwarded command, hang, trap, memory, bad reply, command storm,
  permissions, folder, disabled state) and `tests/sample.rs` runs a plugin built with the guest SDK — the fixture in
  `tests/sample-plugin`, which isn't in the repository as a module: `cargo build --manifest-path
  crates/plugin/tests/sample-plugin/Cargo.toml --target wasm32-unknown-unknown --release` before the tests. Without
  the build they skip with the command to run; in CI (`CI=1`) a skip is a failure.