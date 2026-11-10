//! The tools the model is offered: the MCP server's reading and editing ones, run on the window's session through the same
//! [`bridge::run`] that answers an agent connected to the window, so they follow its rules (a project is never replaced, and a
//! window that lets agents only read lets the assistant only read). On top of that: no command that reads or writes a file (a
//! model steered by a note inside the project must not be able to write the user's disk), no picture (the model has no use for
//! a PNG's bytes), and every answer is capped at [`MAX_RESULT`].

use bimcraft_core::bridge::{self, Access, Request};
use bimcraft_core::engine::{Session, find, touches_files};
use serde_json::{Value, json};

use crate::{Call, MAX_RESULT, clip};

pub struct Tool {
    pub name: &'static str,
    pub about: &'static str,
    /// JSON Schema of its arguments: plain types only, so all three providers take it.
    pub schema: Value,
}

pub fn all() -> Vec<Tool> {
    vec![
        Tool {
            name: "execute",
            about: "Run a BIMCraft command with JSON params, e.g. wall {points: [[0,0],[5000,0]]}, door {at: [2500, 0]}, room {point: [x, y], name}, \
                    modify {id, ...fields}, delete {ids}. list_commands shows every command with its parameters. A command that fails changes nothing.",
            schema: json!({"type": "object", "properties": {
                "command": {"type": "string", "description": "a command id such as wall, or its shortcut"},
                "params": {"type": "object", "description": "the command's parameters, as JSON"},
            }, "required": ["command"]}),
        },
        Tool {
            name: "list_commands",
            about: "The commands with their id, label, shortcut and parameters. Pass a filter (part of an id, label or shortcut): the whole list is long.",
            schema: json!({"type": "object", "properties": {"filter": {"type": "string", "description": "part of an id, label or shortcut"}}}),
        },
        Tool {
            name: "inspect_model",
            about: "The project at a glance: levels, element counts, types, warnings and undo history.",
            schema: json!({"type": "object", "properties": {}}),
        },
        Tool {
            name: "query_elements",
            about: "Elements with their six-digit id and their guid, their fields and derived values (wall length and height, room area, door position). \
                    Filter by category and level; page with limit and offset.",
            schema: json!({"type": "object", "properties": {
                "category": {"type": "string", "description": "such as wall, door, window, room, floor"},
                "level": {"type": "string", "description": "a level's name"},
                "limit": {"type": "integer", "description": "at most this many (50 by default, 200 at most)"},
                "offset": {"type": "integer", "description": "skip this many"},
            }}),
        },
    ]
}

/// A tool's result as text for the model: compact JSON, cut at [`MAX_RESULT`] bytes, saying what was left out.
pub fn to_text(v: &Value) -> String {
    let text = v.to_string();
    if text.len() <= MAX_RESULT {
        return text;
    }
    let mut cut = MAX_RESULT;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}… [{} more bytes left out: ask for less, with a filter, a category or a limit]", &text[..cut], text.len() - cut)
}

fn exec(session: &mut Session, command: &str, params: Value, access: Access) -> Result<Value, String> {
    bridge::run(session, &Request::Execute { command: command.into(), params, read_only: false }, access)
}

/// Runs a call on the session. `Err` is what the model is told went wrong.
pub fn run(session: &mut Session, call: &Call, access: Access) -> Result<Value, String> {
    let args = match &call.args {
        Value::Object(_) => call.args.clone(),
        Value::Null => json!({}),
        _ => return Err(format!("the arguments of `{}` must be a JSON object", call.name)),
    };
    match call.name.as_str() {
        "execute" => {
            let command = args["command"].as_str().ok_or("`execute` needs a `command`, a string")?;
            let params = match args.get("params") {
                None | Some(Value::Null) => json!({}),
                Some(p @ Value::Object(_)) => p.clone(),
                // Gemini declares the free-form object as text.
                Some(Value::String(s)) => serde_json::from_str(s).ok().filter(Value::is_object).ok_or("`params` is a JSON object")?,
                Some(_) => return Err("`params` is a JSON object".into()),
            };
            if let Some(c) = find(command) {
                if c.id == "render" {
                    return Err("the assistant cannot look at pictures: query_elements and inspect_model say what is there".into());
                }
                if touches_files(c, &params).is_some() {
                    return Err(format!(
                        "`{}` with those params reads or writes a file, which the assistant never does: the user opens, saves and exports from the File menu",
                        c.id
                    ));
                }
            }
            exec(session, command, params, access)
        }
        "list_commands" => {
            let filter = args["filter"].as_str().map(str::to_ascii_lowercase).unwrap_or_default();
            let has = |v: &Value| v.as_str().is_some_and(|t| t.to_ascii_lowercase().contains(&filter));
            let all = bridge::run(session, &Request::Catalog, access)?;
            Ok(all
                .as_array()
                .map(|cmds| {
                    cmds.iter()
                        .filter(|c| has(&c["id"]) || has(&c["label"]) || c["aliases"].as_array().is_some_and(|a| a.iter().any(has)))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default())
        }
        "inspect_model" => exec(session, "inspect", json!({}), access),
        "query_elements" => {
            let mut params = serde_json::Map::new();
            for key in ["category", "level", "offset"] {
                if let Some(v) = args.get(key).filter(|v| !v.is_null()) {
                    params.insert(key.into(), v.clone());
                }
            }
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50).clamp(1, 200);
            params.insert("limit".into(), json!(limit));
            exec(session, "elements", Value::Object(params), access)
        }
        other => Err(format!("unknown tool `{}`: execute, list_commands, inspect_model or query_elements", clip(other, 40))),
    }
}
