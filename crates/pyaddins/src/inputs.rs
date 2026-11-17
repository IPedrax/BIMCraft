//! What a script asks for: its `main`'s parameters, read by Python's own parser without running anything (`bimcraft.py
//! describe`), and the values given for them checked before it starts. The Player builds its form from them, a button
//! keeps values for them, and an agent passes them to `pyaddin_run`.
//!
//! A type is what the annotation says: `bool`, `int`, `float`, `str`, `Literal[...]` (one of those), `bc.Length` (typed in
//! the project's units, given in millimetres), `bc.Level`, `bc.Selection` (element ids, by default what is selected),
//! `Annotated[str, bc.Library("type:wall")]` (a name from the library) and `Annotated[float, bc.Range(lo, hi)]`.

use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::python::{Python, output};

/// How long Python has to read a script.
const DESCRIBE: Duration = Duration::from_secs(20);
/// The most inputs a script may ask for, and the longest text one may be given.
pub const MAX_INPUTS: usize = 64;
pub const MAX_TEXT: usize = 10_000;
/// The most elements a selection input carries.
pub const MAX_IDS: usize = 100_000;

/// What an input is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Bool,
    Int,
    Float,
    Text,
    Length,
    Level,
    Selection,
    /// One of these values.
    Choice(Vec<Value>),
    /// A library entry's name, of this kind (`type:wall`, `material`…).
    Library(String),
}

/// An input of a script's `main`.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub name: String,
    pub kind: Kind,
    /// What it is when it's given nothing; `"selection"` for a selection input means what's selected.
    pub default: Option<Value>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// What the docstring says of it.
    pub hint: String,
}

/// A script, as the Player shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Description {
    /// `__title__`, if it has one.
    pub title: Option<String>,
    /// The first paragraph of `main`'s docstring, or else the module's.
    pub doc: String,
    /// Whether it has a `main` (one that hasn't runs from top to bottom, and asks for nothing).
    pub main: bool,
    pub inputs: Vec<Input>,
}

impl Input {
    pub fn to_json(&self) -> Value {
        let mut v = json!({"name": self.name});
        let (kind, extra) = match &self.kind {
            Kind::Bool => ("bool", None),
            Kind::Int => ("int", None),
            Kind::Float => ("float", None),
            Kind::Text => ("text", None),
            Kind::Length => ("length", None),
            Kind::Level => ("level", None),
            Kind::Selection => ("selection", None),
            Kind::Choice(c) => ("choice", Some(("choices", json!(c)))),
            Kind::Library(l) => ("library", Some(("library", json!(l)))),
        };
        v["type"] = json!(kind);
        if let Some((k, x)) = extra {
            v[k] = x;
        }
        for (k, x) in [("default", self.default.clone()), ("min", self.min.map(Value::from)), ("max", self.max.map(Value::from))] {
            if let Some(x) = x {
                v[k] = x;
            }
        }
        if !self.hint.is_empty() {
            v["hint"] = json!(self.hint);
        }
        v
    }
}

impl Description {
    pub fn to_json(&self) -> Value {
        json!({"title": self.title, "doc": self.doc, "main": self.main, "inputs": self.inputs.iter().map(Input::to_json).collect::<Vec<_>>()})
    }

    /// Reads what `bimcraft.py describe` printed. It is checked as any outside input is: a description that is not one is an
    /// error.
    pub fn from_json(v: &Value) -> Result<Description, String> {
        if let Some(e) = v["error"].as_str() {
            return Err(e.to_string());
        }
        let text = |v: &Value, max: usize| v.as_str().map(|s| s.chars().take(max).collect::<String>());
        let mut inputs = Vec::new();
        for i in v["inputs"].as_array().into_iter().flatten().take(MAX_INPUTS) {
            let name = text(&i["name"], 100).filter(|n| !n.is_empty()).ok_or("an input has no name")?;
            let kind = match i["type"].as_str() {
                Some("bool") => Kind::Bool,
                Some("int") => Kind::Int,
                Some("float") => Kind::Float,
                Some("text") => Kind::Text,
                Some("length") => Kind::Length,
                Some("level") => Kind::Level,
                Some("selection") => Kind::Selection,
                Some("choice") => Kind::Choice(i["choices"].as_array().cloned().unwrap_or_default().into_iter().take(200).collect()),
                Some("library") => Kind::Library(text(&i["library"], 100).unwrap_or_default()),
                other => return Err(format!("`{name}` is of a type the Player doesn't know: {other:?}")),
            };
            let bound = |key: &str| i[key].as_f64().filter(|x| x.is_finite());
            inputs.push(Input {
                name,
                kind,
                default: i.get("default").cloned(),
                min: bound("min"),
                max: bound("max"),
                hint: text(&i["hint"], 500).unwrap_or_default(),
            });
        }
        Ok(Description {
            title: text(&v["title"], 100).filter(|t| !t.is_empty()),
            doc: text(&v["doc"], 2000).unwrap_or_default(),
            main: v["main"].as_bool().unwrap_or(false),
            inputs,
        })
    }
}

/// What the script at `path` asks for, read by `python` with the module at `client`: nothing of the script runs.
pub fn describe(python: &Python, client: &Path, path: &Path) -> Result<Description, String> {
    let mut c = python.command();
    c.arg(client).arg("describe").arg(path);
    let out = output(c, None, DESCRIBE, 1 << 20).map_err(|e| format!("Python {e}"))?;
    let line = out.stdout.lines().last().unwrap_or("");
    let v: Value = serde_json::from_str(line).map_err(|_| {
        let why = out.stderr.trim();
        if why.is_empty() { "Python said nothing about the script".to_string() } else { why.to_string() }
    })?;
    Description::from_json(&v)
}

/// The values `main` is called with: those `given`, checked against what each input is, and the defaults for the rest (a
/// selection's default being `selection`). An input with neither, a value of the wrong kind or out of its range, or a name
/// `main` doesn't take is an error that says which.
pub fn resolve(inputs: &[Input], given: &Map<String, Value>, selection: &[u64]) -> Result<Map<String, Value>, String> {
    if let Some(k) = given.keys().find(|k| !inputs.iter().any(|i| &i.name == *k)) {
        let names: Vec<&str> = inputs.iter().map(|i| i.name.as_str()).collect();
        return Err(if names.is_empty() {
            format!("the script asks for nothing, not `{k}`")
        } else {
            format!("the script asks for no `{k}`: it asks for {}", names.join(", "))
        });
    }
    let mut out = Map::new();
    for i in inputs {
        let v = match (given.get(&i.name), &i.default) {
            (Some(v), _) => v.clone(),
            (None, Some(Value::String(s))) if i.kind == Kind::Selection && s == "selection" => json!(selection),
            (None, Some(d)) => d.clone(),
            (None, None) => return Err(format!("`{}` needs a value", i.name)),
        };
        out.insert(i.name.clone(), check(i, v)?);
    }
    Ok(out)
}

fn check(i: &Input, v: Value) -> Result<Value, String> {
    let n = &i.name;
    let number = |v: &Value| -> Result<f64, String> {
        let x = v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("`{n}` is a number"))?;
        if let Some(lo) = i.min.filter(|lo| x < *lo) {
            return Err(format!("`{n}` is at least {lo}"));
        }
        if let Some(hi) = i.max.filter(|hi| x > *hi) {
            return Err(format!("`{n}` is at most {hi}"));
        }
        Ok(x)
    };
    match &i.kind {
        Kind::Bool => v.as_bool().map(Value::from).ok_or_else(|| format!("`{n}` is true or false")),
        Kind::Int => {
            let x = number(&v)?;
            if x.fract() != 0.0 || x.abs() > 9.0e15 {
                return Err(format!("`{n}` is a whole number"));
            }
            Ok(json!(x as i64))
        }
        Kind::Float | Kind::Length => {
            let x = number(&v)?;
            if i.kind == Kind::Length && x.abs() > bimcraft_core::model::MAX_COORD {
                return Err(format!("`{n}` is at most {} mm", bimcraft_core::model::MAX_COORD));
            }
            Ok(json!(x))
        }
        Kind::Text | Kind::Level | Kind::Library(_) => match v {
            Value::String(s) if s.chars().count() <= MAX_TEXT => Ok(Value::String(s)),
            Value::String(_) => Err(format!("`{n}` is at most {MAX_TEXT} characters")),
            _ => Err(format!("`{n}` is text")),
        },
        Kind::Choice(choices) => {
            if choices.contains(&v) {
                Ok(v)
            } else {
                let list: Vec<String> = choices.iter().map(Value::to_string).collect();
                Err(format!("`{n}` is one of {}", list.join(", ")))
            }
        }
        Kind::Selection => {
            let ids = v.as_array().filter(|a| a.len() <= MAX_IDS).ok_or_else(|| format!("`{n}` is a list of element ids"))?;
            ids.iter()
                .map(|id| id.as_u64().map(Value::from).ok_or_else(|| format!("`{n}` is a list of element ids")))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::from)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::python::tests::python;
    use crate::store::Store;

    const STUDS: &str = r#""""Studs along walls."""
__title__ = "Studs"
from typing import Annotated, Literal
import bimcraft as bc

def main(level: bc.Level, walls: bc.Selection = bc.SELECTION,
         spacing: Annotated[bc.Length, bc.Range(300, 5000)] = 1200,
         side: Literal["in", "out"] = "in", tag: bool = True, count: int = 3, prefix: str = "R",
         *, kind: Annotated[str, bc.Library("type:wall")] = "Generic - 200"):
    """Place a stud every `spacing` along each wall.

    spacing: centre to centre
    """
    raise SystemExit("describing must not run this")
"#;

    fn studs() -> Description {
        Description::from_json(&json!({
            "title": "Studs", "doc": "Place a stud every `spacing` along each wall.", "main": true,
            "inputs": [
                {"name": "level", "type": "level"},
                {"name": "walls", "type": "selection", "default": "selection"},
                {"name": "spacing", "type": "length", "min": 300, "max": 5000, "default": 1200, "hint": "centre to centre"},
                {"name": "side", "type": "choice", "choices": ["in", "out"], "default": "in"},
                {"name": "tag", "type": "bool", "default": true},
                {"name": "count", "type": "int", "default": 3},
                {"name": "prefix", "type": "text", "default": "R"},
                {"name": "kind", "type": "library", "library": "type:wall", "default": "Generic - 200"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn python_reads_what_main_asks_for_without_running_it() {
        let Some(py) = python() else { return };
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let path = store.save("Studs", STUDS).unwrap();
        let d = describe(&py, &store.client().unwrap(), &path).unwrap();
        assert_eq!(d, studs(), "{d:#?}");
        // What can't be read is said: a syntax error by its line, a type the Player doesn't know by its input.
        let path = store.save("broken", "def main(:\n  pass\n").unwrap();
        assert!(describe(&py, &store.client().unwrap(), &path).unwrap_err().starts_with("line 1"));
        let path = store.save("odd", "def main(x: dict = {}):\n  pass\n").unwrap();
        assert!(describe(&py, &store.client().unwrap(), &path).unwrap_err().contains("`x`"));
        let path = store.save("worked", "def main(x: int = 2 * 3):\n  pass\n").unwrap();
        assert!(describe(&py, &store.client().unwrap(), &path).unwrap_err().contains("written out"));
        // No main: it asks for nothing.
        let path = store.save("plain", "\"\"\"Just prints.\"\"\"\nprint(1)\n").unwrap();
        let d = describe(&py, &store.client().unwrap(), &path).unwrap();
        assert_eq!((d.main, d.inputs.len(), d.doc.as_str()), (false, 0, "Just prints."));
    }

    #[test]
    fn values_are_checked_against_what_each_input_is_and_the_rest_take_their_defaults() {
        let d = studs();
        let given = |v: Value| v.as_object().cloned().unwrap();
        let out = resolve(&d.inputs, &given(json!({"level": "Level 1", "spacing": 900.5})), &[100001, 100002]).unwrap();
        assert_eq!(
            Value::Object(out),
            json!({"level": "Level 1", "walls": [100001, 100002], "spacing": 900.5, "side": "in", "tag": true, "count": 3,
                   "prefix": "R", "kind": "Generic - 200"})
        );
        let err = |v: Value| resolve(&d.inputs, &given(v), &[]).unwrap_err();
        assert!(err(json!({})).contains("`level` needs a value"));
        assert!(err(json!({"level": "L", "spacing": 100})).contains("at least 300"));
        assert!(err(json!({"level": "L", "spacing": 9000})).contains("at most 5000"));
        assert!(err(json!({"level": "L", "spacing": "wide"})).contains("is a number"));
        assert!(err(json!({"level": "L", "side": "up"})).contains("one of \"in\", \"out\""));
        assert!(err(json!({"level": "L", "tag": 1})).contains("true or false"));
        assert!(err(json!({"level": "L", "count": 2.5})).contains("whole number"));
        assert!(err(json!({"level": "L", "walls": [1, "x"]})).contains("element ids"));
        assert!(err(json!({"level": 5})).contains("is text"));
        assert!(err(json!({"level": "L", "colour": "red"})).contains("asks for no `colour`"));
        assert!(resolve(&[], &given(json!({"x": 1})), &[]).unwrap_err().contains("asks for nothing"));
        let big = json!({"level": "L", "prefix": "x".repeat(MAX_TEXT + 1)});
        assert!(err(big).contains("at most"));
    }

    #[test]
    fn a_description_reads_back_what_it_writes() {
        let d = studs();
        assert_eq!(Description::from_json(&d.to_json()), Ok(d));
    }

    #[test]
    fn a_description_that_is_not_one_is_an_error() {
        assert_eq!(Description::from_json(&json!({"error": "line 3: bad"})), Err("line 3: bad".to_string()));
        assert!(Description::from_json(&json!({"inputs": [{"type": "int"}]})).unwrap_err().contains("no name"));
        assert!(Description::from_json(&json!({"inputs": [{"name": "x", "type": "dict"}]})).unwrap_err().contains("doesn't know"));
        let d = Description::from_json(&json!({"inputs": [{"name": "x", "type": "float", "min": "a", "max": null}]})).unwrap();
        assert_eq!((d.inputs[0].min, d.inputs[0].max), (None, None));
    }
}
