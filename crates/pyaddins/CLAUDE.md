# bimcraft-pyaddins

The user's Python scripts: `store.rs` (the folder: scripts, buttons, the Python set, the client written beside them),
`python.rs` (finding the user's Python, 3.9 or newer, and running a process with a time limit and capped output),
`editor.rs` (opening a script to edit it, never to run it), `skill.rs` (the agent skill, built in, and installing it),
`inputs.rs` (what a script's `main` asks for, read by Python's `ast`, and the values given checked), `run.rs` (a run: the
process, its JSON-lines protocol, its requests answered through `bimcraft_core::bridge::run`) and `bimcraft.py` (the module
scripts import, and the `describe` and `run` the app starts; standard library alone, Python 3.9). The write-up is
[PyAddins](../../docs/areas/pyaddins.md).

- No UI, and the core never depends on it: the desktop and the MCP server do.
- What comes from the folder and from the process is hostile: names are checked, files and lines capped, a message that is not
  one ends the run.
- The tests that run Python skip, saying so, where none is found; `BIMCRAFT_TEST_PYTHON=$(uv python find 3.9) cargo test -p
  bimcraft-pyaddins` runs them in the oldest supported.
- A change to `bimcraft.py`'s protocol changes `run.rs` with it; the module is rewritten in the folder when it differs.
