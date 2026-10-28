# The apps

`bimcraft` is the desktop app: `main.rs` reads the arguments, builds the session, attaches the updater and opens the window (the
write-up of the start, its log and the graphics fallback is under Startup in [the desktop](../docs/areas/desktop.md)). `bimcraft-cli`
is the same engine from a terminal: `run` (scripts and commands), `convert`, `info`, and `mcp`, the MCP server for agents, whose
tools run the same commands. Neither adds behaviour of its own: a feature is a command in the core.

- Only the desktop phones home, and only through `crates/update`; the CLI and the MCP server never do (a test reads their manifests).
- The desktop listens only when asked (`--control PORT`), only on 127.0.0.1, and lets in only a connection that says the token in the file only
  the user reads; an agent through it may not replace the project (`bimcraft_core::bridge`). `mcp --connect` is the asking side (`window.rs`).
- A release build on Windows has no console: a start that fails says so through `startup::note`, not `eprintln!` alone.
- `bimcraft-web` is the desktop's window in a browser page (`index.html`, built with `trunk build --release`): no updater, no Revit
  reader, no control port, and files in the page's memory. What it can't do and how to look at it: [the browser build](../docs/areas/web.md).
- `cargo test -p bimcraft -p bimcraft-cli -p bimcraft-web` runs in a few seconds.
