---
name: bimcraft-pyaddins
description: Write, test and publish BIMCraft PyAddins - Python scripts that automate a BIMCraft model, shown in its Script Automation palette (Player) and as buttons on its PyAddins tab. Use when the user asks for a BIMCraft script, plugin, add-in, macro, automation, or a new ribbon button.
---

# BIMCraft PyAddins

A PyAddin is one Python file in the user's PyAddins folder. BIMCraft lists it in its Script Automation palette with a form
built from its `main`'s parameters, and can put it on its PyAddins tab as a button. It runs in the user's own Python (3.9 or
newer) and changes the model only through BIMCraft's commands.

## Setup

`bimcraft-cli skill install` puts this skill where Claude Code and Codex find it, and prints the `claude mcp add` and
`codex mcp add` lines that give the agent BIMCraft's tools. PyAddins > Python in the window has the same button.

## Tools

Use the `bimcraft` MCP server (`bimcraft-cli mcp`, or `bimcraft-cli mcp --connect` to work on the project open in the window):

- `pyaddin_save {name, code}` writes the script. It shows in the user's Player at once. The reply has the inputs it read, or
  `problem`: fix that before going on.
- `pyaddin_run {name, inputs?, selection?}` runs it. The reply has `ok`, `message` (what `main` returned), `log` (what it
  printed), `select`, or `error` with the script's line.
- `pyaddin_button {label, script, panel?, tip?, inputs?}` puts a button on the PyAddins tab. `remove: true` takes it off.
- `pyaddin_list`, `pyaddin_read`, `pyaddin_delete`.
- `list_commands {filter}` shows each command's parameters. `query_elements` and `render` let you check what a run did.

Without the MCP server, write the file to the folder yourself:
- Linux: `~/.config/BIMCraft/pyaddins/scripts/<name>.py`
- macOS: `~/Library/Application Support/BIMCraft/pyaddins/scripts/<name>.py`
- Windows: `%APPDATA%\BIMCraft\pyaddins\scripts\<name>.py`

## Workflow

1. Read what the commands take: `list_commands {filter: "wall"}`, and so on.
2. Write the script (see below), `pyaddin_save` it, and fix any `problem`.
3. Try it on a scratch model. Build one with `new_model` and `execute`/`script`, or `bimcraft-cli run --sample` gives a house.
   `pyaddin_run` it, then check the result with `query_elements` or `render`. Undo after every try (`execute {command: "undo"}`):
   a run is one undo step.
4. Make a button when the user wants one.

## A script

```python
"""Number the rooms of a level, left to right: the Player shows this line."""
__title__ = "Number Rooms"
from typing import Annotated, Literal
import bimcraft as bc


def main(level: bc.Level,
         prefix: str = "R",
         start: Annotated[int, bc.Range(1, 9999)] = 101,
         order: Literal["left to right", "top to bottom"] = "left to right"):
    """Number each room of a level in order.

    prefix: what goes before each number
    start: the first number
    """
    rooms = bc.query(category="room", level=level)
    key = (lambda r: r["point"][0]) if order == "left to right" else (lambda r: -r["point"][1])
    with bc.batch():  # together: one round trip, all or nothing
        for n, room in enumerate(sorted(rooms, key=key), start):
            bc.modify(id=room["id"], number=f"{prefix}{n}")
    bc.select([r["id"] for r in rooms])
    return f"{len(rooms)} rooms numbered"
```

- **Every command is a function**, keyword arguments being its params: `bc.wall(points=[[0, 0], [4000, 0]])`,
  `bc.door(at=[2000, 0])`, `bc.modify(id=..., ...)`, `bc.delete(ids=[...])`. `bc.execute("wall", {...})` does the same.
  Lengths are millimetres; x is east, y north, z up. A command returns its reply (`wall` gives `{"ids": [...]}`).
- **Reading:** `bc.query(category, level)` gives the elements with `id`, `guid`, fields and derived values (a wall's
  `length`, a room's `area_m2`). Also `bc.inspect()`, `bc.schedule(category=...)`, and `bc.selection()` (the ids selected when
  the run started).
- **Results:** what `main` returns is shown to the user (text, or anything JSON). `print` goes to the run's log.
  `bc.select(ids)` selects those elements when the run ends.
- **Errors:** a refused command raises `bc.CommandError`, whose message says what to change. Catch it to carry on. What ran
  before it stays done.
- **Speed:** each call is a round trip. Wrap loops of many commands in `with bc.batch():`. A command inside a batch returns
  None; the results are `batch.results` afterwards.
- **Not allowed:** `new`, `open` and other commands that replace the user's project are refused. Never touch `sys.stdout`
  directly; use `print`. `input()` reads nothing.

## Inputs: the form

The Player builds the form from `main`'s typed parameters. A parameter without a default must be filled in.

| Annotation | Field | The script gets |
|---|---|---|
| `bool` | check box | `True`/`False` |
| `int`, `float` | number | the number |
| `str` | text | the text |
| `Literal["a", "b"]` | list | one of them |
| `bc.Length` | length in the project's units | millimetres |
| `bc.Level` | the project's levels | the level's name |
| `bc.Selection = bc.SELECTION` | "N selected" | the ids selected |
| `Annotated[str, bc.Library("type:wall")]` | the library's names of that kind | the name |
| `Annotated[float, bc.Range(lo, hi)]` | number, checked | the number |

- **Defaults** must be written out as literals (`1200`, `"R"`, `True`), not worked out (`2 * 600` is refused).
- **Text:** `name: hint` lines in `main`'s docstring become each field's tooltip. The docstring's first paragraph says what the
  script does, and `__title__` names its row.
- **No `main`:** the script runs top to bottom and asks for nothing.
- **Other library kinds:** `material`, `family`, `type:door`, `type:floor`… (`library {}` lists them).

Keep to the standard library unless the user says their Python has a package. It must run on Python 3.9: no `match`, and
write `Optional[X]`, not `X | None`, in annotations.
