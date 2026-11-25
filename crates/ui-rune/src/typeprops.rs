//! Type Properties, the pure part: the project's types (`Model::types`, what a file writes of them) as a form. A type is a library
//! entry with a name in a category; its scalar fields (a wall's thickness, a door's width and sill) are the ones a person types, the
//! rest (a wall's layers) is shown and left to the commands made for it. What is changed goes to the core as one `type` command with
//! just the fields that differ, which "creates the type, or changes it everywhere it's used". The dialog is `ui/types.rs`.

use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use serde_json::{Map, Value};

use crate::props::{self, Kind, Row};

/// A type: its category as the `type` command says it (`wall`, `structuralcolumn`, `beam`) and its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeKey {
    pub category: String,
    pub name: String,
}

/// A name with no case and no separators, so `structural_column`, `StructuralColumn` and `structuralcolumn` are one.
fn word(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '_' | ' ' | '-')).flat_map(char::to_lowercase).collect()
}

/// Every type of the project with its fields as the file writes them. The runs (beams, pipes, conduits…) are kept by category, then
/// name, in the file; here each is of its own category like the rest.
pub fn all(model: &Model) -> Vec<(TypeKey, Value)> {
    let Ok(Value::Object(categories)) = serde_json::to_value(&*model.types) else { return Vec::new() };
    let mut found = Vec::new();
    let mut take = |category: &str, types: &Value| {
        if let Value::Object(types) = types {
            for (name, fields) in types {
                found.push((TypeKey { category: word(category), name: name.clone() }, fields.clone()));
            }
        }
    };
    for (category, types) in &categories {
        match (category.as_str(), types) {
            ("runs", Value::Object(runs)) => runs.iter().for_each(|(run, types)| take(run.as_str(), types)),
            _ => take(category.as_str(), types),
        }
    }
    found
}

/// The names of the types of a category, in order.
pub fn names(model: &Model, category: &str) -> Vec<String> {
    let want = word(category);
    all(model).into_iter().filter(|(k, _)| k.category == want).map(|(k, _)| k.name).collect()
}

/// The type the element `id` is of, when it has one the project holds.
pub fn of_element(model: &Model, id: Id) -> Option<TypeKey> {
    let json = bimcraft_core::element_json(model, id);
    let (category, name) = (word(json["category"].as_str()?), json["type"].as_str()?);
    all(model).into_iter().map(|(k, _)| k).find(|k| k.category == category && k.name == name)
}

/// A type's fields: the ones that can be typed (name, kind, the text to start from) and every row to show.
pub struct Fields {
    pub json: Value,
    pub editable: Vec<(String, Kind, String)>,
    pub rows: Vec<Row>,
}

pub fn fields(model: &Model, key: &TypeKey) -> Option<Fields> {
    let (_, json) = all(model).into_iter().find(|(k, _)| k == key)?;
    Some(Fields { editable: props::editable(&json), rows: props::rows(&json), json })
}

/// Whether two JSON values are the same thing: numbers by value (a wall's `200.0` is the `200` that was typed).
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// The `type` command's parameters for the fields typed: the category and the name, and each field whose text is not what the type
/// has already. An error says which field could not be read. None when nothing is different.
pub fn changes(key: &TypeKey, original: &Value, typed: &[(String, Kind, String)]) -> Result<Option<Value>, String> {
    let mut params = Map::new();
    for (name, kind, text) in typed {
        let value = props::parse(*kind, text).map_err(|why| format!("{name}: {why}"))?;
        if !original.get(name).is_some_and(|had| same(had, &value)) {
            params.insert(name.clone(), value);
        }
    }
    if params.is_empty() {
        return Ok(None);
    }
    params.insert("category".into(), Value::String(key.category.clone()));
    params.insert("name".into(), Value::String(key.name.clone()));
    Ok(Some(Value::Object(params)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Model {
        let app = crate::app::App::sample();
        (*app.session.model).clone()
    }

    #[test]
    fn the_projects_types_are_listed_by_category_and_name() {
        let model = sample();
        let all = all(&model);
        assert!(!all.is_empty());
        let walls = names(&model, "wall");
        assert!(!walls.is_empty(), "the sample house has wall types");
        assert!(all.iter().all(|(k, _)| !k.category.contains('_') && !k.name.is_empty()), "categories are the command's words");
        assert_eq!(names(&model, "Wall"), walls, "a category is found whatever its case");
        assert!(names(&model, "no such category").is_empty());
        assert!(all.iter().any(|(_, v)| v.get("thickness").is_some()), "some type has a thickness to type");
    }

    #[test]
    fn an_element_is_of_a_type_the_project_holds_and_its_fields_are_typed_by_kind() {
        let model = sample();
        let id = model.elements.iter().map(|(id, _)| *id).find(|id| of_element(&model, *id).is_some_and(|k| k.category == "wall"));
        let id = id.expect("a wall of a type the project holds");
        let key = of_element(&model, id).expect("its type");
        let f = fields(&model, &key).expect("its fields");
        assert!(f.editable.iter().any(|(name, kind, _)| name == "thickness" && *kind == Kind::Number), "{:?}", f.editable);
        assert!(!f.rows.is_empty());
        assert!(fields(&model, &TypeKey { category: "wall".into(), name: "no such type".into() }).is_none());
        assert_eq!(of_element(&model, 1), None, "an id that is no element has no type");
    }

    #[test]
    fn only_what_differs_is_sent_numbers_compared_by_value() {
        let key = TypeKey { category: "wall".into(), name: "W".into() };
        let original = json!({"thickness": 200.0, "name": "x"});
        let typed = |t: &str, n: &str| {
            vec![("thickness".to_string(), Kind::Number, t.to_string()), ("name".to_string(), Kind::Text, n.to_string())]
        };
        assert_eq!(changes(&key, &original, &typed("200", "x")), Ok(None), "200 typed is the 200.0 the type has");
        assert_eq!(
            changes(&key, &original, &typed("250", "x")),
            Ok(Some(json!({"category": "wall", "name": "W", "thickness": 250}))),
            "the field that differs, with the type's own name"
        );
        assert_eq!(
            changes(&key, &original, &typed("200", "y")).map(|p| p.map(|v| v["name"].clone())),
            Ok(Some(json!("W"))),
            "the type's name is the key's"
        );
        assert!(changes(&key, &original, &typed("abc", "x")).is_err_and(|e| e.starts_with("thickness: ")));
        assert!(changes(&key, &original, &typed("NaN", "x")).is_err());
    }
}
