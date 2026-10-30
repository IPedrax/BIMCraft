//! The MCP server's PyAddins tools: an agent writes the user's Python scripts and the buttons made of them into the
//! PyAddins folder (`bimcraft_pyaddins::Store`), where the window's Player and its PyAddins tab show them as they come, and
//! runs one, on the server's session or, with `--connect`, on the window's.

use std::time::Duration;

use bimcraft_pyaddins::{Button, Description, Store, inputs, run};
use serde_json::{Map, Value, json};

/// How long a script may run for an agent, by default and at most.
const RUN: u64 = 120;
const MAX_RUN: u64 = 3600;

pub fn tools() -> Vec<Value> {
    vec![
        json!({"name": "pyaddin_list", "description": "The user's PyAddins: the Python scripts in the Player (PyAddins > Player), and the buttons on the PyAddins tab that run them. With `describe`, each script's inputs too (it asks Python, a moment a script).",
         "inputSchema": {"type": "object", "properties": {"describe": {"type": "boolean"}}}}),
        json!({"name": "pyaddin_read", "description": "A PyAddin script's code, and what its `main` asks for (the Player's form).",
         "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}}),
        json!({"name": "pyaddin_save", "description": "Write a PyAddin script (new, or over the one of that name): it is in the user's Player at once. \
            `import bimcraft as bc`; every command is a function (bc.wall(points=[[0,0],[4000,0]]), or bc.execute(\"wall\", {...})); bc.query(category, level) lists elements \
            with their ids and values; bc.selection() is what was selected; bc.select(ids) selects at the end; `with bc.batch():` sends commands together, all or nothing; \
            a refused command raises bc.CommandError. The Player's form is `main`'s typed parameters: bool, int, float, str, Literal[\"a\", \"b\"], bc.Length (mm), bc.Level, \
            bc.Selection = bc.SELECTION, Annotated[str, bc.Library(\"type:wall\")], Annotated[float, bc.Range(lo, hi)], defaults written out; `name: hint` lines in its docstring; \
            `__title__` names it. What main returns is shown. The reply says the inputs read, or what is wrong with the script. Names: letters, digits, spaces, _ and -.",
         "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "code": {"type": "string"}}, "required": ["name", "code"]}}),
        json!({"name": "pyaddin_delete", "description": "Delete a PyAddin script, and the buttons that run it.",
         "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}}),
        json!({"name": "pyaddin_button", "description": "Put a button on the PyAddins tab that runs a script with the input values it keeps (`inputs`, by name; the rest take their defaults), \
            in a ribbon panel (`panel`, Scripts by default), with a tooltip (`tip`). The same `label` replaces that button; `remove: true` takes it off.",
         "inputSchema": {"type": "object", "properties": {"label": {"type": "string"}, "script": {"type": "string"}, "panel": {"type": "string"}, "tip": {"type": "string"}, "inputs": {"type": "object"}, "remove": {"type": "boolean"}}, "required": ["label"]}}),
        json!({"name": "pyaddin_run", "description": "Run a PyAddin script on the model, as the Player does, with `inputs` by name (the rest take their defaults) and `selection` (element ids, for its \
            bc.Selection inputs and bc.selection()). The reply is what main returned (`message`), what it printed (`log`), what it selected, or the error with the script's line. \
            Each command it runs is checked as any is; the whole run is one undo step. It is stopped after `timeout` seconds (120 by default).",
         "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "inputs": {"type": "object"}, "selection": {"type": "array", "items": {"type": "integer"}}, "timeout": {"type": "integer"}}, "required": ["name"]}}),
    ]
}

/// What the agent gave as `key`: text, or the error that says so.
fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key].as_str().ok_or_else(|| format!("`{key}` is text"))
}

fn object(args: &Value, key: &str) -> Result<Map<String, Value>, String> {
    match &args[key] {
        Value::Null => Ok(Map::new()),
        Value::Object(m) => Ok(m.clone()),
        _ => Err(format!("`{key}` is an object, by name")),
    }
}

/// What the script asks for, when Python can read it: none and the reason when it can't.
fn describe(store: &Store, name: &str) -> Result<Description, String> {
    let python = store.python()?;
    inputs::describe(&python, &store.client()?, &store.path(name)?)
}

/// The model a script works on: the server's own session, or the window's.
pub trait Target {
    /// Runs `f`, which runs a script, its commands answered on the model and, where the model's history is here, made one
    /// undo step.
    fn as_one(&mut self, f: &mut dyn FnMut(&mut run::Ask) -> run::Outcome) -> run::Outcome;
}

/// A PyAddins tool, when `name` is one.
pub fn call(name: &str, args: &Value, store: Option<&Store>, target: &mut dyn Target) -> Option<Result<Value, String>> {
    if !name.starts_with("pyaddin_") {
        return None;
    }
    let Some(store) = store else {
        return Some(Err("there is no PyAddins folder: no home folder to keep it in (set BIMCRAFT_PYADDINS)".into()));
    };
    Some(match name {
        "pyaddin_list" => {
            let full = args["describe"].as_bool() == Some(true);
            let scripts: Vec<Value> = store
                .scripts()
                .into_iter()
                .map(|s| {
                    let mut v = json!({"name": s.name, "path": s.path});
                    if full {
                        match describe(store, &s.name) {
                            Ok(d) => v["description"] = d.to_json(),
                            Err(e) => v["problem"] = json!(e),
                        }
                    }
                    v
                })
                .collect();
            let buttons: Vec<Value> = store.buttons().iter().map(Button::to_json).collect();
            Ok(json!({"folder": store.dir, "scripts": scripts, "buttons": buttons}))
        }
        "pyaddin_read" => text(args, "name").and_then(|n| {
            let code = store.read(n)?;
            let mut v = json!({"name": n, "code": code});
            match describe(store, n) {
                Ok(d) => v["description"] = d.to_json(),
                Err(e) => v["problem"] = json!(e),
            }
            Ok(v)
        }),
        "pyaddin_save" => text(args, "name").and_then(|n| {
            let path = store.save(n, text(args, "code")?)?;
            let mut v = json!({"saved": n, "path": path});
            match describe(store, n) {
                Ok(d) => v["description"] = d.to_json(),
                Err(e) => v["problem"] = json!(format!("saved, but the Player can't read it: {e}")),
            }
            Ok(v)
        }),
        "pyaddin_delete" => text(args, "name").and_then(|n| store.delete(n).map(|()| json!({"deleted": n}))),
        "pyaddin_button" => (|| {
            let label = text(args, "label")?;
            if args["remove"].as_bool() == Some(true) {
                store.remove_button(label)?;
                return Ok(json!({"removed": label}));
            }
            let mut b = args.clone();
            if let Some(o) = b.as_object_mut() {
                o.remove("remove");
            }
            let button = Button::from_json(&b)?;
            // Its values must be ones the script takes, when Python can say.
            if let Ok(d) = describe(store, &button.script) {
                let missing = d.inputs.iter().filter(|i| i.default.is_none() && !button.inputs.contains_key(&i.name));
                let unknown_or_wrong = inputs::resolve(&d.inputs, &button.inputs, &[]);
                if let Err(e) = unknown_or_wrong
                    && missing.clone().count() == 0
                {
                    return Err(e);
                }
                if let Some(i) = missing.into_iter().next() {
                    return Err(format!("`{}` needs a value: give it in `inputs`, or a default in the script", i.name));
                }
            }
            let label = button.label.clone();
            store.set_button(button)?;
            Ok(json!({"button": label}))
        })(),
        "pyaddin_run" => (|| {
            let name = text(args, "name")?;
            let python = store.python()?;
            let client = store.client()?;
            let path = store.path(name)?;
            if !path.is_file() {
                return Err(format!("there is no script `{name}`"));
            }
            let d = inputs::describe(&python, &client, &path)?;
            let selection: Vec<u64> = match &args["selection"] {
                Value::Null => Vec::new(),
                Value::Array(a) if a.len() <= inputs::MAX_IDS => {
                    a.iter().map(|v| v.as_u64().ok_or("`selection` is element ids")).collect::<Result<_, _>>()?
                }
                _ => return Err("`selection` is a list of element ids".into()),
            };
            let values = inputs::resolve(&d.inputs, &object(args, "inputs")?, &selection)?;
            let limit = match &args["timeout"] {
                Value::Null => RUN,
                v => v.as_u64().filter(|t| (1..=MAX_RUN).contains(t)).ok_or(format!("`timeout` is 1 to {MAX_RUN} seconds"))?,
            };
            let out =
                target.as_one(&mut |ask| run::run_to_end(&python, &client, &path, &values, &selection, ask, Duration::from_secs(limit)));
            Ok(out.to_json())
        })(),
        other => Err(format!("unknown tool `{other}`")),
    })
}
