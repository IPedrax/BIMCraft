//! Plan symbol sets: families that are a symbol and nothing else, loaded on demand with the `symbol_set` command. The one set
//! so far is IEC 60617 (which the Belgian AREI/RGIE Table 2.23 is based on), redrawn: the names are those printed in its
//! Dutch and French versions, and a symbol is drawn at about 30 mm per drawing point, so a socket is about 25 cm wide on a
//! plan. Their drawings are in `data/iec-60617-symbols.json`, in the families' own `symbols` schema (mm, y up), with
//! parametric variants (`Switch (IEC 60617)`, `Push button`, `Socket (IEC 60617)`) and an instance `Circuit` text parameter.
//! These are plan symbols only for now: no forms, connectors or loads.

use serde_json::{Value, json};

use crate::family::{Category, Family};
use crate::language::Language;

/// The sets a project may load, by the name `symbol_set` takes.
pub const SETS: &[&str] = &["iec60617"];

/// Normalize a set name (`arei` is kept as an alias for `iec60617`).
pub fn canonical_set(set: &str) -> Option<&'static str> {
    match set {
        "iec60617" | "arei" => Some("iec60617"),
        _ => None,
    }
}

/// The language a set's family names are in: English, or the Dutch and French wording of the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Names {
    English,
    Dutch,
    French,
}

impl Names {
    /// By code (`en`, `nl`, `fr`).
    pub fn from_code(code: &str) -> Option<Names> {
        match code.trim().to_lowercase().as_str() {
            "en" => Some(Names::English),
            "nl" => Some(Names::Dutch),
            "fr" => Some(Names::French),
            _ => None,
        }
    }

    /// What a project in `language` gets: the French wording in a French project, the Dutch in a Dutch one, English otherwise.
    pub fn of(language: Language) -> Names {
        match language {
            Language::French => Names::French,
            Language::Dutch => Names::Dutch,
            _ => Names::English,
        }
    }
}

/// The IEC 60617 / AREI-RGIE symbols: English working name, Dutch and French wording, the drawing's layer, optional
/// parameters/types and its `symbols`.
const IEC_60617: &str = include_str!("../data/iec-60617-symbols.json");

/// The category of a symbol by its layer in the source: switches, sockets and luminaires are the electrical categories the
/// discipline tabs place; a data socket is a data outlet; everything else is generic.
fn category(layer: &str, english: &str) -> Category {
    match (layer, english) {
        (_, "Data socket") => Category::DataOutlet,
        ("switch", _) => Category::Switch,
        ("socket", _) => Category::Outlet,
        ("light", _) => Category::Lighting,
        _ => Category::Generic,
    }
}

/// The families of set `set`, named in `names`, as (name, family); none for a set that doesn't exist.
pub fn families(set: &str, names: Names) -> Vec<(String, Family)> {
    if canonical_set(set).is_none() {
        return Vec::new();
    }
    let Ok(Value::Object(file)) = serde_json::from_str::<Value>(IEC_60617) else { return Vec::new() };
    let Some(Value::Object(symbols)) = file.get("symbols") else { return Vec::new() };
    symbols
        .iter()
        .filter_map(|(english, v)| {
            let name = match names {
                Names::English => english.as_str(),
                Names::Dutch => v.get("name_nl")?.as_str()?,
                Names::French => v.get("name_fr")?.as_str()?,
            };
            let layer = v.get("layer")?.as_str()?;
            let mut parameters = v.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default();
            if !parameters.iter().any(|p| p.get("name").and_then(Value::as_str) == Some("Circuit")) {
                parameters.push(json!({"name": "Circuit", "kind": "text", "instance": true, "value": "-"}));
            }
            if !parameters.iter().any(|p| p.get("name").and_then(Value::as_str) == Some("Show Circuit")) {
                parameters.push(json!({"name": "Show Circuit", "kind": "yesno", "instance": true, "value": false}));
            }
            if !parameters.iter().any(|p| p.get("name").and_then(Value::as_str) == Some("Label Position")) {
                parameters.push(json!({"name": "Label Position", "kind": "integer", "instance": true, "value": 0}));
            }
            let types = v.get("types").cloned().unwrap_or_else(|| json!({"Standard": {}}));
            let mut syms = v.get("symbols")?.as_array()?.clone();
            syms.push(json!({
                "text": {
                    "at": [
                        "if(Label Position = 0, 380, if(Label Position = 2, -380, 0))",
                        "if(Label Position = 1, 420, if(Label Position = 3, -420, 0))"
                    ],
                    "height": 120.0,
                    "value": "{Circuit}",
                    "align": "center"
                },
                "visible": "Show Circuit"
            }));
            let forms = v.get("forms").cloned().unwrap_or_else(|| json!([]));
            let connectors = v.get("connectors").cloned().unwrap_or_else(|| json!([]));
            let default_elevation = v.get("default_elevation").and_then(Value::as_f64).unwrap_or(0.0);
            let family = json!({
                "category": category(layer, english),
                "parameters": parameters,
                "types": types,
                "forms": forms,
                "connectors": connectors,
                "default_elevation": default_elevation,
                "symbols": syms,
            });
            Some((name.to_string(), serde_json::from_value(family).ok()?))
        })
        .collect()
}

#[cfg(test)]
mod tests;
