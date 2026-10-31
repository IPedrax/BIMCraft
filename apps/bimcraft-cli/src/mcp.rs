//! BIMCraft's MCP server: JSON-RPC 2.0 over stdio, one message per line (MCP's stdio transport).
//! Its tools run the same commands as the CLI and scripts, on a headless session, or, with `--connect`, on the session of a window
//! (`window.rs`): the project the user has open.

use std::io::{BufRead, Write};

use bimcraft_core::engine::{Session, catalog};
use serde_json::{Value, json};

use crate::window::Window;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED: &[&str] = &[PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "BIMCraft is a BIM modeler, a Revit-class building model. Lengths are millimetres; x east, y north, z up; \
the model starts with Level 1 (0) and Level 2 (3000). Model with execute: wall {points: [[0,0],[8000,0],[8000,6000],[0,6000]], \
closed: true}, door {at: [2000, 0]}, window {at: [8000, 3000]}, room {point: [4000, 3000], name: \"Living\"}, floor {} (open \
over a loop of walls standing free inside), \
roof {} (on Level 2, over the walls below, hipped, open over a loop of them standing free inside; gables: [1, 3] stands \
those edges upright; courtyards: [[[x,y],…]] slopes it round holes in it too), dormer {host: <roof id>, \
at: [4000, 800]} (on its slope), column {at: [4000, 3000]}, stair {start: [1000, 1000], end: [1000, 2000], \
railings: true} (with landings: [{at: 8, turn: -90}], it turns right after 8 risers), ramp {start: [-5000, 0], height: 600}, \
dimension {refs: [[0, 3000], [8000, 3000]], at: [4000, -1000]} (between the walls under those points), tag {} (every door and \
window); or send several at once with script, one `command {json}` per line. list_commands shows every command and its \
parameters, and the menu it sits in. Check your work with inspect_model and query_elements, look at it with render (a PNG of a level's plan, with \
ceiling: true its reflected ceiling plan, of a section or elevation by name, or of the 3D view with view \"{3D}\"; or a family \
alone with family, type and mode \"3d\", \"plan\" or \"elevation\" — which draws in no project of yours), and save \
to .bimcraft or .ifc.";

/// Where the tools run: on a session of the server's own, or on a window's.
enum Backend {
    Own(Session),
    Window(Window),
}

impl Backend {
    fn execute(&mut self, command: &str, params: &Value) -> Result<Value, String> {
        match self {
            Backend::Own(s) => s.execute(command, params),
            Backend::Window(w) => w.ask(json!({"op": "execute", "command": command, "params": params})),
        }
    }

    /// A script's results, as `{results: [...]}`.
    fn script(&mut self, text: &str, atomic: bool) -> Result<Value, String> {
        let results = match self {
            Backend::Own(s) if atomic => s.script_atomic(text).map(Value::from),
            Backend::Own(s) => s.script(text).map(Value::from),
            Backend::Window(w) => w.ask(json!({"op": "script", "text": text, "atomic": atomic})),
        };
        results.map(|results| json!({ "results": results }))
    }

    /// The server's own session, for what replaces a project: a window's is the user's, and is started and opened in the window.
    fn own(&mut self) -> Result<&mut Session, String> {
        match self {
            Backend::Own(s) => Ok(s),
            Backend::Window(_) => {
                Err("this is the window's project, which is the user's: start or open another in the window, not from here".into())
            }
        }
    }
}

pub struct Server {
    backend: Backend,
}

impl Default for Server {
    fn default() -> Self {
        Server::new()
    }
}

fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn text(v: &Value) -> Value {
    json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(v).unwrap_or_default()}]})
}

fn tool_error(e: &str) -> Value {
    json!({"content": [{"type": "text", "text": e}], "isError": true})
}

fn tools() -> Value {
    json!([
        {"name": "execute", "description": "Run any BIMCraft command with JSON params, e.g. wall {points: [[0,0],[5000,0]]}, door {at: [2500, 0]}, \
            room {point: [x, y], name}, modify {id, …fields}, delete {ids}, undo. list_commands shows them all with their parameters.",
         "inputSchema": {"type": "object", "properties": {"command": {"type": "string"}, "params": {"type": "object"}}, "required": ["command"]}},
        {"name": "script", "description": "Run several commands, one per line: `wall {\"points\": [[0,0],[4000,0]]}`. Stops at the first failing line and names it; the lines before it stay done, each its own undo step. \
            With `atomic`, all or nothing: a failing line takes back the lines before it, and a script that goes through is one undo step.",
         "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}, "atomic": {"type": "boolean"}}, "required": ["text"]}},
        {"name": "list_commands", "description": "Every command with its id, label, Revit-style shortcut, parameters and the menu it sits in (`menu`, null when the interface has no place for it). `filter` matches part of an id, label or shortcut.",
         "inputSchema": {"type": "object", "properties": {"filter": {"type": "string"}}}},
        {"name": "inspect_model", "description": "The model at a glance: levels, element counts, types, warnings (such as rooms no longer enclosed) and undo history.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "query_elements", "description": "Elements with their six-digit `id` and their `guid` (never changes), fields and derived values (wall length and height, room area_m2 and enclosed, door position `at`); what an element points at (level, host, view) is a GUID, and commands take an element by id or GUID. Filter by category and level; page with limit and offset.",
         "inputSchema": {"type": "object", "properties": {"category": {"type": "string"}, "level": {"type": ["string", "integer"]}, "limit": {"type": "integer"}, "offset": {"type": "integer"}}}},
        {"name": "schedule", "description": "A schedule's columns, rows and totals, and the fields it could show (`available`). `category` alone reads that category's own: rooms (number, name, level, area, perimeter), doors, windows, walls, floors, levels; ceilings, roofs, columns, components, stairs, ramps and railings too. `name` and `category` make a new one. `fields` (names, or {name, formula, kind, heading, total}: a calculated one such as Area * 120, lengths in mm and areas in m²), `filters` ([{field, op: equals, greater, contains, has-value…, value}]), `sort` ([field or {field, descending}]) and `grouped` change it, each list replacing the last.",
         "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "category": {"type": "string"}, "fields": {"type": "array"}, "filters": {"type": "array"}, "sort": {"type": "array"}, "grouped": {"type": "boolean"}}}},
        {"name": "render", "description": "Look at the model as a PNG: a level's floor plan (walls and columns with poché, doors with swings, windows, stairs, ramps and railings, roofs, room tags with areas, grids, dimensions, door and window tags), or with ceiling true its reflected ceiling plan, a section or elevation by name (the cut in poché, what's beyond, levels and grids), a sheet by number (its title block and the views on it), or with view \"{3D}\" the 3D view, seen from azimuth degrees round from east (counter-clockwise) and elevation degrees up; by default from the south-east, 30° up. With `family`, that family alone instead of the model: its `type` (the first by default) in a model of its own — in a wall `wall_thickness` mm thick (200) if it's a door's or a window's, on a floor of its own footprint if it isn't — mode \"3d\" (default), \"plan\" (what its symbols draw) or \"elevation\" (front-on); the project is touched by none of it and keeps no elements and no undo steps from it. `path` writes a .png there instead of sending it back.",
         "inputSchema": {"type": "object", "properties": {"view": {"type": ["string", "integer"]}, "family": {"type": "string"}, "type": {"type": "string"}, "mode": {"type": "string", "enum": ["3d", "plan", "elevation"]}, "wall_thickness": {"type": "number"}, "ceiling": {"type": "boolean"}, "width": {"type": "integer"}, "height": {"type": "integer"}, "azimuth": {"type": "number"}, "elevation": {"type": "number"}, "path": {"type": "string"}}}},
        {"name": "new_model", "description": "Start an empty project with Level 1 and Level 2.",
         "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}}},
        {"name": "open", "description": "Open a .bimcraft project, import a Revit .rvt file (read directly, no Revit needed: its levels, walls, floors, ceilings, doors and windows, columns, rooms and flat roofs come in, and the reply lists what was left out and why), or import an .ifc file (IFC2x3 or IFC4, from Revit or any BIM tool): its storeys, straight and curved walls with their doors and windows, floors (level or sloped) and ceilings, with their holes, rooms, columns, grids, stairs and ramps of straight runs with landings, railings, shafts and roofs (flat, sloping all round over any footprint with gable ends, courtyards and openings, or one profile swept across) come in measured from their geometry; the reply lists what was left out and why.",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}},
        {"name": "save", "description": "Save to .bimcraft (the project), or export by extension: .ifc (IFC4 for other BIM tools), .svg or .png (the lowest level's plan), .pdf (every sheet, a page each).",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}}}
    ])
}

impl Server {
    pub fn new() -> Self {
        let mut session = Session::new();
        session.dwg = crate::dwg_reader();
        Server { backend: Backend::Own(session) }
    }

    /// A server whose tools work on the project `window` has open.
    pub fn attached(window: Window) -> Self {
        Server { backend: Backend::Window(window) }
    }

    /// Serve until `input` closes. Logs go to stderr: stdout carries only protocol messages.
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            if let Some(reply) = self.handle_line(&line?) {
                writeln!(output, "{reply}")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) if batch.is_empty() => Some(error(Value::Null, INVALID_REQUEST, "empty batch")),
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch.into_iter().filter_map(|m| self.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(msg) => self.handle(msg),
            Err(e) => Some(error(Value::Null, PARSE_ERROR, format!("parse error: {e}"))),
        };
        reply.map(|r| r.to_string())
    }

    fn handle(&mut self, msg: Value) -> Option<Value> {
        let Some(o) = msg.as_object() else { return Some(error(Value::Null, INVALID_REQUEST, "a message must be an object")) };
        let id = o.get("id").cloned();
        let Some(method) = o.get("method").and_then(Value::as_str) else {
            // A response to us: there is nothing to answer.
            if o.contains_key("result") || o.contains_key("error") {
                return None;
            }
            return Some(error(id.unwrap_or(Value::Null), INVALID_REQUEST, "missing `method`"));
        };
        // Notifications (no id) get no reply.
        let id = id?;
        let params = o.get("params").cloned().unwrap_or(Value::Null);
        Some(match self.request(method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => error(id, code, message),
        })
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
                let version = if SUPPORTED.contains(&asked) { asked } else { PROTOCOL_VERSION };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "bimcraft", "title": "BIMCraft", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": match self.backend {
                        Backend::Own(_) => INSTRUCTIONS.to_string(),
                        Backend::Window(_) => format!("{INSTRUCTIONS} You are attached to the window the user has open: what you make is in their project, to see and to undo. new_model and open are not available; the user starts and opens projects in the window."),
                    },
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing tool `name`".to_string()))?;
                self.call(name, params.get("arguments").unwrap_or(&Value::Null))
            }
            _ => Err((METHOD_NOT_FOUND, format!("method `{method}` not found"))),
        }
    }

    /// A tool call. `Err` is a protocol error (unknown tool, arguments that aren't an object or lack
    /// a required one); a tool that runs and fails answers with `isError`.
    fn call(&mut self, name: &str, args: &Value) -> Result<Value, (i64, String)> {
        let tools = tools();
        let tool = tools
            .as_array()
            .and_then(|a| a.iter().find(|t| t["name"] == name))
            .ok_or_else(|| (INVALID_PARAMS, format!("unknown tool `{name}`")))?;
        let args = match args {
            Value::Null => json!({}),
            Value::Object(_) => args.clone(),
            _ => return Err((INVALID_PARAMS, format!("the arguments of `{name}` must be an object"))),
        };
        for key in tool["inputSchema"]["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if args.get(key).is_none_or(Value::is_null) {
                return Err((INVALID_PARAMS, format!("`{name}` needs `{key}`")));
            }
        }
        let b = &mut self.backend;
        let result = match name {
            "execute" => {
                let params = args.get("params").cloned().unwrap_or_else(|| json!({}));
                b.execute(args["command"].as_str().unwrap_or_default(), &params)
            }
            "script" => {
                let text = args["text"].as_str().unwrap_or_default();
                let atomic = match args.get("atomic") {
                    None | Some(Value::Null) => false,
                    Some(Value::Bool(b)) => *b,
                    Some(v) => return Err((INVALID_PARAMS, format!("`atomic` is true or false, not {v}"))),
                };
                b.script(text, atomic)
            }
            "list_commands" => {
                let filter = args["filter"].as_str().map(str::to_ascii_lowercase).unwrap_or_default();
                let has = |v: &Value| v.as_str().is_some_and(|t| t.to_ascii_lowercase().contains(&filter));
                let all = catalog();
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
            "inspect_model" => b.execute("inspect", &json!({})),
            "query_elements" => b.execute("elements", &args),
            "schedule" => b.execute("schedule", &args),
            "new_model" => b.own().and_then(|s| s.execute("new", &args)),
            "open" if args["path"].as_str().is_some_and(crate::revit::is_revit) => {
                b.own().and_then(|s| crate::revit::open(s, args["path"].as_str().unwrap_or_default()))
            }
            "open" => b.own().and_then(|s| s.execute("open", &args)),
            "save" => b.execute("save", &args),
            "render" => {
                let keys =
                    ["family", "type", "mode", "wall_thickness", "view", "ceiling", "width", "height", "azimuth", "elevation", "path"];
                let view: serde_json::Map<String, Value> = keys.iter().map(|k| (k.to_string(), args[*k].clone())).collect();
                return Ok(match b.execute("render", &Value::Object(view)) {
                    Ok(v) => {
                        let name = v["view"].as_str().unwrap_or("");
                        let what = match v["kind"].as_str() {
                            Some("3D view") => {
                                let deg = |k: &str| v[k].as_f64().unwrap_or_default();
                                format!("3D view from {}° round, {}° up", deg("azimuth"), deg("elevation"))
                            }
                            Some(kind) => format!("{name} ({kind})"),
                            None => name.to_string(),
                        };
                        // With a `path` the picture is written to it, and there is no picture to send back.
                        let mut text = match (v["width"].as_u64(), v["height"].as_u64()) {
                            (Some(w), Some(h)) => format!("{what}, {w} x {h} px"),
                            _ => what,
                        };
                        if let Some(path) = v["path"].as_str() {
                            text.push_str(&format!(", written to {path}"));
                        }
                        let mut content = vec![json!({"type": "text", "text": text})];
                        if v["pngBase64"].is_string() {
                            content.insert(0, json!({"type": "image", "data": v["pngBase64"], "mimeType": "image/png"}));
                        }
                        json!({"content": content})
                    }
                    Err(e) => tool_error(&e),
                });
            }
            _ => return Err((INVALID_PARAMS, format!("unknown tool `{name}`"))),
        };
        Ok(match result {
            Ok(v) => text(&v),
            Err(e) => tool_error(&e),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        serde_json::from_str(&s.handle_line(&line).expect("a reply")).unwrap()
    }

    /// A tool's result: its text content parsed as JSON, or its first content item.
    fn tool(s: &mut Server, name: &str, args: Value) -> Value {
        let r = call(s, 9, "tools/call", json!({"name": name, "arguments": args}));
        let content = &r["result"]["content"][0];
        assert_ne!(r["result"]["isError"], json!(true), "{name} failed: {content}");
        if content["type"] == "text" { serde_json::from_str(content["text"].as_str().unwrap()).unwrap() } else { content.clone() }
    }

    #[test]
    fn lifecycle_and_tool_list() {
        let mut s = Server::new();
        let init = call(&mut s, 1, "initialize", json!({"protocolVersion": "2025-06-18"}));
        assert_eq!(init["result"]["serverInfo"]["name"], "bimcraft");
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        let tools = call(&mut s, 2, "tools/list", json!({}));
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"execute") && names.contains(&"render"), "{names:?}");
        assert_eq!(call(&mut s, 3, "nope", json!({}))["error"]["code"], -32601);
        let old = call(&mut s, 4, "initialize", json!({"protocolVersion": "1999-01-01"}));
        assert_eq!(old["result"]["protocolVersion"], PROTOCOL_VERSION);
    }

    /// `list_commands` says where each command sits in the ribbon (task 6 of ТЗ-003), and leaves
    /// `null` where the interface has no place for it.
    #[test]
    fn list_commands_says_where_the_command_sits_in_the_interface() {
        let mut s = Server::new();
        let list = tool(&mut s, "list_commands", json!({}));
        let by = |id: &str| {
            list.as_array().and_then(|a| a.iter().find(|c| c["id"] == id)).unwrap_or_else(|| panic!("no `{id}` in the catalog")).clone()
        };
        assert_eq!(by("family")["menu"], json!("Manage > Family Editor"), "the button the agent looked for");
        assert_eq!(by("door")["menu"], json!("Architecture > Door"));
        assert_eq!(by("purge")["menu"], json!("Manage > Purge Unused"));
        assert_eq!(by("undo")["menu"], json!(null), "a shortcut with no place in the ribbon");
        let filtered = tool(&mut s, "list_commands", json!({"filter": "family"}));
        assert!(filtered.as_array().is_some_and(|a| a.iter().any(|c| c["menu"] == json!("Manage > Family Editor"))), "{filtered}");
    }

    #[test]
    fn an_agent_models_a_house_and_checks_it_through_mcp_only() {
        let mut s = Server::new();
        tool(&mut s, "new_model", json!({"name": "Cabin"}));
        tool(
            &mut s,
            "execute",
            json!({"command": "wall", "params": {"points": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "closed": true}}),
        );
        let script =
            "door {\"at\": [3000, 0]}\nwindow {\"at\": [6000, 2000]}\nroom {\"point\": [3000, 2000], \"name\": \"Cabin\"}\nfloor {}";
        tool(&mut s, "script", json!({ "text": script }));
        let m = tool(&mut s, "inspect_model", json!({}));
        assert_eq!(m["counts"], json!({"door": 1, "floor": 1, "level": 2, "room": 1, "wall": 4, "window": 1}));
        let doors = tool(&mut s, "query_elements", json!({"category": "door"}));
        assert_eq!(doors["elements"][0]["at"], json!([3000.0, 0.0]));
        let rooms = tool(&mut s, "schedule", json!({"category": "rooms"}));
        assert_eq!(rooms["rows"][0][3], 24.0);
        let big = json!({"name": "Big Rooms", "category": "rooms", "fields": ["Name", {"name": "Cost", "formula": "Area * 100", "total": true}], "filters": [{"field": "Area", "op": "greater", "value": 20}]});
        let big = tool(&mut s, "schedule", big);
        assert_eq!((big["rows"].clone(), big["totals"].clone()), (json!([["Cabin", 2400.0]]), json!([null, 2400.0])));
        let img = tool(&mut s, "render", json!({"width": 400, "height": 300}));
        assert_eq!((img["type"].as_str(), img["mimeType"].as_str()), (Some("image"), Some("image/png")));
        let r = call(&mut s, 10, "tools/call", json!({"name": "render", "arguments": {"view": "{3D}", "azimuth": 135, "elevation": 20}}));
        assert_eq!(r["result"]["content"][1]["text"], "3D view from 135° round, 20° up, 1600 x 1000 px");
        let path = std::env::temp_dir().join(format!("bimcraft-mcp-{}.ifc", std::process::id()));
        tool(&mut s, "save", json!({ "path": path }));
        assert!(std::fs::read_to_string(&path).unwrap().contains("IFCSPACE("));
        let _ = std::fs::remove_file(path);
    }

    /// The render card offers a family's preview, and a preview through MCP leaves nothing in the project.
    #[test]
    fn render_draws_a_family_on_its_own_and_the_project_keeps_nothing_of_it() {
        let mut s = Server::new();
        tool(&mut s, "new_model", json!({ "name": "Preview" }));
        tool(
            &mut s,
            "execute",
            json!({"command": "family", "params": {
                "name": "Stool", "parameters": [{"name": "Height", "value": 450}],
                "types": {"A": {}, "B": {"Height": 460}},
                "forms": [
                    {"profile": [[0, 0], [320, 0], [320, 320], [0, 320]], "from": 420, "to": 450, "material": "Timber"},
                    {"profile": [[20, 20], [60, 20], [60, 60], [20, 60]], "to": 420, "material": "Timber"}
                ]
            }}),
        );
        let before = tool(&mut s, "inspect_model", json!({}));
        for mode in ["3d", "plan", "elevation"] {
            let r = call(
                &mut s,
                7,
                "tools/call",
                json!({"name": "render", "arguments": {"family": "Stool", "type": "A", "mode": mode, "width": 300, "height": 240}}),
            );
            let content = &r["result"]["content"];
            assert_eq!(content[0]["type"], json!("image"), "{mode}: {r}");
            assert_eq!(content[0]["mimeType"], json!("image/png"), "{mode}: {r}");
            assert_eq!(content[1]["text"], json!("Stool (family 3d preview), 300 x 240 px".replace("3d", mode)), "{mode}: {r}");
        }
        // The render card says what a family's preview takes, so an agent asks for one.
        let tools = call(&mut s, 2, "tools/list", json!({}));
        let render = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "render").unwrap();
        assert_eq!(render["inputSchema"]["properties"]["mode"]["enum"], json!(["3d", "plan", "elevation"]));
        for key in ["family", "type", "wall_thickness"] {
            assert!(render["inputSchema"]["properties"].get(key).is_some(), "{key} in the render card");
        }
        assert!(render["description"].as_str().unwrap().contains("family"), "the card says what it draws");
        // Nothing of it stood in the project: no element, no undo step.
        let after = tool(&mut s, "inspect_model", json!({}));
        assert_eq!(after["counts"], before["counts"], "a preview makes no element");
        assert_eq!(after["history"], before["history"], "a preview makes no undo step");
        // A family's preview written to a path, and a family that isn't there, is a tool error naming it.
        let path = std::env::temp_dir().join(format!("bimcraft-mcp-family-{}.png", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let r = call(&mut s, 8, "tools/call", json!({"name": "render", "arguments": {"family": "Stool", "mode": "plan", "path": path}}));
        assert_eq!(r["result"]["content"].as_array().map(|c| c.len()), Some(1), "{r}");
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("written to"), "{r}");
        assert_eq!(&std::fs::read(&path).unwrap()[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let _ = std::fs::remove_file(&path);
        let r = call(&mut s, 9, "tools/call", json!({"name": "render", "arguments": {"family": "Nowhere"}}));
        assert!(
            r["result"]["isError"] == true && r["result"]["content"][0]["text"].as_str().unwrap().contains("there's no family `Nowhere`"),
            "{r}"
        );
    }

    #[cfg(feature = "rvt")]
    #[test]
    fn open_imports_a_revit_file_through_the_same_importer_as_the_command_line() {
        let path = crate::revit::tests::house("mcp.rvt");
        let mut s = Server::new();
        let reply = tool(&mut s, "open", json!({ "path": path }));
        assert_eq!((reply["format"].as_str(), reply["elements"].clone()), (Some("Revit 2024"), json!({"level": 2, "wall": 1})));
        assert_eq!(tool(&mut s, "inspect_model", json!({}))["counts"], json!({"level": 2, "wall": 1}));
        // A file that is not Revit's is a tool error that names it, and the model stays as it was.
        let fake = std::env::temp_dir().join(format!("bimcraft-mcp-{}-fake.rvt", std::process::id()));
        std::fs::write(&fake, b"not a Revit file").unwrap();
        let r = call(&mut s, 11, "tools/call", json!({"name": "open", "arguments": {"path": fake}}));
        assert!(r["result"]["isError"] == true && r["result"]["content"][0]["text"].as_str().unwrap().contains("fake.rvt"), "{r}");
        assert_eq!(tool(&mut s, "inspect_model", json!({}))["counts"], json!({"level": 2, "wall": 1}));
        let _ = std::fs::remove_file(fake);
    }

    #[cfg(not(feature = "rvt"))]
    #[test]
    fn open_of_a_revit_file_in_a_build_without_revit_support_is_a_tool_error_that_says_so() {
        let mut s = Server::new();
        let r = call(&mut s, 1, "tools/call", json!({"name": "open", "arguments": {"path": "x.rvt"}}));
        assert!(r["result"]["isError"] == true && r["result"]["content"][0]["text"].as_str().unwrap().contains("without Revit support"));
    }

    #[test]
    fn failures_are_tool_errors_and_bad_calls_are_protocol_errors() {
        let mut s = Server::new();
        let r = call(&mut s, 1, "tools/call", json!({"name": "execute", "arguments": {"command": "door", "params": {"at": [0, 0]}}}));
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("no wall"));
        assert_eq!(call(&mut s, 2, "tools/call", json!({"name": "execute", "arguments": {}}))["error"]["code"], -32602);
        assert_eq!(call(&mut s, 3, "tools/call", json!({"name": "teleport", "arguments": {}}))["error"]["code"], -32602);
        assert_eq!(call(&mut s, 4, "tools/call", json!({"name": "script", "arguments": "x"}))["error"]["code"], -32602);
        assert!(s.handle_line("{not json").unwrap().contains("-32700"));
        assert!(s.handle_line("[]").unwrap().contains("-32600"));
    }

    #[test]
    fn an_atomic_script_through_mcp_is_all_or_nothing_and_one_undo_step() {
        let mut s = Server::new();
        let levels = |s: &mut Server| tool(s, "inspect_model", json!({}))["counts"]["level"].clone();
        let r = call(
            &mut s,
            1,
            "tools/call",
            json!({"name": "script", "arguments": {"text": "level {\"elevation\": 6000}\nnope", "atomic": true}}),
        );
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("line 2"));
        assert_eq!(levels(&mut s), 2, "the first line is taken back");
        tool(&mut s, "script", json!({"text": "level {\"elevation\": 6000}\nlevel {\"elevation\": 9000}", "atomic": true}));
        assert_eq!(levels(&mut s), 4);
        tool(&mut s, "execute", json!({"command": "undo"}));
        assert_eq!(levels(&mut s), 2, "one undo takes back the whole script");
        let r = call(&mut s, 2, "tools/call", json!({"name": "script", "arguments": {"text": "inspect", "atomic": "yes"}}));
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn a_server_attached_to_a_window_works_on_its_project_and_leaves_starting_and_opening_to_it() {
        let entry = crate::window::tests::stand_in(usize::MAX);
        let mut s = Server::attached(Window::connect_to(&entry).unwrap());
        let init = call(&mut s, 1, "initialize", json!({"protocolVersion": "2025-06-18"}));
        assert!(init["result"]["instructions"].as_str().unwrap().contains("attached to the window"), "{init}");
        tool(
            &mut s,
            "execute",
            json!({"command": "wall", "params": {"points": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "closed": true}}),
        );
        tool(
            &mut s,
            "script",
            json!({"text": "door {\"at\": [3000, 0]}\nroom {\"point\": [3000, 2000], \"name\": \"Cabin\"}", "atomic": true}),
        );
        let m = tool(&mut s, "inspect_model", json!({}));
        assert_eq!(m["counts"]["wall"], 4);
        assert_eq!(m["counts"]["door"], 1, "what the agent made is in the window's project");
        assert_eq!(tool(&mut s, "query_elements", json!({"category": "room"}))["elements"][0]["name"], "Cabin");
        assert_eq!(tool(&mut s, "schedule", json!({"category": "rooms"}))["rows"][0][3], 24.0);
        let img = tool(&mut s, "render", json!({"width": 400, "height": 300}));
        assert_eq!((img["type"].as_str(), img["mimeType"].as_str()), (Some("image"), Some("image/png")));
        // list_commands is the engine's catalog, with or without a window.
        assert!(tool(&mut s, "list_commands", json!({"filter": "wall"})).as_array().is_some_and(|a| !a.is_empty()));
        // The window's project is the user's: new_model and open are the window's to do, and the project stays.
        for (name, args) in
            [("new_model", json!({})), ("open", json!({"path": "/x/house.bimcraft"})), ("open", json!({"path": "/x/house.rvt"}))]
        {
            let r = call(&mut s, 5, "tools/call", json!({"name": name, "arguments": args}));
            assert!(
                r["result"]["isError"] == true && r["result"]["content"][0]["text"].as_str().unwrap().contains("the user's"),
                "{name}: {r}"
            );
        }
        assert_eq!(tool(&mut s, "inspect_model", json!({}))["counts"]["wall"], 4);
        // What the window refuses by its own rules comes back as the tool's error, as the window says it.
        let r = call(&mut s, 6, "tools/call", json!({"name": "execute", "arguments": {"command": "new"}}));
        assert!(
            r["result"]["isError"] == true && r["result"]["content"][0]["text"].as_str().unwrap().contains("would replace the project"),
            "{r}"
        );
    }

    #[test]
    fn a_window_that_has_gone_is_a_tool_error_and_the_server_goes_on() {
        let entry = crate::window::tests::stand_in(2);
        let mut s = Server::attached(Window::connect_to(&entry).unwrap());
        tool(&mut s, "inspect_model", json!({}));
        let r = call(&mut s, 2, "tools/call", json!({"name": "inspect_model", "arguments": {}}));
        assert!(
            r["result"]["isError"] == true && r["result"]["content"][0]["text"].as_str().unwrap().contains("window closed the connection"),
            "{r}"
        );
        assert_eq!(call(&mut s, 3, "ping", json!({}))["result"], json!({}), "the server answers still");
    }

    #[test]
    fn serve_answers_each_request_on_its_own_line() {
        let mut s = Server::new();
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n\
                     {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
                     {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
        let mut out = Vec::new();
        s.serve(input.as_bytes(), &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.iter().map(|l| l["id"].as_u64()).collect::<Vec<_>>(), [Some(1), Some(2)]);
    }
}
