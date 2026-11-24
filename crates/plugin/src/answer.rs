//! What a plugin's function answers with in `panel`: the values it gives its widgets, a block of text, a table, a badge
//! and an error line. It is untrusted input as the manifest is, so the window checks it before it draws any of it: a
//! broken answer is one line in the plugin log, and the panel keeps what it showed.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// The most rows a panel's table may have.
pub const MAX_ROWS: usize = 2000;
/// The most cells a row may have.
pub const MAX_CELLS: usize = 32;
/// The longest a cell's text may be; longer is cut.
pub const MAX_CELL_TEXT: usize = 1024;
/// The longest a panel's block of text may be; longer is cut.
pub const MAX_PANEL_TEXT: usize = 64 << 10;
/// The longest the panel's error line may be.
pub const MAX_ERROR_TEXT: usize = 4096;
/// The longest the panel's badge may be.
pub const MAX_BADGE_TEXT: usize = 64;
/// The longest a widget's value may be as text.
pub const MAX_VALUE_TEXT: usize = 4096;
/// The most widget values an answer may set.
pub const MAX_VALUES: usize = 512;

/// What a panel shows of an answer: only what the plugin sent changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelAnswer {
    /// Values of widgets, by their id. An id the panel hasn't is ignored, with one line in the log.
    pub values: BTreeMap<String, Value>,
    /// The text a `text_panel`, or the panel's read-only `text` widgets, show.
    pub text: Option<String>,
    /// The table a `table` widget shows.
    pub table: Option<PanelTable>,
    /// A count or a state shown beside the panel's title.
    pub badge: Option<String>,
    /// A line in red at the foot of the panel. It takes nothing back.
    pub error: Option<String>,
}

/// A table of a panel's answer: its rows, each a list of cells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelTable {
    pub rows: Vec<Vec<Value>>,
}

impl PanelAnswer {
    /// The answer a function's `panel` holds, checked: an object, the fields it knows, within the limits. Anything
    /// else is refused with a message that says why, which the window puts in the plugin log.
    pub fn from_json(v: &Value) -> Result<PanelAnswer, String> {
        let Some(object) = v.as_object() else {
            return Err(format!("panel must be an object, not {v}"));
        };
        for key in object.keys() {
            if !matches!(key.as_str(), "values" | "text" | "table" | "badge" | "error") {
                return Err(format!("panel has the field `{key}`, which it hasn't"));
            }
        }
        let mut answer = PanelAnswer::default();
        if let Some(values) = object.get("values") {
            answer.values = values_from(values)?;
        }
        if let Some(text) = object.get("text") {
            let Value::String(text) = text else {
                return Err(format!("panel.text must be a string, not {text}"));
            };
            answer.text = Some(cut(text, MAX_PANEL_TEXT));
        }
        if let Some(table) = object.get("table") {
            answer.table = Some(table_from(table)?);
        }
        if let Some(badge) = object.get("badge") {
            match badge {
                Value::Null => {}
                Value::String(badge) => answer.badge = Some(cut(badge, MAX_BADGE_TEXT)),
                other => return Err(format!("panel.badge must be a string or null, not {other}")),
            }
        }
        if let Some(error) = object.get("error") {
            match error {
                Value::Null => {}
                Value::String(error) => answer.error = Some(cut(error, MAX_ERROR_TEXT)),
                other => return Err(format!("panel.error must be a string or null, not {other}")),
            }
        }
        Ok(answer)
    }
}

fn values_from(v: &Value) -> Result<BTreeMap<String, Value>, String> {
    let Some(values) = v.as_object() else {
        return Err(format!("panel.values must be an object, not {v}"));
    };
    if values.len() > MAX_VALUES {
        return Err(format!("panel.values has {} entries; the most is {MAX_VALUES}", values.len()));
    }
    let mut out = BTreeMap::new();
    for (id, value) in values {
        if id.is_empty() || id.chars().count() > 64 {
            return Err(format!("panel.values names a widget `{id}`, which isn't a widget id"));
        }
        out.insert(id.clone(), check_value(value, &format!("panel.values.{id}"))?);
    }
    Ok(out)
}

fn table_from(v: &Value) -> Result<PanelTable, String> {
    let Some(table) = v.as_object() else {
        return Err(format!("panel.table must be an object with `rows`, not {v}"));
    };
    for key in table.keys() {
        if key != "rows" {
            return Err(format!("panel.table has the field `{key}`, which it hasn't"));
        }
    }
    let Some(rows) = table.get("rows") else {
        return Err("panel.table.rows is required".to_string());
    };
    let Some(rows) = rows.as_array() else {
        return Err(format!("panel.table.rows must be an array, not {rows}"));
    };
    if rows.len() > MAX_ROWS {
        return Err(format!("panel.table.rows has {} rows; the most is {MAX_ROWS}", rows.len()));
    }
    let mut out = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let Some(cells) = row.as_array() else {
            return Err(format!("panel.table.rows[{i}] must be an array, not {row}"));
        };
        if cells.len() > MAX_CELLS {
            return Err(format!("panel.table.rows[{i}] has {} cells; the most is {MAX_CELLS}", cells.len()));
        }
        let mut line = Vec::with_capacity(cells.len());
        for (j, cell) in cells.iter().enumerate() {
            line.push(match cell {
                Value::String(s) => Value::String(cut(s, MAX_CELL_TEXT)),
                Value::Number(n) if n.as_f64().is_some_and(f64::is_finite) => cell.clone(),
                Value::Bool(_) | Value::Null => cell.clone(),
                other => {
                    return Err(format!("panel.table.rows[{i}][{j}] must be a string, a number, true, false or null, not {other}"));
                }
            });
        }
        out.push(line);
    }
    Ok(PanelTable { rows: out })
}

/// A cell or a value: a string (cut), a number, a bool or null.
fn check_value(v: &Value, what: &str) -> Result<Value, String> {
    match v {
        Value::String(s) => {
            if s.chars().count() > MAX_VALUE_TEXT {
                return Err(format!("{what} is longer than {MAX_VALUE_TEXT} characters"));
            }
            Ok(Value::String(s.clone()))
        }
        Value::Number(n) if n.as_f64().is_some_and(f64::is_finite) => Ok(v.clone()),
        Value::Bool(_) | Value::Null => Ok(v.clone()),
        other => Err(format!("{what} must be a string, a number, true, false or null, not {other}")),
    }
}

/// `text` cut to `limit` characters, at a character boundary.
fn cut(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect()
}

/// The values of an answer as a call's parameters: the object the window gives the plugin.
pub fn params(values: &BTreeMap<String, Value>) -> Map<String, Value> {
    values.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_panel_answer_is_read_as_the_specification_says() {
        let answer = PanelAnswer::from_json(&json!({
            "values": {"level": "Level 1", "min_area": 12.5, "outer": true},
            "text": "3 rooms, 42.5 m²",
            "table": {"rows": [["Level 1", "42.5"], ["Level 2", 18.0]]},
            "badge": "5",
            "error": null
        }))
        .unwrap();
        assert_eq!(answer.values["level"], json!("Level 1"));
        assert_eq!(answer.text.as_deref(), Some("3 rooms, 42.5 m²"));
        assert_eq!(answer.table.unwrap().rows.len(), 2);
        assert_eq!(answer.badge.as_deref(), Some("5"));
        assert_eq!(answer.error, None, "null is no error line");
    }

    #[test]
    fn the_answer_of_a_panel_is_checked_before_the_window_sees_it() {
        for broken in [
            json!(7),
            json!("text"),
            json!([]),
            json!({"values": []}),
            json!({"values": {"x": {"nested": 1}}}),
            json!({"values": {"": 1}}),
            json!({"text": 7}),
            json!({"table": []}),
            json!({"table": {"rows": {}}}),
            json!({"table": {"rows": [7]}}),
            json!({"table": {"rows": [[1]], "more": 1}}),
            json!({"badge": 5}),
            json!({"error": []}),
            json!({"unknown": 1}),
        ] {
            assert!(PanelAnswer::from_json(&broken).is_err(), "{broken} was taken");
        }
        // Every field is optional, and an empty object is an answer that changes nothing.
        assert_eq!(PanelAnswer::from_json(&json!({})).unwrap(), PanelAnswer::default());
    }

    #[test]
    fn a_table_is_held_to_its_limits_and_a_long_cell_is_cut() {
        let long = "x".repeat(MAX_CELL_TEXT + 100);
        let answer = PanelAnswer::from_json(&json!({"table": {"rows": [[long]]}, "text": "y".repeat(MAX_PANEL_TEXT + 1)})).unwrap();
        assert_eq!(answer.table.unwrap().rows[0][0].as_str().unwrap().chars().count(), MAX_CELL_TEXT);
        assert_eq!(answer.text.unwrap().chars().count(), MAX_PANEL_TEXT);
        let many = json!({"table": {"rows": vec![vec![1]; MAX_ROWS + 1]}});
        assert!(PanelAnswer::from_json(&many).is_err());
        let wide = json!({"table": {"rows": [vec![1; MAX_CELLS + 1]]}});
        assert!(PanelAnswer::from_json(&wide).is_err());
    }
}
