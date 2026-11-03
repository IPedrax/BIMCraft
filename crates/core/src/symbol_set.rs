//! Plan symbol sets: families that are a symbol and nothing else, loaded on demand with the `symbol_set` command. The one set
//! so far is the Belgian AREI/RGIE's, the graphical symbols of Table 2.23 (FOD Economie, Book 1), redrawn: the names are those
//! printed in its Dutch and French versions, and a symbol is drawn at about 30 mm per drawing point, so a socket is about
//! 25 cm wide on a plan. Their drawings are in `data/arei-symbols.json`, in the families' own `symbols` schema (mm, y up).
//! These are plan symbols only for now: no forms, connectors or loads.

use serde_json::{Value, json};

use crate::family::{Category, Family};
use crate::language::Language;

/// The sets a project may load, by the name `symbol_set` takes.
pub const SETS: &[&str] = &["arei"];

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

/// The Belgian AREI/RGIE symbols: English working name, Dutch and French wording, the drawing's layer and its `symbols`.
const AREI: &str = include_str!("../data/arei-symbols.json");

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
    if set != "arei" {
        return Vec::new();
    }
    let Ok(Value::Object(file)) = serde_json::from_str::<Value>(AREI) else { return Vec::new() };
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
            let family = json!({"category": category(layer, english), "types": {"Standard": {}}, "symbols": v.get("symbols")?});
            Some((name.to_string(), serde_json::from_value(family).ok()?))
        })
        .collect()
}

#[cfg(test)]
mod tests;
