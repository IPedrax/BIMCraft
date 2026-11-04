//! An example BIMCraft plugin: the rooms' areas, read with a command and written back into the project with another.
//!
//! `summary` reads the rooms (`elements {category: "room"}`) and returns how many there are and their total area.
//! `write_note` does that and puts the total on the plan as a text note at `at`. A project with no rooms is an error:
//! the call comes back failed, and the plugin's transaction leaves the project as it was.

use bimcraft_plugin_sdk::{Value, execute, info_log};
use serde_json::json;

/// How many rooms there are and how much floor they have, m²; with `level`, only that level's.
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
        "rooms_without_area": list.iter().filter(|r| r["area_m2"].as_f64().unwrap_or(0.0) <= 0.0).count(),
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
}
