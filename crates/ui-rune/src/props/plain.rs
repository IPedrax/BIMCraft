//! The Properties panel's rows: what an element's JSON (`bimcraft_core::element_json`, as agents read it) says, as a name and a
//! value to show. Read-only: a change to a property goes through `Session::execute` (`modify`), never through these rows.
//! Lengths come as millimetres, as every file and command holds them; showing them in the project's units
//! (`Units::length`) is the egui panel's job today and moves to the core when this one needs it (docs/areas/rune-ui.md).

use serde_json::Value;

/// The most rows and the longest value one element shows: a hostile file's element is not a way to fill the screen.
const MAX_ROWS: usize = 200;
const MAX_VALUE: usize = 120;
const MAX_LIST: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub value: String,
}

/// `v` for a person: whole numbers whole, other numbers to two decimals, yes or no, a short list's items joined; nothing for null.
fn show(v: &Value) -> Option<String> {
    let text = match v {
        Value::Null => return None,
        Value::Bool(b) => (if *b { "yes" } else { "no" }).to_string(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => n.as_f64().map_or_else(|| n.to_string(), |f| format!("{f:.2}").trim_end_matches('0').trim_end_matches('.').to_string()),
        },
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let shown: Vec<String> = items.iter().take(MAX_LIST).filter_map(show).collect();
            let more = if items.len() > MAX_LIST { ", …" } else { "" };
            format!("{}{more}", shown.join(", "))
        }
        Value::Object(fields) => format!("{{{} fields}}", fields.len()),
    };
    Some(cut(text))
}

/// `text` held to [`MAX_VALUE`] characters.
fn cut(text: String) -> String {
    if text.chars().count() <= MAX_VALUE {
        return text;
    }
    let mut short: String = text.chars().take(MAX_VALUE - 1).collect();
    short.push('…');
    short
}

/// What a field holds, for reading what a person typed back into the JSON the core takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Number,
    Bool,
}

/// The fields a person may try to change: the scalars of the JSON but its identity. Some are derived (a length, an area) and the core
/// refuses them with its reason, which is better than a list of ours that drifts from the core's.
const READ_ONLY: [&str; 3] = ["id", "guid", "category"];
const MAX_TEXT: usize = 10_000;

/// (name, kind, the text to start editing from) of each field of an element's JSON that can be tried, in the order of [`rows`].
pub fn editable(json: &Value) -> Vec<(String, Kind, String)> {
    let Value::Object(fields) = json else { return Vec::new() };
    fields
        .iter()
        .filter(|(name, _)| !READ_ONLY.contains(&name.as_str()))
        .filter_map(|(name, v)| match v {
            Value::String(s) => Some((name.clone(), Kind::Text, s.chars().take(MAX_TEXT).collect())),
            Value::Number(n) => Some((name.clone(), Kind::Number, n.to_string())),
            Value::Bool(b) => Some((name.clone(), Kind::Bool, b.to_string())),
            _ => None,
        })
        .take(MAX_ROWS)
        .collect()
}

/// What was typed, as the JSON value of a field of this kind, or why it is none.
pub fn parse(kind: Kind, typed: &str) -> Result<Value, String> {
    let typed = typed.trim();
    match kind {
        Kind::Text => Ok(Value::String(typed.to_string())),
        Kind::Number => {
            let n: f64 = typed.parse().map_err(|_| format!("{typed:?} is not a number"))?;
            if !n.is_finite() {
                return Err(format!("{typed:?} is not a number"));
            }
            if n.fract() == 0.0 && n.abs() < 9e15 {
                Ok(Value::from(n as i64))
            } else {
                serde_json::Number::from_f64(n).map(Value::Number).ok_or_else(|| format!("{typed:?} is not a number"))
            }
        }
        Kind::Bool => match typed.to_lowercase().as_str() {
            "yes" | "true" | "1" | "on" => Ok(Value::Bool(true)),
            "no" | "false" | "0" | "off" => Ok(Value::Bool(false)),
            _ => Err(format!("{typed:?} is not yes or no")),
        },
    }
}

/// The rows of an element's JSON: its `id` first, then its fields by name; an empty list for what isn't an object.
pub fn rows(json: &Value) -> Vec<Row> {
    let Value::Object(fields) = json else { return Vec::new() };
    let mut all: Vec<(&String, &Value)> = fields.iter().collect();
    // Stable: the id first, the rest in the order the map keeps them (by name).
    all.sort_by_key(|(name, _)| name.as_str() != "id");
    all.into_iter().filter_map(|(name, v)| show(v).map(|value| Row { name: cut(name.clone()), value })).take(MAX_ROWS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn value<'a>(rows: &'a [Row], name: &str) -> Option<&'a str> {
        rows.iter().find(|r| r.name == name).map(|r| r.value.as_str())
    }

    #[test]
    fn an_element_shows_its_id_first_then_its_fields_in_a_form_a_person_reads() {
        let r = rows(&json!({
            "category": "wall", "id": 100234, "length": 3200.456, "height": 2700.0, "open": true, "gone": null,
            "tags": ["a", "b"], "nested": {"x": 1, "y": 2}
        }));
        assert_eq!(r[0], Row { name: "id".into(), value: "100234".into() });
        assert_eq!(value(&r, "length"), Some("3200.46"));
        assert_eq!(value(&r, "height"), Some("2700"), "2700.0 reads as 2700");
        assert_eq!(value(&r, "open"), Some("yes"));
        assert_eq!(value(&r, "tags"), Some("a, b"));
        assert_eq!(value(&r, "nested"), Some("{2 fields}"));
        assert_eq!(value(&r, "gone"), None, "a null is no row");
        assert_eq!(r.len(), 7);
    }

    #[test]
    fn the_scalars_but_the_identity_can_be_tried_and_what_is_typed_is_read_back_by_kind() {
        let f = editable(
            &json!({"id": 1, "guid": "g", "category": "wall", "name": "A", "height": 2700.5, "mark": 3, "open": true, "tags": ["x"], "gone": null}),
        );
        let names: Vec<&str> = f.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(names, ["height", "mark", "name", "open"]);
        assert_eq!(f[0], ("height".to_string(), Kind::Number, "2700.5".to_string()));
        assert_eq!(parse(Kind::Number, " 2700 "), Ok(json!(2700)));
        assert_eq!(parse(Kind::Number, "2700.5"), Ok(json!(2700.5)));
        for bad in ["", "abc", "NaN", "inf", "-inf", "1e999"] {
            assert!(parse(Kind::Number, bad).is_err(), "{bad:?}");
        }
        assert_eq!(parse(Kind::Bool, "Yes"), Ok(json!(true)));
        assert_eq!(parse(Kind::Bool, "off"), Ok(json!(false)));
        assert!(parse(Kind::Bool, "maybe").is_err());
        assert_eq!(parse(Kind::Text, "  Ground "), Ok(json!("Ground")));
        assert!(editable(&json!([1])).is_empty());
    }

    #[test]
    fn hostile_json_is_cut_to_a_screenful_and_a_non_object_has_no_rows() {
        let long = "x".repeat(10_000);
        let mut big = serde_json::Map::new();
        for i in 0..1000 {
            big.insert(format!("field{i:04}"), json!(i));
        }
        big.insert("text".into(), json!(long));
        let r = rows(&Value::Object(big));
        assert_eq!(r.len(), MAX_ROWS);
        let list: Vec<i32> = (0..100).collect();
        let r = rows(&json!({"text": "y".repeat(10_000), "list": list}));
        assert!(r.iter().all(|row| row.value.chars().count() <= MAX_VALUE));
        assert_eq!(value(&r, "list"), Some("0, 1, 2, 3, 4, 5, …"));
        for v in [json!(null), json!(3), json!("x"), json!([1])] {
            assert!(rows(&v).is_empty());
        }
    }
}
