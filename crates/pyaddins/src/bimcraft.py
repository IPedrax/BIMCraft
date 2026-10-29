"""BIMCraft from Python: what a PyAddin script imports.

    import bimcraft as bc

    def main(level: bc.Level, spacing: bc.Length = 1200, tag: bool = True):
        \"\"\"Place a column every `spacing` along each selected wall.

        spacing: centre to centre
        \"\"\"
        for wall in bc.query(category="wall", level=level):
            ...
        return "done"

Every BIMCraft command is a function: `bc.wall(points=[[0, 0], [4000, 0]])` runs `wall`, and `bc.execute("wall", {...})`
does the same. Lengths are millimetres. A command that fails raises `bc.CommandError`, and what the script did before it
stays done; `with bc.batch():` sends the commands inside it together, all or nothing.

The app runs this file (`run` or `describe`); it writes it beside the scripts, and a script imports it from there. It is
the standard library alone, and reads as Python 3.9 does.
"""

import ast
import io
import json
import os
import sys
import traceback

__all__ = [
    "CommandError", "execute", "script", "batch", "query", "inspect", "schedule", "selection", "select",
    "Length", "Level", "Selection", "SELECTION", "Range", "Library",
]

# ---------- the inputs a script's `main` asks for ----------


class Length(float):
    """A length: typed in the project's units, and given to the script in millimetres."""


class Level(str):
    """A level of the project, by name: the Player lists them."""


class Selection(list):
    """Elements, by id: what is selected when the script runs, unless it is given others."""


class _Selected:
    def __repr__(self):
        return "bimcraft.SELECTION"


SELECTION = _Selected()
"""The default of a `Selection`: what is selected when the script runs."""


class Range:
    """`Annotated[float, bc.Range(0, 5000)]`: the least and the most a number may be."""

    def __init__(self, low=None, high=None):
        self.low, self.high = low, high


class Library:
    """`Annotated[str, bc.Library("type:wall")]`: a name from the project's library of that kind, which the Player lists."""

    def __init__(self, kind):
        self.kind = kind


# ---------- talking to the app ----------


class CommandError(Exception):
    """A command the app refused: the message says why, and what to change."""


_proto = None  # the stream the app reads requests from (the process's own stdout)
_replies = None  # the stream its replies come on (the process's own stdin)
_selection = []
_select = None


def _send(message):
    if _proto is None:
        raise RuntimeError("bimcraft talks to the app only in a script the app runs (PyAddins > Player)")
    _proto.write((json.dumps(message, ensure_ascii=True) + "\n").encode("ascii"))
    _proto.flush()


def _ask(message):
    _send(message)
    line = _replies.readline()
    if not line:
        raise SystemExit("the app stopped the script")
    reply = json.loads(line.decode("utf-8"))
    if "error" in reply:
        raise CommandError(reply["error"])
    return reply.get("ok")


_batch = None


def execute(command, params=None, **more):
    """Run a BIMCraft command: `execute("wall", {"points": [[0, 0], [4000, 0]]})`, or with keywords."""
    p = dict(params or {})
    p.update(more)
    if _batch is not None:
        _batch.append(command + " " + json.dumps(p))
        return None
    return _ask({"op": "execute", "command": command, "params": p})


def script(text, atomic=False):
    """Run a BIMCraft script, one `command {json}` a line; `atomic`: all or nothing. Its results, a line each."""
    return _ask({"op": "script", "text": text, "atomic": bool(atomic)})


class batch:
    """`with bc.batch():` sends the commands run inside it at its end, together, all or nothing: one undo step, and
    much faster than one at a time. A command inside returns None; the results are `batch.results` after."""

    def __enter__(self):
        global _batch
        if _batch is not None:
            raise RuntimeError("a batch is open already")
        _batch = []
        self.results = None
        return self

    def __exit__(self, kind, value, tb):
        global _batch
        lines, _batch = _batch, None
        if kind is None and lines:
            self.results = script("\n".join(lines), atomic=True)
        return False


def query(category=None, level=None, limit=None, offset=None):
    """The elements, with their `id`, `guid`, fields and derived values (a wall's length, a room's area), as
    the `elements` command gives them: by category and level. Without `limit`, all of them, a page at a time."""
    p = {k: v for k, v in (("category", category), ("level", level)) if v is not None}
    if limit is not None:
        p.update(limit=limit, offset=offset or 0)
        return execute("elements", p).get("elements", [])
    out, at = [], offset or 0
    while True:
        page = execute("elements", dict(p, limit=500, offset=at)).get("elements", [])
        out.extend(page)
        if len(page) < 500:
            return out
        at += len(page)


def inspect():
    """The model at a glance: levels, counts, types and warnings."""
    return execute("inspect")


def schedule(**params):
    """A schedule's columns, rows and totals (see the `schedule` command)."""
    return execute("schedule", params)


def selection():
    """The ids of what was selected when the script started."""
    return list(_selection)


def select(ids):
    """What is selected when the script ends: these elements, by id."""
    global _select
    _select = [int(i) for i in ids]


def __getattr__(name):
    # bc.wall(points=...) runs `wall`: every command is a function.
    if name.startswith("_"):
        raise AttributeError(name)
    return lambda params=None, **more: execute(name, params, **more)


# ---------- describe: the inputs of a script, read without running it ----------

_PLAIN = {"bool": "bool", "int": "int", "float": "float", "str": "text"}
_OURS = {"Length": "length", "Level": "level", "Selection": "selection"}


def _name_of(node):
    """`bc.Length`, `bimcraft.Length` or `Length` all name `Length`."""
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return node.attr
    return None


def _literal(node, what):
    try:
        return ast.literal_eval(node)
    except Exception:
        raise ValueError("%s must be written out (a number, text, True or False), not worked out" % what)


def _kind(annotation, name):
    """What an annotation asks for: {"type": ..., "choices"?, "min"?, "max"?, "library"?}."""
    if annotation is None:
        return {"type": "text"}
    if isinstance(annotation, ast.Constant) and isinstance(annotation.value, str):
        # `from __future__ import annotations` style, or a quoted annotation.
        try:
            annotation = ast.parse(annotation.value, mode="eval").body
        except SyntaxError:
            raise ValueError("`%s`: its type %r is not one the Player knows" % (name, annotation.value))
    if isinstance(annotation, ast.Subscript):
        head = _name_of(annotation.value)
        inner = annotation.slice
        if isinstance(inner, ast.Index):  # Python 3.8
            inner = inner.value
        items = list(inner.elts) if isinstance(inner, ast.Tuple) else [inner]
        if head == "Literal":
            choices = [_literal(i, "`%s`'s choices" % name) for i in items]
            return {"type": "choice", "choices": choices}
        if head == "Annotated" and items:
            kind = _kind(items[0], name)
            for meta in items[1:]:
                if not isinstance(meta, ast.Call):
                    continue
                what = _name_of(meta.func)
                args = [_literal(a, "`%s`'s %s" % (name, what)) for a in meta.args]
                args += [_literal(k.value, "`%s`'s %s" % (name, what)) for k in meta.keywords]
                if what == "Range":
                    low = args[0] if len(args) > 0 else None
                    high = args[1] if len(args) > 1 else None
                    if low is not None:
                        kind["min"] = low
                    if high is not None:
                        kind["max"] = high
                elif what == "Library" and args:
                    kind = {"type": "library", "library": str(args[0])}
            return kind
        if head in ("list", "List") and _name_of(inner) in ("int", "Element"):
            return {"type": "selection"}
        raise ValueError("`%s`: the Player knows bool, int, float, str, Literal[...], Annotated[...], bc.Length, bc.Level and bc.Selection" % name)
    kind = _name_of(annotation)
    if kind in _PLAIN:
        return {"type": _PLAIN[kind]}
    if kind in _OURS:
        return {"type": _OURS[kind]}
    raise ValueError("`%s`: the Player knows bool, int, float, str, Literal[...], Annotated[...], bc.Length, bc.Level and bc.Selection" % name)


def _hints(doc):
    """`name: what it is` lines of a docstring."""
    hints = {}
    for line in (doc or "").splitlines():
        key, sep, text = line.strip().partition(":")
        if sep and key.isidentifier() and text.strip():
            hints[key] = text.strip()
    return hints


def describe(source, filename="<script>"):
    """What a script is and what its `main` asks for, from its text alone: nothing of it runs."""
    try:
        tree = ast.parse(source, filename)
    except SyntaxError as e:
        return {"error": "line %s: %s" % (e.lineno, e.msg)}
    out = {"doc": (ast.get_docstring(tree) or "").strip(), "main": False, "inputs": []}
    for node in tree.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and _name_of(node.targets[0]) == "__title__":
            try:
                out["title"] = str(ast.literal_eval(node.value))
            except Exception:
                pass
    main = [n for n in tree.body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)) and n.name == "main"]
    if not main:
        return out
    fn = main[-1]
    out["main"] = True
    doc = ast.get_docstring(fn) or ""
    if doc:
        out["doc"] = doc.strip().split("\n\n")[0].strip()
    hints = _hints(doc)
    a = fn.args
    positional = list(getattr(a, "posonlyargs", [])) + list(a.args)
    defaults = [None] * (len(positional) - len(a.defaults)) + list(a.defaults)
    pairs = list(zip(positional, defaults)) + list(zip(a.kwonlyargs, a.kw_defaults))
    try:
        for arg, default in pairs:
            item = {"name": arg.arg}
            item.update(_kind(arg.annotation, arg.arg))
            if default is not None:
                if _name_of(default) == "SELECTION":
                    item["default"] = "selection"
                else:
                    item["default"] = _literal(default, "`%s`'s default" % arg.arg)
            if arg.arg in hints:
                item["hint"] = hints[arg.arg]
            out["inputs"].append(item)
    except ValueError as e:
        return {"error": str(e)}
    return out


# ---------- run: a script, as the app asks ----------


class _Printer(io.TextIOBase):
    """What the script prints, sent to the app's log a line at a time."""

    def __init__(self, err):
        self.err, self.part = err, ""

    def writable(self):
        return True

    def write(self, text):
        self.part += str(text)
        while "\n" in self.part:
            line, self.part = self.part.split("\n", 1)
            _send({"op": "print", "text": line, "err": self.err})
        return len(text)

    def flush(self):
        if self.part:
            _send({"op": "print", "text": self.part, "err": self.err})
            self.part = ""


def _message(value):
    if value is None:
        return None
    if isinstance(value, str):
        return value
    try:
        return json.dumps(value)
    except Exception:
        return repr(value)


def _run(path):
    global _proto, _replies, _selection
    _proto, _replies = sys.stdout.buffer, sys.stdin.buffer
    sys.stdout, sys.stderr = _Printer(False), _Printer(True)
    sys.stdin = io.StringIO("")
    start = json.loads(_replies.readline().decode("utf-8") or "{}")
    _selection = list(start.get("selection") or [])
    inputs = start.get("inputs") or {}
    try:
        with open(path, "r", encoding="utf-8") as f:
            source = f.read()
        sys.path.insert(0, os.path.dirname(os.path.abspath(path)))
        code = compile(source, path, "exec")
        scope = {"__name__": "__bimcraft__", "__file__": path, "__builtins__": __builtins__}
        exec(code, scope)
        result = None
        main = scope.get("main")
        if callable(main):
            result = main(**inputs)
        sys.stdout.flush()
        sys.stderr.flush()
        _send({"op": "done", "message": _message(result), "select": _select})
    except SystemExit as e:
        sys.stdout.flush()
        sys.stderr.flush()
        if e.code in (None, 0):
            _send({"op": "done", "message": None, "select": _select})
        else:
            _send({"op": "failed", "error": str(e.code)})
    except BaseException as e:
        sys.stdout.flush()
        sys.stderr.flush()
        if isinstance(e, CommandError):
            text = "CommandError: %s" % e
            frames = [f for f in traceback.extract_tb(e.__traceback__) if f.filename == path]
            if frames:
                text = "line %d: %s" % (frames[-1].lineno, text)
        else:
            # The script's own frames: this file's are the app's business.
            frames = [f for f in traceback.extract_tb(e.__traceback__) if os.path.abspath(f.filename) != os.path.abspath(__file__)]
            text = "".join(traceback.format_list(frames)) + "".join(traceback.format_exception_only(type(e), e))
        _send({"op": "failed", "error": text.rstrip()})


def _main(argv):
    if len(argv) == 3 and argv[1] == "describe":
        try:
            with open(argv[2], "r", encoding="utf-8") as f:
                source = f.read()
            out = describe(source, argv[2])
        except Exception as e:
            out = {"error": str(e)}
        sys.stdout.write(json.dumps(out, ensure_ascii=True) + "\n")
        return 0
    if len(argv) == 3 and argv[1] == "run":
        # A script's `import bimcraft` is this module, not a second copy of it.
        sys.modules["bimcraft"] = sys.modules[__name__]
        _run(argv[2])
        return 0
    if len(argv) == 2 and argv[1] == "version":
        sys.stdout.write("%d.%d.%d\n" % sys.version_info[:3])
        return 0
    sys.stderr.write("usage: bimcraft.py describe|run SCRIPT\n")
    return 2


if __name__ == "__main__":
    sys.exit(_main(sys.argv))
