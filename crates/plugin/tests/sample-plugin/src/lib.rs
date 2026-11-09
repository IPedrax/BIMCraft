//! A plugin built with the guest SDK, for the host's tests: the rooms' areas, read with a command and written back
//! into the project with another. It is the end-to-end check of the SDK — `export!`, `bimcraft_alloc`/`bimcraft_free`,
//! `bimcraft_run` and reading the call's pointers out of linear memory — not a user-facing example: the repository
//! has none, and `docs/areas/plugins.md` is what an author builds from.

use bimcraft_plugin_sdk::{Value, execute, info_log};
use serde_json::json;

/// How many rooms there are and how much floor they have, m².
fn areas(params: &Value) -> Result<Value, String> {
    let mut query = json!({"category": "room", "limit": 5000});
    if let Some(level) = params.get("level").filter(|v| !v.is_null()) {
        query["level"] = level.clone();
    }
    let rooms = execute("elements", &query)?;
    let list = rooms["elements"].as_array().cloned().unwrap_or_default();
    let total: f64 = list.iter().filter_map(|r| r["area_m2"].as_f64()).sum();
    let total = if total == 0.0 { 0.0 } else { (total * 100.0).round() / 100.0 };
    Ok(json!({
        "rooms": rooms["count"].as_u64().unwrap_or(list.len() as u64),
        "total_area_m2": total,
    }))
}

/// `summary`: read only, so it makes no undo step.
fn summary(params: &Value) -> Result<Value, String> {
    let report = areas(params)?;
    info_log(&format!("{} room(s), {} m²", report["rooms"], report["total_area_m2"]));
    Ok(report)
}

/// `write_note`: one command, so the whole call is one undo step.
fn write_note(params: &Value) -> Result<Value, String> {
    let report = areas(params)?;
    let rooms = report["rooms"].as_u64().unwrap_or(0);
    if rooms == 0 {
        return Err("there are no rooms to sum: draw walls round a room first".to_string());
    }
    let at = params.get("at").cloned().unwrap_or_else(|| json!([0, 0]));
    let text = format!("Rooms: {rooms} · {} m²", report["total_area_m2"]);
    let note = execute("text", &json!({"at": at, "text": text}))?;
    info_log(&format!("wrote the area note at {}", note["id"]));
    Ok(json!({"note": note["id"], "rooms": rooms, "total_area_m2": report["total_area_m2"]}))
}

bimcraft_plugin_sdk::export! {
    "summary" => summary,
    "write_note" => write_note,
    "fail" => fail,
    "open_file" => open_file,
}

/// `open_file`: opens the project at `path` in place of this one, which is what a plugin does with the `files`
/// permission. The window asks for the file when its manifest makes `path` required.
fn open_file(params: &Value) -> Result<Value, String> {
    let path = params.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()).ok_or("`path` is required")?;
    let reply = execute("open", &json!({"path": path}))?;
    Ok(json!({"report": format!("OPENED {path}"), "open": reply}))
}

/// `fail`: changes the model, then fails, which is how a call's transaction is tested: nothing may stay.
fn fail(_: &Value) -> Result<Value, String> {
    execute("level", &json!({"elevation": 1200, "name": "Half done"}))?;
    Err("the plugin changed the project and then failed".to_string())
}