# PyAddins

The user's own tools, in Python, as pyRevit's buttons and Dynamo Player's scripts are in Revit. The code is `crates/pyaddins`
(no UI: the folder, finding Python, a script's inputs, a run), the desktop's `crates/ui-egui/src/pyaddins.rs` (the PyAddins
tab and the Script Automation palette) and the MCP server's `apps/bimcraft-cli/src/pyaddins.rs` (the `pyaddin_*` tools). The
skill agents follow to write one is [skills/bimcraft-pyaddins/SKILL.md](../../skills/bimcraft-pyaddins/SKILL.md).

## Where scripts live

A script is a `.py` file in the user's PyAddins folder, beside the favorites and the settings (`pyaddins/scripts/<name>.py`;
`BIMCRAFT_PYADDINS` puts the folder elsewhere). **A project never carries code**: a `.bimcraft` file has no scripts in it, so
opening someone's project runs nothing. The buttons made of scripts are `pyaddins/buttons.json` (label, script, ribbon panel,
tooltip, the input values it runs with); the Python set in PyAddins > Python is `pyaddins/settings.json`; `pyaddins/lib/bimcraft.py`
is the module scripts import, written by the app when it isn't this version's. A name is letters, digits, spaces, `_` and `-`;
a script is at most 1 MB, and the folder lists 500 at most. A new folder starts with the samples (`store::SAMPLES`, in
`crates/pyaddins/src/samples`: Change Wall Type, two lists of the project's wall types), written once, so a sample deleted stays
deleted.

The window looks at the folder every 0.8 s (on its own clock) while the PyAddins tab or the palette is shown, and reads it
again when a script or the buttons file changed: an agent's `pyaddin_save` (or any editor saving the file) is in the Player a
moment later, and a deleted script takes its buttons with it.

## Python

BIMCraft bundles no Python. A script runs in the one PyAddins > Python names, or else the first of `py -3` (Windows),
`python3` and `python` that answers and is **3.9 or newer** (macOS's own is 3.9). `bimcraft.py` is the standard library alone
and reads as 3.9 does; a script may use whatever its Python has installed (pandas, openpyxl…). `BIMCRAFT_TEST_PYTHON` runs the
crate's tests in another interpreter, the oldest supported, say.

## A run

`bimcraft.py run SCRIPT` in a process of its own (`run.rs`), so nothing it does takes the window down, and Stop kills it. It
speaks JSON a line at a time on its own standard input and output: the app sends `{"inputs", "selection"}` first; the script
sends `execute` and `script` requests (each answered `{"ok"}` or `{"error"}`), `print` (what it prints: `sys.stdout` and
`sys.stderr` are replaced, `input()` reads nothing) and at its end `done` (what `main` returned, what to select) or `failed`
(the traceback of the script's own lines; a refused command is `line N: CommandError: …`). A line that is no message, or one
past 16 MB, ends the run.

The requests go through `bimcraft_core::bridge::run`, as an agent's do: each a command, checked and rolled back on its own
failure, and nothing that replaces the project (`new`, `open`…). A command that fails raises `bc.CommandError` in Python; what
the script did before it stays done. The whole run is **one undo step**, `PyAddin` (`Session::mark` and `Session::squash`, since
the window answers a run's requests a few each frame). `with bc.batch():` sends the commands inside it together, all or nothing.

## Inputs: the Player's form

What a script asks for is its `main`'s parameters, read by Python's own parser without running anything (`bimcraft.py
describe`, `inputs.rs`): `bool` (a check box), `int`, `float`, `str`, `Literal["a", "b"]` (a list), `bc.Length` (typed in the
project's units, given in mm), `bc.Level` (the project's levels), `bc.Selection = bc.SELECTION` (element ids; by default what
is selected), `Annotated[str, bc.Library("type:wall")]` (the library's names of that kind) and `Annotated[float, bc.Range(lo,
hi)]`. A default must be written out, not worked out; a library list with none starts on the first name of its kind an
earlier list of the form hasn't taken. Each field is named by its label for screen readers. `name: hint` lines in `main`'s docstring are each field's tooltip; its
first paragraph, or the module's docstring, says what the script does; `__title__` names its row. A script without `main` runs
from top to bottom and asks for nothing. The values are checked before the run (`inputs::resolve`): a missing one, one of the
wrong kind, one out of its range, or one `main` doesn't take is an error that names it.

## The desktop

The **PyAddins** tab (after Manage): Player (the Script Automation palette), Reload (the folder, and Python, again), Python (the
path, what was found, the editor scripts open in, where the scripts are, and the skill for agents), New Script (the Dummy
Script: a dummy of every kind of field, to copy from, under the first free name); Stop, and Restart (the last script again,
with the same values, its selection as the selection is now); then a panel for each ribbon panel the buttons name (PyAddins when they name none), a button
each. A button runs its script with the values it keeps (the rest their defaults); a click on one that is running stops it;
right-click: Show in Player, Remove Button.

The **Script Automation** palette docks on the right the first time it opens (it starts closed, and a settings file from before
it keeps it closed): a search, and a row for each script, its ▶ running it with its form's values (■ stops it). A row opens to
its description, its form, Advanced Settings (Make Button: a ribbon button of the form's values, in the panel named; Edit;
Delete Script; where the file is), Run Script, and the last run's log and result. The status bar says how
a run ended.

**Edit** (`editor.rs`) opens the script to edit it, never to run it: the editor set in PyAddins > Python, else on Windows the
Edit verb, on macOS `open -t`, and on Linux the `.py` handler when it is an editor, else the plain-text one, through `gio
launch`. A `.py` file's own association often runs it (Python on Windows; Wine registers itself for `.py` on Linux, and Edit
then seemed to do nothing). A launcher that fails within three seconds says why in the status bar.

## Agents

The MCP server's tools write the folder, so they work with or without a window (`mcp --connect` runs a script on the window's
project): `pyaddin_list` (scripts and buttons; `describe` for each one's inputs), `pyaddin_read` (code and inputs),
`pyaddin_save` (the reply has the inputs read, or why the Player can't read it), `pyaddin_delete`, `pyaddin_button` (checks
its values against the script) and `pyaddin_run` (inputs, a selection, a timeout of 120 s by default; the reply is the message,
the log, what it selected or the error). On the server's own session the run is one undo step; on a window's, each command is
one, as an agent's are.

The skill (`skills/bimcraft-pyaddins/SKILL.md`) is built into the programs (`skill.rs`): `bimcraft-cli skill install [DIR]` and
PyAddins > Python's Install Skill write it where Claude Code (`~/.claude/skills`) and Codex (`~/.agents/skills`, as OpenAI's
skills documentation has it) read skills, and say the `claude mcp add` and `codex mcp add` lines that give each the MCP server;
`bimcraft-cli skill` prints it. In this repository `.claude/skills` and `.agents/skills` link to it, so an agent working here
has it.

## Decisions, and why

- **A tab of its own, PyAddins**, as pyRevit has its own: the user's tools stay apart from BIMCraft's, and a new button never
  moves a built-in one. A button with no panel goes in the PyAddins panel.
- **No Python bundled.** Python is the language BIM people know (pyRevit, Dynamo's Python node), and most already have one;
  bundling would add 15-25 MB to every installer, a second runtime to keep patched, and still not have the user's packages.
  The cost is a step for a user without Python, which PyAddins > Python and the run's error say how to take.
- **3.9 or newer.** What macOS ships and every supported Linux has; the client is the standard library alone, so a newer
  Python (3.15 today) needs nothing from BIMCraft. Older ones are end-of-life.
- **A process of its own, not Python inside the app.** In-process (PyO3) would be faster per call, but a crash in a C
  extension would take the window and the user's unsaved work down, which breaks the first rule (never crash); Stop could not
  kill it; and the app would be tied to one Python's ABI. A round trip is well under a millisecond, and `bc.batch()` sends many
  commands as one.
- **Only commands.** A script changes the model the way an agent does, through `Session::execute`: each change checked by
  `Model::check` and rolled back when it fails, nothing that replaces the project, every run one undo step. A script can't
  leave a model a later version can't open.
- **No code in project files.** Opening someone's project must never run code: scripts live in the user's folder, and a project
  carries none. (A sandboxed language could make shared scripts safe later; Python can't be sandboxed.)
- **The form is `main`'s signature**, the way Typer and Gradio do it: one source of truth, an ordinary Python function an IDE
  checks and an agent writes well, and a script that also runs on its own. It is read with Python's own parser, so showing a
  form runs nothing.
- **The folder is the truth, looked at every 0.8 s.** An agent, an editor or a synced folder can write a script, and the
  Player shows it; no channel from the MCP server to the window is needed, and it works with no window open.
- **Agents through MCP and a skill.** The tools write and run scripts the same way for Claude Code, Codex or any MCP client; the
  skill teaches the API and the workflow (save, run on a scratch model, check, undo, then a button). The app itself does not
  call an AI service: only the updater phones home.
- **Samples once.** A new folder gets Change Wall Type, written once, so a sample the user deletes stays deleted.

## Not yet

Hooks (on open, on save), warnings a script adds to the model's, panels a script describes, input types for a picked point or
a picked element in the window, and a `.pyi` stub of every command for an editor's completion.
