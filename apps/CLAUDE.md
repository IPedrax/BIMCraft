# The apps

`bimcraft` is the desktop app: `main.rs` reads the arguments, builds the session, attaches the updater and opens the window (the
write-up of the start, its log and the graphics fallback is under Startup in [the desktop](../docs/areas/desktop.md)). `bimcraft-cli`
is the same engine from a terminal: `run` (scripts and commands), `convert`, `info`, and `mcp`, the MCP server for agents, whose
tools run the same commands. Neither adds behaviour of its own: a feature is a command in the core.

- Only the desktop phones home, and only through `crates/update`; the CLI and the MCP server never do (a test reads their manifests).
- The desktop listens only when asked (`--control PORT`, or File > Settings > Agents), only on 127.0.0.1, and lets in only a connection that
  says the token in the file only the user reads; an agent through it may not replace the project (`bimcraft_core::bridge`), and while the
  user lets agents only read (Settings > Agents) the engine runs its requests read-only. `mcp --connect` is the asking side (`window.rs`).
- `mcp --read-only [FILE]` is a headless server that opens FILE and then only reads it (`Session::read_only`); `mcp --connect --read-only`
  asks the window to run each request read-only. The refusals say what lets the agent edit, and the instructions say it may only read.
- A release build on Windows has no console: a start that fails says so through `startup::note`, not `eprintln!` alone.
- `cargo test -p bimcraft -p bimcraft-cli` runs in a few seconds.
