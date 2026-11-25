# The apps

`bimcraft` is the desktop app: `main.rs` reads the arguments, builds the session, attaches the updater and the assistant's network and opens the window (the
write-up of the start, its log and the graphics fallback is under Startup in [the desktop](../docs/areas/desktop.md)). `bimcraft-cli`
is the same engine from a terminal: `run` (scripts and commands), `convert`, `info`, and `mcp`, the MCP server for agents, whose
tools run the same commands. Neither adds behaviour of its own: a feature is a command in the core.

- Only the desktop reaches the network, and only through `crates/update` (the updater) and `crates/assistant` (the AI chat, on the user's key and
  message); the CLI and the MCP server never do (a test reads every manifest).
- The desktop listens only when asked (`--control PORT`, or File > Settings > Agents), only on 127.0.0.1, and lets in only a connection that
  says the token in the file only the user reads; an agent through it may not replace the project (`bimcraft_core::bridge`), and while the
  user lets agents only read (Settings > Agents) the engine runs its requests read-only. `mcp --connect` is the asking side (`window.rs`).
- `mcp --read-only [FILE]` is a headless server that opens FILE and then only reads it (`Session::read_only`); `mcp --connect --read-only`
  asks the window to run each request read-only. The refusals say what lets the agent edit, and the instructions say it may only read.
  Either way, or with a window that lets agents only read, the `pyaddin_*` tools only list and read: a script is Python that no
  session's read-only reaches, so saving, deleting, running one and changing a button are refused ([PyAddins](../docs/areas/pyaddins.md#agents)).
- Both count what they allocate (`cap`, the global allocator) and read what the machine has free (`sysinfo`), and install that as
  the core's meter first thing in `main`, so a file opened is held to the memory there is (`bimcraft_core::memory`).
- A release build on Windows has no console: a start that fails says so through `startup::note`, not `eprintln!` alone.
- `wgpu_log.rs` installs the one `log` logger: wgpu's, egui-wgpu's and eframe's warnings go to `bimcraft.log`, the first 20 of a start
  and then a count a minute (software rendering, under Startup in [the desktop](../docs/areas/desktop.md)).
- `cargo test -p bimcraft -p bimcraft-cli` runs in a few seconds.
